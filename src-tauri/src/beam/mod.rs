//! Transfer: files, folders, installers, apps and setups between your PCs over the local network
//! (AppBeam's engine). Both PCs run Deck with the same pairing code; they find each other by a UDP
//! beacon and talk over an encrypted TCP channel (Deck's own ports, 47833/47834).
//!
//! The engine runs in its own threads; Deck's Transfer page reads `state()` and acts through
//! `act()`. Offers and requests that wait for an answer also show as Deck notifications.

use crate::util::LockAnyway;
pub mod apps;
pub mod cli;
pub mod config;
pub mod crypto;
pub mod device;
pub mod net;
pub mod setup;
pub mod snapshot;
pub mod winutil;

use crate::app::{App, Toast};
use config::{human, parse_list, InboxItem, Shared, XState};
use net::{Lan, Peer, ReqOpts, SendOpts};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

pub struct Transfer {
    pub sh: Arc<Shared>,
    pub lan: Arc<Lan>,
    /// apps installed here that winget knows (None while listing)
    apps: Mutex<Option<Result<Vec<apps::App>, String>>>,
    /// folders waiting to be sent: what they hold [(files, bytes) all, without build output]
    scans: Mutex<Vec<(String, Option<[(u64, u64); 2]>)>>,
    /// "Browse" on the other PC: (peer id, path, listing)
    browse: Mutex<Option<(String, String, Option<Result<Vec<net::Item>, String>>)>>,
    /// a setup snapshot being packed (message shown on the page)
    setup_msg: Mutex<String>,
    /// prompts already shown as notifications
    toasted: Mutex<HashSet<u64>>,
}

static T: OnceLock<Transfer> = OnceLock::new();


/// Starts the engine (once) when Transfer is on; switching it off later stops announcing,
/// answering and accepting (the threads stay, idle).
pub fn set_on(app: &Arc<App>, on: bool) {
    if let Some(t) = T.get() {
        t.sh.off.store(!on, Ordering::Relaxed);
        if !on {
            t.lan.peers.lock_anyway().clear();
        }
        app.ui_changed("transfer");
        return;
    }
    if !on {
        return;
    }
    let sh = Arc::new(Shared::new(config::Paths::new(None)));
    let lan = Arc::new(Lan::new());
    let t = Transfer {
        sh: sh.clone(),
        lan: lan.clone(),
        apps: Mutex::new(None),
        scans: Mutex::new(vec![]),
        browse: Mutex::new(None),
        setup_msg: Mutex::new(String::new()),
        toasted: Mutex::new(HashSet::new()),
    };
    if T.set(t).is_err() {
        return;
    }
    {
        let app = app.clone();
        sh.set_notify(move |attention| {
            app.ui_changed("transfer");
            if attention {
                prompt_toasts(&app);
            }
        });
    }
    sh.log(format!("PC to PC started on {}", sh.cfg().machine_name));
    std::thread::spawn(crate::remote::allow_between_pcs);
    net::start(sh, lan, None);
    // a prompt can also wait for an answer while nobody looks at Deck: keep its notification up
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(700));
        prompt_toasts(&app);
    });
}

/// Offers and requests waiting for an answer, as notifications with the answer buttons.
fn prompt_toasts(app: &Arc<App>) {
    let Some(t) = T.get() else { return };
    let pend = t.sh.pending.lock_anyway().clone();
    let reqs = t.sh.reqs.lock_anyway().clone();
    let mut seen = t.toasted.lock();
    let live: HashSet<u64> = pend.iter().map(|p| p.id).chain(reqs.iter().map(|r| r.id)).collect();
    // answered or timed out elsewhere: their notifications go
    for id in seen.clone().difference(&live) {
        app.dismiss_toast(&format!("xfer:{id}"));
        seen.remove(id);
    }
    for p in pend.iter() {
        if !seen.insert(p.id) {
            continue;
        }
        let mut body = format!("{} file{}, {}", p.files, if p.files == 1 { "" } else { "s" }, human(p.total));
        if p.install {
            body.push_str(", and install it here");
        }
        if p.deletes > 0 {
            body.push_str(&format!(". Also deletes {} file{} here that {} no longer has", p.deletes, if p.deletes == 1 { "" } else { "s" }, p.peer));
        }
        app.toast(Toast {
            id: format!("xfer:{}", p.id),
            kind: "info".into(),
            title: format!("{} wants to send you {}", p.peer, p.label),
            body,
            actions: vec![("accept".into(), if p.install { "Accept and install".into() } else { "Accept".into() }), ("show".into(), "Trust this PC…".into()), ("decline".into(), "Decline".into())],
            sticky: true,
            sound: true,
        });
    }
    for r in reqs.iter() {
        if !seen.insert(r.id) {
            continue;
        }
        app.toast(Toast {
            id: format!("xfer:{}", r.id),
            kind: "info".into(),
            title: format!("{} wants to {}", r.peer, r.what),
            body: "Declines by itself in 60 seconds.".into(),
            actions: vec![("allow".into(), "Allow".into()), ("always".into(), "Always allow".into()), ("deny".into(), "Deny".into())],
            sticky: true,
            sound: true,
        });
    }
}

/// A notification's button, or the page's: answers offer / request `id`. "trust_7", "trust_30",
/// "trust_182" and "trust_always" accept and trust the sending PC for that many days (or always).
pub fn answer(id: u64, action: &str) {
    let Some(t) = T.get() else { return };
    let pend = t.sh.pending.lock_anyway().iter().find(|p| p.id == id).cloned();
    if let Some(p) = pend {
        if let Some(days) = action.strip_prefix("trust_") {
            trust(&t.sh, &p.peer_id, &p.peer, days);
        }
        *p.answer.lock_anyway() = Some(matches!(action, "accept" | "allow") || action.starts_with("trust_"));
    }
    if let Some(r) = t.sh.reqs.lock_anyway().iter().find(|r| r.id == id) {
        *r.answer.lock_anyway() = Some(match action {
            "allow" | "accept" => 1,
            "always" => 3,
            _ => 2,
        });
    }
    t.sh.repaint();
}

/// Transfers from PC `id` come in without asking for `days` ("7", "30", "182") or "always".
fn trust(sh: &Shared, id: &str, name: &str, days: &str) {
    let until = trust_until(days);
    sh.set_cfg(|c| {
        c.trusted.retain(|t| t.id != id);
        c.trusted.push(config::Trust { id: id.into(), name: name.into(), until });
    });
    sh.log(format!("{name} is trusted {}", if until == 0 { "from now on".to_string() } else { format!("for {days} days") }));
}

fn trust_until(days: &str) -> u64 {
    match days {
        "always" => 0,
        d => config::now_ms() + d.parse::<u64>().unwrap_or(30).clamp(1, 3650) * 86_400_000,
    }
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "inno" | "nsis" | "burn" | "exe" => "Installer",
        "msi" => "MSI installer",
        "msix" => "App package",
        "zip" => "Zip archive",
        "setup" => "Setup snapshot",
        "apps" => "App list",
        _ => "",
    }
}

/// The Claude Code skill that teaches Claude the command line (`deck.exe --pc-send` …).
const SKILL: &str = include_str!("skill.md");

fn skill_file() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let claude = std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| Path::new(&home).join(".claude"));
    claude.join("skills").join("deck-pc-to-pc").join("SKILL.md")
}

/// Claude Code knows how to use PC to PC (the skill is installed).
fn skill_on() -> bool {
    skill_file().is_file()
}

fn set_skill(on: bool) -> Result<(), String> {
    let f = skill_file();
    if on {
        std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&f, SKILL).map_err(|e| e.to_string())
    } else {
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_dir(f.parent().unwrap());
        Ok(())
    }
}

/// Everything the Transfer page shows.
pub fn state() -> Value {
    let Some(t) = T.get() else { return json!({ "on": false }) };
    let sh = &t.sh;
    sh.reload_inbox_if_changed();
    let cfg = sh.cfg();
    let peers: Vec<Value> = t.lan.live_peers().iter().map(|p| json!({ "id": p.id, "name": p.name, "ip": p.addr.ip().to_string() })).collect();
    let xfers: Vec<Value> = sh
        .xfers
        .lock()
        .unwrap()
        .iter()
        .rev()
        .map(|x| {
            let (state, err) = match &x.state {
                XState::Waiting => ("waiting", String::new()),
                XState::Active => ("active", String::new()),
                XState::Done => ("done", String::new()),
                XState::Failed(m) => ("failed", m.clone()),
                XState::Cancelled => ("cancelled", String::new()),
                XState::Declined => ("declined", String::new()),
            };
            let secs = x.finished_at.map(|f| f.duration_since(x.started).as_secs_f64()).unwrap_or_else(|| x.started.elapsed().as_secs_f64());
            json!({ "id": x.id, "send": x.send, "peer": x.peer, "label": x.label, "total": x.total, "done": x.done, "files": x.files,
                    "state": state, "error": err, "speed": x.speed, "secs": secs, "note": x.note })
        })
        .collect();
    let pending: Vec<Value> = sh.pending.lock_anyway().iter().map(|p| json!({ "id": p.id, "peer": p.peer, "label": p.label, "files": p.files, "total": p.total, "install": p.install, "deletes": p.deletes, "peer_id": p.peer_id })).collect();
    let reqs: Vec<Value> = sh.reqs.lock_anyway().iter().map(|r| json!({ "id": r.id, "peer": r.peer, "what": r.what, "left": 60u64.saturating_sub(r.since.elapsed().as_secs()) })).collect();
    let inbox: Vec<Value> = sh
        .inbox
        .lock()
        .unwrap()
        .iter()
        .map(|it| {
            let d = Path::new(&it.dir);
            let gone = !d.exists();
            let extra = match it.kind.as_str() {
                "apps" => apps::read_list(d).map(|l| json!({ "apps": l.apps.iter().map(|a| &a.name).collect::<Vec<_>>() })),
                "setup" => snapshot::read_manifest(d).map(|m| json!({ "look": m.look.len(), "wallpaper": m.wallpaper.is_some(), "windhawk": m.windhawk_version, "mods": m.windhawk_mods.len(), "backup": snapshot::has_backup(d) })),
                _ => None,
            };
            json!({ "name": it.name, "dir": it.dir, "entry": it.entry, "kind": it.kind, "kind_label": kind_label(&it.kind), "from": it.from, "size": it.size,
                    "files": it.files, "time": it.time, "note": it.note, "gone": gone, "silent": setup::can_silent(&it.kind), "extra": extra })
        })
        .collect();
    let scans: Vec<Value> = t.scans.lock().iter().map(|(p, s)| json!({ "path": p, "scan": s })).collect();
    let browse = t.browse.lock().as_ref().map(|(peer, path, items)| {
        json!({ "peer": peer, "path": path, "items": items.as_ref().map(|r| match r { Ok(v) => json!(v), Err(e) => json!({ "error": e }) }) })
    });
    let policies: Vec<Value> = {
        let mut devs: Vec<(String, String, String)> = cfg.policies.iter().map(|p| (p.id.clone(), p.name.clone(), p.mode.clone())).collect();
        for p in t.lan.live_peers() {
            if !devs.iter().any(|d| d.0 == p.id) {
                devs.push((p.id.clone(), p.name.clone(), "ask".into()));
            }
        }
        devs.into_iter().map(|(id, name, mode)| json!({ "id": id, "name": name, "mode": mode })).collect()
    };
    let logs: Vec<String> = sh.logs.lock_anyway().iter().rev().take(200).cloned().collect();
    json!({
        "on": !sh.off.load(Ordering::Relaxed),
        "name": cfg.machine_name, "code": cfg.code, "manual_peers": cfg.manual_peers, "inbox_dir": cfg.inbox_dir,
        "ask_before_receive": cfg.ask_before_receive, "trust_install": cfg.trust_install, "shared_folders": cfg.shared_folders,
        "trusted": cfg.trusted.iter().filter(|t| t.until == 0 || t.until > config::now_ms()).map(|t| json!({ "id": t.id, "name": t.name, "until": t.until })).collect::<Vec<_>>(),
        "skip_build": cfg.skip_build, "excludes": cfg.excludes, "dest_exists": cfg.dest_exists, "policies": policies,
        "port": *t.lan.port.lock_anyway(),
        "peers": peers, "xfers": xfers, "pending": pending, "reqs": reqs, "inbox": inbox, "scans": scans, "browse": browse,
        "claude_skill": skill_on(),
        "setup_msg": *t.setup_msg.lock(), "logs": logs, "data": sh.paths.data.display().to_string(),
    })
}

fn peer(t: &Transfer, id: &str) -> Result<Peer, String> {
    let peers = t.lan.live_peers();
    peers.iter().find(|p| p.id == id).or_else(|| if id.is_empty() { peers.first() } else { None }).cloned().ok_or_else(|| "That PC isn't connected right now".to_string())
}

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a[k].as_str().unwrap_or("")
}

fn b(a: &Value, k: &str) -> bool {
    a[k].as_bool().unwrap_or(false)
}

fn paths(a: &Value) -> Vec<PathBuf> {
    a["paths"].as_array().map(|v| v.iter().filter_map(|x| x.as_str()).map(PathBuf::from).collect()).unwrap_or_default()
}

/// Only ever inside the inbox folder (deleting received files).
fn in_inbox(cfg: &config::Config, dir: &str) -> bool {
    let inbox = PathBuf::from(&cfg.inbox_dir);
    let d = PathBuf::from(dir);
    d != inbox && d.starts_with(&inbox) && !dir.contains("..")
}

/// The Transfer page's actions. Returns the new state.
pub fn act(app: &Arc<App>, what: &str, a: &Value) -> Result<Value, String> {
    if what == "on" {
        let on = b(a, "on");
        app.settings.lock().transfer_on = on;
        app.save_settings();
        set_on(app, on);
        return Ok(state());
    }
    let t = T.get().ok_or("PC to PC is off")?;
    let sh = &t.sh;
    match what {
        // files: sent now; folders: counted first (the page shows what goes and the options)
        "send" => {
            let ps = paths(a);
            if ps.is_empty() {
                return Ok(state());
            }
            let p = peer(t, s(a, "peer"))?;
            let folder = ps.iter().any(|x| x.is_dir());
            if folder && !b(a, "confirmed") {
                let key = ps.iter().map(|x| x.display().to_string()).collect::<Vec<_>>().join("|");
                t.scans.lock().retain(|x| x.0 != key);
                t.scans.lock().push((key.clone(), None));
                let ex = parse_list(&sh.cfg().excludes);
                let sh2 = sh.clone();
                std::thread::spawn(move || {
                    let (mut all, mut kept) = ((0u64, 0u64), (0u64, 0u64));
                    for x in &ps {
                        let (a, k) = (net::scan(x, &[]), net::scan(x, &ex));
                        all = (all.0 + a.0, all.1 + a.1);
                        kept = (kept.0 + k.0, kept.1 + k.1);
                    }
                    if let Some(t) = T.get() {
                        if let Some(x) = t.scans.lock().iter_mut().find(|x| x.0 == key) {
                            x.1 = Some([all, kept]);
                        }
                    }
                    sh2.repaint();
                });
                return Ok(state());
            }
            let cfg = sh.cfg();
            let o = SendOpts {
                install: b(a, "install"),
                project: ps.len() == 1 && ps[0].is_dir(),
                exclude: if folder && b(a, "skip") { parse_list(&cfg.excludes) } else { vec![] },
                include: vec![],
                sync: b(a, "sync"),
                delete_extra: b(a, "delete_extra"),
            };
            let key = ps.iter().map(|x| x.display().to_string()).collect::<Vec<_>>().join("|");
            t.scans.lock().retain(|x| x.0 != key);
            net::send_with(sh.clone(), p, ps, o);
        }
        "send_text" => {
            let text = s(a, "text");
            if text.trim().is_empty() {
                return Ok(state());
            }
            let p = peer(t, s(a, "peer"))?;
            let dir = sh.paths.data.join("outgoing");
            let _ = std::fs::create_dir_all(&dir);
            let f = dir.join(format!("Text {}.txt", config::now_ms() % 100000));
            std::fs::write(&f, text).map_err(|e| e.to_string())?;
            net::send(sh.clone(), p, vec![f], false);
        }
        "unstage" => {
            let key = s(a, "path").to_string();
            t.scans.lock().retain(|x| x.0 != key);
        }
        "cancel" => {
            let id = a["id"].as_u64().unwrap_or(0);
            if let Some(x) = sh.xfers.lock_anyway().iter().find(|x| x.id == id) {
                x.cancel.store(true, Ordering::Relaxed);
            }
        }
        "dismiss" => {
            let id = a["id"].as_u64().unwrap_or(0);
            sh.xfers.lock_anyway().retain(|x| x.id != id || !x.state.finished());
        }
        "clear_done" => sh.xfers.lock_anyway().retain(|x| !x.state.finished()),
        "answer" => answer(a["id"].as_u64().unwrap_or(0), s(a, "answer")),
        // the other PC's shared folders
        "browse" => {
            let p = peer(t, s(a, "peer"))?;
            let path = s(a, "path").to_string();
            *t.browse.lock() = Some((p.id.clone(), path.clone(), None));
            let sh2 = sh.clone();
            std::thread::spawn(move || {
                let r = net::list_remote(&sh2, &p, &path);
                if let Some(t) = T.get() {
                    let mut g = t.browse.lock();
                    if let Some(x) = g.as_mut().filter(|x| x.0 == p.id && x.1 == path) {
                        x.2 = Some(r);
                    }
                }
                sh2.repaint();
            });
        }
        "browse_close" => *t.browse.lock() = None,
        "claude_skill" => set_skill(b(a, "on"))?,
        "request" => {
            let p = peer(t, s(a, "peer"))?;
            let cfg = sh.cfg();
            let items: Vec<String> = a["items"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            for w in items.into_iter().filter(|w| !w.trim().is_empty()) {
                net::request(sh.clone(), p.clone(), w.trim().to_string(), ReqOpts { exclude: if b(a, "skip") { parse_list(&cfg.excludes) } else { vec![] }, include: vec![], sync: b(a, "sync"), delete_extra: false });
            }
            *t.browse.lock() = None;
        }
        // apps (winget)
        "apps_scan" => {
            *t.apps.lock() = None;
            let sh2 = sh.clone();
            std::thread::spawn(move || {
                let r = apps::list_installed();
                if let Some(t) = T.get() {
                    *t.apps.lock() = Some(r);
                }
                sh2.repaint();
            });
        }
        "apps_send" => {
            let p = peer(t, s(a, "peer"))?;
            let ids: HashSet<String> = a["ids"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let chosen: Vec<apps::App> = match &*t.apps.lock() {
                Some(Ok(l)) => l.iter().filter(|x| ids.contains(&x.id)).cloned().collect(),
                _ => vec![],
            };
            if chosen.is_empty() {
                return Err("Pick at least one app".into());
            }
            let d = apps::build(&sh.paths.data.join("outgoing"), &sh.cfg().machine_name, &chosen).map_err(|e| format!("Couldn't prepare the list: {e}"))?;
            net::send(sh.clone(), p, vec![d], true);
        }
        // setup snapshots
        "setup_send" => {
            let p = peer(t, s(a, "peer"))?;
            let (look, wp, wh) = (b(a, "look"), b(a, "wallpaper"), b(a, "windhawk"));
            *t.setup_msg.lock() = "Packing your setup…".into();
            let sh2 = sh.clone();
            std::thread::spawn(move || {
                let r = snapshot::build(&sh2.paths.data.join("outgoing"), &sh2.cfg().machine_name, look, wp, wh);
                let msg = match r {
                    Ok(d) => {
                        net::send(sh2.clone(), p, vec![d], false);
                        String::new()
                    }
                    Err(e) => {
                        sh2.log(format!("Setup snapshot failed: {e}"));
                        format!("Couldn't pack the setup: {e}")
                    }
                };
                if let Some(t) = T.get() {
                    *t.setup_msg.lock() = msg;
                }
                sh2.repaint();
            });
        }
        "setup_apply" => setup::apply_setup_async(sh.clone(), s(a, "dir").into(), b(a, "look"), b(a, "wallpaper"), b(a, "windhawk"), b(a, "restart")),
        "setup_undo" => setup::undo_setup_async(sh.clone(), s(a, "dir").into(), true),
        // received items
        "install" => setup::install_async(sh.clone(), s(a, "dir").into(), b(a, "silent")),
        "apps_install" => apps::install_async(sh.clone(), s(a, "dir").into()),
        "show" => winutil::open_folder(Path::new(s(a, "dir"))),
        "open_inbox" => {
            let d = sh.cfg().inbox_dir;
            let _ = std::fs::create_dir_all(&d);
            winutil::open_folder(Path::new(&d));
        }
        "inbox_remove" | "inbox_delete" => {
            let dir = s(a, "dir").to_string();
            if what == "inbox_delete" {
                if !in_inbox(&sh.cfg(), &dir) {
                    return Err("Only files inside the received-files folder can be deleted here".into());
                }
                std::fs::remove_dir_all(&dir).or_else(|e| if Path::new(&dir).exists() { Err(e.to_string()) } else { Ok(()) })?;
            }
            sh.inbox.lock_anyway().retain(|i: &InboxItem| i.dir != dir);
            sh.save_inbox();
        }
        // settings
        "set" => {
            let v = &a["value"];
            let key = s(a, "key").to_string();
            let mut err = None;
            sh.set_cfg(|c| match key.as_str() {
                "name" => match v.as_str().map(str::trim).filter(|x| !x.is_empty()) {
                    Some(n) => c.machine_name = n.chars().take(40).collect(),
                    None => err = Some("Type a name"),
                },
                "inbox_dir" => match v.as_str().map(str::trim).filter(|x| !x.is_empty()) {
                    Some(d) => c.inbox_dir = d.into(),
                    None => err = Some("Pick a folder"),
                },
                "ask_before_receive" => c.ask_before_receive = v.as_bool().unwrap_or(false),
                "trust_install" => c.trust_install = v.as_bool().unwrap_or(false),
                "skip_build" => c.skip_build = v.as_bool().unwrap_or(true),
                "excludes" => c.excludes = v.as_str().unwrap_or("").trim().to_string(),
                "dest_exists" => c.dest_exists = v.as_str().filter(|x| ["merge", "replace", "both"].contains(x)).unwrap_or("merge").into(),
                "manual_peers" => c.manual_peers = parse_list(v.as_str().unwrap_or("")),
                "code" => {
                    let n = crypto::normalize(v.as_str().unwrap_or(""));
                    if n.len() == 20 {
                        c.code = n.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect::<Vec<_>>().join("-");
                    } else {
                        err = Some("A pairing code has 20 letters and digits");
                    }
                }
                "new_code" => c.code = crypto::gen_code(),
                "share_add" => {
                    if let Some(p) = v.as_str().filter(|p| Path::new(p).is_dir()) {
                        if !c.shared_folders.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                            c.shared_folders.push(p.into());
                        }
                    }
                }
                "share_remove" => c.shared_folders.retain(|x| Some(x.as_str()) != v.as_str()),
                "trust" => {
                    let (id, name, days) = (s(v, "id").to_string(), s(v, "name").to_string(), s(v, "days").to_string());
                    c.trusted.retain(|t| t.id != id);
                    if !days.is_empty() {
                        c.trusted.push(config::Trust { id, name, until: trust_until(&days) });
                    }
                }
                "policy" => {
                    let (id, name, mode) = (s(v, "id").to_string(), s(v, "name").to_string(), s(v, "mode").to_string());
                    if ["off", "ask", "always"].contains(&mode.as_str()) {
                        match c.policies.iter_mut().find(|p| p.id == id) {
                            Some(p) => p.mode = mode,
                            None => c.policies.push(config::DevicePolicy { id, name, mode }),
                        }
                    }
                }
                _ => err = Some("Unknown setting"),
            });
            if let Some(e) = err {
                return Err(e.into());
            }
            if key == "code" || key == "new_code" {
                t.lan.peers.lock_anyway().clear();
                sh.log("Pairing code changed");
            }
        }
        _ => return Err("Unknown action".into()),
    }
    Ok(state())
}

/// The installed apps list for the Apps tab (None while it is being made).
pub fn apps_state() -> Value {
    let Some(t) = T.get() else { return Value::Null };
    match &*t.apps.lock() {
        None => json!({ "busy": true }),
        Some(Err(e)) => json!({ "error": e }),
        Some(Ok(l)) => json!({ "apps": l.iter().map(|a| json!({ "id": a.id, "name": a.name, "version": a.version, "runtime": apps::is_runtime(&a.id) })).collect::<Vec<_>>() }),
    }
}

/// What a setup snapshot of this PC would hold (Setup tab).
pub fn setup_preview() -> Value {
    let c = snapshot::preview();
    json!({ "look": c.look, "wallpaper": c.wallpaper, "windhawk": snapshot::windhawk_installed().then(snapshot::windhawk_version).flatten(), "windhawk_installed": snapshot::windhawk_installed(), "mods": c.windhawk })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// one engine with its own folders, listening on `port`, that knows the other at `peer_port`
    fn engine(root: &Path, name: &str, port: u16, peer_port: u16) -> (Arc<Shared>, Arc<Lan>) {
        let sh = Arc::new(Shared::new(config::Paths::new(Some(root.join(name).join("data").display().to_string()))));
        let inbox = root.join(name).join("inbox").display().to_string();
        sh.set_cfg(|c| {
            c.machine_name = name.into();
            c.code = "ABCD-EFGH-JKMN-PQRS-TUVW".into();
            c.inbox_dir = inbox;
            c.manual_peers = vec![format!("127.0.0.1:{peer_port}")];
        });
        *sh.test_answer.lock_anyway() = Some(true);
        let lan = Arc::new(Lan::new());
        net::start(sh.clone(), lan.clone(), Some(port));
        (sh, lan)
    }

    fn wait(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
        let t0 = Instant::now();
        while !ok() {
            assert!(t0.elapsed() < Duration::from_secs(secs), "timed out: {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn done(sh: &Shared) -> usize {
        sh.xfers.lock_anyway().iter().filter(|x| x.state == XState::Done).count()
    }

    /// Two Transfer engines on this PC: a file, a project folder (build output left out) and a
    /// request from a shared folder arrive byte for byte; a wrong pairing code finds nothing.
    #[test]
    #[ignore = "opens network ports; run with --ignored"]
    fn two_engines_send_and_request() {
        let root = std::env::temp_dir().join(format!("deck-transfer-test-{}", config::now_ms()));
        let (a, la) = engine(&root, "PCA", 47901, 47902);
        let (b, _) = engine(&root, "PCB", 47902, 47901);
        wait("peers", 20, || la.live_peers().iter().any(|p| p.name == "PCB"));
        let pb = la.live_peers().into_iter().find(|p| p.name == "PCB").unwrap();

        // a 5 MB file
        let src = root.join("src");
        std::fs::create_dir_all(src.join("proj").join("target")).unwrap();
        let big: Vec<u8> = (0..5_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        std::fs::write(src.join("big.bin"), &big).unwrap();
        net::send(a.clone(), pb.clone(), vec![src.join("big.bin")], false);
        wait("file", 30, || done(&a) >= 1 && b.inbox.lock_anyway().iter().any(|i| i.name.contains("big.bin")));
        let it = b.inbox.lock_anyway().iter().find(|i| i.name.contains("big.bin")).cloned().unwrap();
        let got = std::fs::read(Path::new(&it.dir).join("big.bin")).unwrap();
        assert_eq!(got, big, "file arrives byte for byte");

        // a project folder: build output skipped
        std::fs::write(src.join("proj").join("main.rs"), b"fn main() {}").unwrap();
        std::fs::write(src.join("proj").join("target").join("out.exe"), vec![7u8; 1000]).unwrap();
        let o = SendOpts { project: true, exclude: parse_list("target"), ..Default::default() };
        net::send_with(a.clone(), pb.clone(), vec![src.join("proj")], o);
        wait("folder", 30, || done(&a) >= 2);
        let inbox_b = PathBuf::from(b.cfg().inbox_dir);
        wait("folder lands", 10, || inbox_b.join("proj").join("main.rs").exists());
        assert_eq!(std::fs::read(inbox_b.join("proj").join("main.rs")).unwrap(), b"fn main() {}");
        assert!(!inbox_b.join("proj").join("target").exists(), "build output left out");

        // A pulls from B's shared folder
        let shared = root.join("PCB").join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("notes.txt"), b"from B").unwrap();
        b.set_cfg(|c| c.shared_folders = vec![shared.display().to_string()]);
        let list = net::list_remote(&a, &pb, "").unwrap();
        assert!(list.iter().any(|i| i.name == "shared" && i.dir), "B's shared folder is listed: {:?}", list.iter().map(|i| &i.name).collect::<Vec<_>>());
        net::request(a.clone(), pb.clone(), "shared/notes.txt".into(), ReqOpts { exclude: vec![], include: vec![], sync: false, delete_extra: false });
        let inbox_a = PathBuf::from(a.cfg().inbox_dir);
        wait("request", 30, || a.inbox.lock_anyway().iter().any(|i| i.name.contains("notes.txt")));
        let it = a.inbox.lock_anyway().iter().find(|i| i.name.contains("notes.txt")).cloned().unwrap();
        assert!(Path::new(&it.dir).starts_with(&inbox_a));
        assert_eq!(std::fs::read(Path::new(&it.dir).join("notes.txt")).unwrap(), b"from B");

        // switched off: B stops being found
        b.off.store(true, Ordering::Relaxed);
        la.peers.lock_anyway().clear();
        std::thread::sleep(Duration::from_secs(7));
        assert!(!la.live_peers().iter().any(|p| p.name == "PCB"), "an engine that is off isn't found");
        let _ = std::fs::remove_dir_all(&root);
    }
}
