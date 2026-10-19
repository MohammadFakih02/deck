//! Claude Sync: keeps Claude Code's sessions, memory and settings the same on your PCs (ClaudeSync's
//! engine), directly over the local network and/or through a cloud-synced folder. Off by default.
//!
//! When on, Deck adds two small hooks to Claude Code's settings.json (a session open on one PC
//! can't be continued on another; Claude is told which PC it runs on). They run a copy of Deck
//! (`%LOCALAPPDATA%\Deck\claude-sync-hook.exe --claude-sync-hook`), and switching Claude Sync off
//! removes them again.

use crate::util::LockAnyway;
pub mod canon;
pub mod config;
pub mod crypto;
pub mod engine;
pub mod guard;
pub mod hub;
pub mod lan;
pub mod locks;
pub mod model;
pub mod scan;
pub mod sessions;
pub mod winutil;

use crate::app::App;
use config::{Cmd, Shared, HOME_ALIAS};
use lan::Lan;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

pub struct ClaudeSync {
    pub sh: Arc<Shared>,
    pub lan: Arc<Lan>,
}

static C: OnceLock<ClaudeSync> = OnceLock::new();
static APP: OnceLock<Arc<App>> = OnceLock::new();

/// A change that would alter what Claude Code runs or may do on this PC is waiting (guard.rs):
/// a notification that stays until it is answered; gone once nothing waits.
pub fn held_changed(sh: &Shared) {
    let Some(app) = APP.get() else { return };
    let waiting = sh.undecided();
    if waiting.is_empty() {
        app.dismiss_toast("ccsync-held");
    } else {
        let who = waiting[0].1.clone();
        let what: Vec<String> = waiting.iter().map(|(p, _)| p.clone()).take(3).collect();
        app.toast(crate::app::Toast {
            id: "ccsync-held".into(),
            kind: "warn".into(),
            title: format!("Claude Code settings from {who} are waiting"),
            body: format!(
                "{} would change what Claude Code runs or may do on this PC (hooks, permissions, plugins…). Apply only if you made this change.",
                what.join(", ")
            ),
            actions: vec![("apply".into(), "Apply".into()), ("keep".into(), "Keep this PC's".into())],
            sticky: true,
            sound: false,
        });
    }
    app.ui_changed("claude_sync");
}

/// The answer to `held_changed`'s notification (or the Claude Code sync page): apply the waiting
/// changes, or keep this PC's versions (not asked again until they change once more).
pub fn decide_held(apply: bool) {
    let Some(c) = C.get() else { return };
    c.sh.decide_held(apply);
    c.sh.log(if apply { "Applied the waiting Claude Code settings" } else { "Kept this PC's Claude Code settings" });
    c.sh.cmds.lock_anyway().push(Cmd::SyncNow);
    c.sh.wake();
    held_changed(&c.sh);
}

/// `deck.exe --claude-sync-hook [--claude-dir D] [--data-dir D]`: Claude Code's hook. Decides fast
/// and quietly; never starts Deck.
pub fn hook_main() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let paths = config::Paths::new(opt("--claude-dir"), opt("--data-dir"), None);
    locks::hook(&paths.data, &paths.claude)
}

/// The standalone ClaudeSync app is running: two sync engines on the same Claude folder would
/// fight over it, so Deck's stays off until it is closed.
fn standalone_running() -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let found = sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case("ClaudeSync.exe"));
    found
}

/// First start: take the sync key, this PC's name, the project folders and what to sync from the
/// standalone ClaudeSync, if it was used here (machines still need Deck on both ends).
fn import_standalone(data: &Path) {
    if data.join("config.json").exists() {
        return;
    }
    let Some(old) = std::env::var("APPDATA").ok().map(|a| Path::new(&a).join("ClaudeSync").join("config.json")) else { return };
    let Ok(b) = std::fs::read(&old) else { return };
    let Ok(old): Result<config::Config, _> = serde_json::from_slice(&b) else { return };
    let mut c = config::Config::default();
    c.sync_key = old.sync_key;
    c.machine_name = old.machine_name;
    c.projects = old.projects;
    c.manual_peers = old.manual_peers.iter().map(|p| p.split(':').next().unwrap_or(p).to_string()).collect();
    c.sync_sessions = old.sync_sessions;
    c.sync_memory = old.sync_memory;
    c.sync_config = old.sync_config;
    c.session_lock = old.session_lock;
    c.machine_note = old.machine_note;
    c.interval_secs = old.interval_secs;
    let _ = std::fs::create_dir_all(data);
    let _ = std::fs::write(data.join("config.json"), serde_json::to_vec_pretty(&c).unwrap_or_default());
}

/// Starts the engine (once) when Claude Sync is on; switching it off removes the hooks and idles it.
pub fn set_on(app: &Arc<App>, on: bool) -> Result<(), String> {
    if on && standalone_running() {
        return Err("The ClaudeSync app is running on this PC. Close it first (its tray icon → Exit): two sync apps on the same Claude folder would get in each other's way.".into());
    }
    if let Some(c) = C.get() {
        c.sh.off.store(!on, Ordering::Relaxed);
        if !on {
            c.lan.peers.lock_anyway().clear();
        } else {
            c.sh.cmds.lock_anyway().push(Cmd::SyncNow);
        }
        c.sh.wake();
        app.ui_changed("claude_sync");
        return Ok(());
    }
    if !on {
        return Ok(());
    }
    let _ = APP.set(app.clone());
    let paths = config::Paths::new(None, None, None);
    import_standalone(&paths.data);
    let sh = Arc::new(Shared::new(paths));
    let lan = Arc::new(Lan::new());
    if C.set(ClaudeSync { sh: sh.clone(), lan: lan.clone() }).is_err() {
        return Ok(());
    }
    // a hook asks for a sync right away (a session was opened that another PC has newer messages for)
    {
        let sh = sh.clone();
        std::thread::spawn(move || {
            use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
            use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
            use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
            // Deck usually runs as admin and Claude Code doesn't: without this the hook (normal
            // rights, medium integrity) can't signal an event an elevated process made
            let mut sd = std::ptr::null_mut();
            let ok = unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(winutil::wide("D:(A;;0x1F0003;;;WD)S:(ML;;NW;;;ME)").as_ptr(), 1, &mut sd, std::ptr::null_mut()) } != 0;
            let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd, bInheritHandle: 0 };
            let evt = unsafe { CreateEventW(if ok { &sa } else { std::ptr::null() }, 0, 0, winutil::wide(&locks::sync_event_name(&sh.paths.data)).as_ptr()) } as usize;
            if evt == 0 {
                return;
            }
            loop {
                unsafe { WaitForSingleObject(evt as _, INFINITE) };
                sh.cmds.lock_anyway().push(Cmd::SyncNow);
                sh.wake();
            }
        });
    }
    sh.log(format!("Claude Sync started on {} (Claude folder {})", sh.cfg().machine_name, sh.paths.claude.display()));
    std::thread::spawn(crate::remote::allow_between_pcs);
    lan::start(sh.clone(), lan.clone(), None);
    std::thread::Builder::new().name("claude-sync".into()).spawn(move || engine::Engine::new(sh, lan).run()).ok();
    app.ui_changed("claude_sync");
    Ok(())
}

/// When Deck starts with Claude Sync off: hooks a previous run left behind (Deck closed while it
/// was on, then switched off in the settings file) are taken out. Cheap: only reads settings.json.
pub fn cleanup_when_off() {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let claude = std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|s| !s.is_empty()).map(std::path::PathBuf::from).unwrap_or_else(|| Path::new(&home).join(".claude"));
    for event in ["UserPromptSubmit", "SessionStart"] {
        let _ = locks::set_hook(&claude, event, None);
    }
}

/// Deck is being uninstalled: its Claude Code hooks out of settings.json, and the hook program gone.
pub fn uninstall_cleanup() {
    cleanup_when_off();
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let exe = locks::hook_exe(&home);
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(exe.with_extension("old.exe"));
}

fn ago_ms(ms: i64) -> i64 {
    if ms <= 0 {
        -1
    } else {
        model::now_ms() - ms
    }
}

/// Everything the Claude Sync page shows.
pub fn state(app: &App) -> Value {
    let on = app.settings.lock().claude_sync_on;
    let Some(c) = C.get() else {
        return json!({ "on": false, "standalone": standalone_running() });
    };
    let sh = &c.sh;
    let cfg = sh.cfg();
    let st = sh.status.lock_anyway().clone();
    let live: Vec<String> = c.lan.live_peers().into_iter().map(|p| p.id).collect();
    let now = model::now_ms();
    let overrides = sh.overrides.lock_anyway().clone();
    let machines: Vec<Value> = st
        .machines
        .iter()
        .map(|m| {
            json!({ "id": m.id, "name": m.name, "via": m.via, "ago": ago_ms(m.generated_at), "files": m.files, "online": live.contains(&m.id),
                    "version": config::app_version(&m.app),
                    // the same version number, another build (a test build, say): not "older"
                    "same": config::app_version(&m.app) == config::app_version(&config::app_id()), "cmp": match config::compare_app(&m.app) { std::cmp::Ordering::Less => -1, std::cmp::Ordering::Equal => 0, _ => 1 } })
        })
        .collect();
    let projects: Vec<Value> = cfg
        .projects
        .iter()
        .map(|(a, p)| {
            let elsewhere: Vec<Value> = st.machines.iter().filter_map(|m| m.aliases.get(a).map(|path| json!({ "machine": m.name, "path": path }))).collect();
            json!({ "alias": a, "home": a == HOME_ALIAS, "path": p.path, "auto": p.auto, "origin": p.origin, "sync": p.sync, "exists": Path::new(&p.path).is_dir(),
                    "sessions": st.sessions_per_alias.get(a).copied().unwrap_or(0), "elsewhere": elsewhere })
        })
        .collect();
    let open_here: Vec<Value> = st.open_here.iter().map(|(sid, r, alias)| json!({ "sid": sid, "name": r.name, "alias": alias, "started": r.started, "pending": st.pending.get(sid) })).collect();
    let open_elsewhere: Vec<Value> = st
        .open_elsewhere
        .iter()
        .map(|(sid, r, alias)| json!({ "sid": sid, "name": r.name, "alias": alias, "machine": r.machine, "started": r.started, "taken": overrides.get(sid).map(|&t| t > now).unwrap_or(false) }))
        .collect();
    let sessions: Vec<Value> = st
        .sessions
        .iter()
        .map(|r| {
            let open_here = st.open_here.iter().any(|(s, _, _)| s.eq_ignore_ascii_case(&r.sid));
            let open_there = st.open_elsewhere.iter().find(|(s, _, _)| s.eq_ignore_ascii_case(&r.sid)).map(|(_, l, _)| l.machine.clone());
            let waiting = st.pending.iter().find(|(s, _)| s.eq_ignore_ascii_case(&r.sid)).map(|(_, m)| m.clone());
            let online: Vec<bool> = r.elsewhere.iter().map(|n| st.machines.iter().any(|m| &m.name == n && live.contains(&m.id))).collect();
            json!({ "sid": r.sid, "title": r.meta.title, "first": r.meta.first, "last": r.meta.last, "prompts": r.meta.prompts, "size": r.meta.size,
                    "ago": ago_ms(r.meta.mtime), "alias": r.meta.alias, "here": r.here, "elsewhere": r.elsewhere, "online": online,
                    "open_here": open_here, "open_there": open_there, "waiting": waiting })
        })
        .collect();
    let logs: Vec<String> = sh.log.lock_anyway().iter().rev().take(200).cloned().collect();
    json!({
        "on": on && !sh.off.load(Ordering::Relaxed),
        "standalone": false,
        "phase": st.phase, "last_cycle": ago_ms(st.last_cycle), "last_result": st.last_result, "last_error": st.last_error,
        "lan_port": st.lan_port, "local_files": st.local_files, "local_bytes": st.local_bytes,
        "received": st.received_total, "conflicts": st.conflicts_total, "hook_note": st.hook_note, "pending": st.pending.len(),
        // changes from another PC that would alter what Claude Code runs here, waiting for an OK
        "held": sh.undecided().into_iter().map(|(p, who)| json!({ "path": p, "from": who })).collect::<Vec<_>>(),
        "machines": machines, "projects": projects, "open_here": open_here, "open_elsewhere": open_elsewhere, "sessions": sessions,
        "version": config::app_version(&config::app_id()),
        "cfg": {
            "name": cfg.machine_name, "key": cfg.sync_key, "lan": cfg.lan_enabled, "manual_peers": cfg.manual_peers.join(", "),
            "hub": cfg.hub_enabled, "hub_dir": cfg.hub_dir, "sessions": cfg.sync_sessions, "memory": cfg.sync_memory, "config": cfg.sync_config,
            "interval": cfg.interval_secs, "paused": cfg.paused, "session_lock": cfg.session_lock, "machine_note": cfg.machine_note,
        },
        "onedrive": std::env::var("OneDrive").ok(),
        "claude_dir": sh.paths.claude.display().to_string(), "data": sh.paths.data.display().to_string(),
        "logs": logs,
    })
}

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a[k].as_str().unwrap_or("")
}

/// The Claude Sync page's actions. Returns the new state.
pub fn act(app: &Arc<App>, what: &str, a: &Value) -> Result<Value, String> {
    if what == "on" {
        let on = a["on"].as_bool().unwrap_or(false);
        set_on(app, on)?;
        app.settings.lock().claude_sync_on = on;
        app.save_settings();
        return Ok(state(app));
    }
    let c = C.get().ok_or("Claude Code sync is off")?;
    let sh = &c.sh;
    let push = |cmd: Cmd| {
        sh.cmds.lock_anyway().push(cmd);
        sh.wake();
    };
    match what {
        "sync_now" => push(Cmd::SyncNow),
        "held" => decide_held(a["apply"].as_bool().unwrap_or(false)),
        "pause" => {
            let p = a["paused"].as_bool().unwrap_or(false);
            sh.set_cfg(|c| c.paused = p);
            sh.log(if p { "Paused" } else { "Resumed" });
            sh.wake();
        }
        "take_over" => {
            sh.overrides.lock_anyway().insert(s(a, "sid").to_string(), model::now_ms() + 6 * 3600 * 1000);
            push(Cmd::SyncNow);
        }
        "take_back" => {
            sh.overrides.lock_anyway().remove(s(a, "sid"));
            sh.wake();
        }
        "remap" => {
            let (alias, path) = (s(a, "alias").to_string(), s(a, "path").trim().to_string());
            if alias == HOME_ALIAS || path.is_empty() {
                return Err("Pick a folder".into());
            }
            push(Cmd::Remap { alias, path });
        }
        "project_sync" => {
            let (alias, on) = (s(a, "alias").to_string(), a["on"].as_bool().unwrap_or(true));
            sh.set_cfg(|c| {
                if let Some(p) = c.projects.get_mut(&alias) {
                    p.sync = on;
                }
            });
            sh.wake();
        }
        "project_delete" => {
            let alias = s(a, "alias").to_string();
            if alias == HOME_ALIAS {
                return Err("The home folder can't be deleted".into());
            }
            push(Cmd::DeleteProject { alias });
        }
        "session_delete" => push(Cmd::DeleteSession { sid: s(a, "sid").to_string(), everywhere: a["everywhere"].as_bool().unwrap_or(false) }),
        "open_data" => {
            let _ = std::process::Command::new("explorer.exe").arg(&sh.paths.data).spawn();
        }
        "set" => {
            let v = &a["value"];
            let key = s(a, "key").to_string();
            let mut err = None;
            let old_key = sh.cfg().sync_key;
            sh.set_cfg(|c| match key.as_str() {
                "name" => match v.as_str().map(str::trim).filter(|x| !x.is_empty()) {
                    Some(n) => c.machine_name = n.chars().take(40).collect(),
                    None => err = Some("Type a name"),
                },
                "key" => match v.as_str().map(str::trim).map(|k| (k, crate::kdf::too_easy(k))) {
                    Some((k, None)) => c.sync_key = k.into(),
                    Some((_, Some(e))) => err = Some(e),
                    None => err = Some("Type the sync key"),
                },
                "new_key" => c.sync_key = crypto::gen_sync_key(),
                "lan" => c.lan_enabled = v.as_bool().unwrap_or(true),
                "manual_peers" => c.manual_peers = v.as_str().unwrap_or("").split([',', ';', ' ']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                "hub" => c.hub_enabled = v.as_bool().unwrap_or(false),
                "hub_dir" => c.hub_dir = v.as_str().unwrap_or("").trim().to_string(),
                "sessions" => c.sync_sessions = v.as_bool().unwrap_or(true),
                "memory" => c.sync_memory = v.as_bool().unwrap_or(true),
                "config" => c.sync_config = v.as_bool().unwrap_or(true),
                "session_lock" => c.session_lock = v.as_bool().unwrap_or(true),
                "machine_note" => c.machine_note = v.as_bool().unwrap_or(true),
                "interval" => c.interval_secs = v.as_u64().unwrap_or(20).clamp(5, 3600),
                _ => err = Some("Unknown setting"),
            });
            if let Some(e) = err {
                return Err(e.into());
            }
            if sh.cfg().sync_key != old_key {
                c.lan.peers.lock_anyway().clear();
                sh.status(|s| s.machines.clear());
            }
            sh.log("Settings saved");
            push(Cmd::SyncNow);
        }
        _ => return Err("Unknown action".into()),
    }
    Ok(state(app))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// one engine with its own Claude folder, home and data, listening on `port`, knowing the other
    fn engine(root: &Path, name: &str, port: u16, peer_port: u16) -> (Arc<Shared>, PathBuf, PathBuf) {
        let home = root.join(name).join("home");
        let claude = home.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let paths = config::Paths::new(Some(claude.display().to_string()), Some(root.join(name).join("data").display().to_string()), Some(home.display().to_string()));
        let sh = Arc::new(Shared::new(paths));
        sh.set_cfg(|c| {
            c.machine_name = name.into();
            c.sync_key = "test-sync-key-0123456789".into();
            c.lan_port = port;
            c.manual_peers = vec![format!("127.0.0.1:{peer_port}")];
            c.interval_secs = 5;
            // no hooks in a test: nothing touches a real settings.json
            c.session_lock = false;
            c.machine_note = false;
        });
        let lan = Arc::new(Lan::new());
        lan::start(sh.clone(), lan.clone(), Some(port));
        let (s2, l2) = (sh.clone(), lan.clone());
        std::thread::spawn(move || engine::Engine::new(s2, l2).run());
        (sh, home, claude)
    }

    /// A session made on PC A arrives on PC B in B's own project folder, with A's paths in it
    /// rewritten to B's; Claude Code's hooks are never added while their settings are off.
    #[test]
    #[ignore = "opens network ports; run with --ignored"]
    fn two_engines_sync_a_session() {
        let root = std::env::temp_dir().join(format!("deck-csync-test-{}", model::now_ms()));
        let (a, home_a, claude_a) = engine(&root, "PCA", 47911, 47912);
        let (b, home_b, claude_b) = engine(&root, "PCB", 47912, 47911);
        // a session in A's home folder project
        let enc = canon::encode_dir(&home_a.display().to_string());
        let sid = "0123abcd-0000-4000-8000-000000000001";
        let dir = claude_a.join("projects").join(&enc);
        std::fs::create_dir_all(&dir).unwrap();
        let line = serde_json::json!({ "type": "user", "sessionId": sid, "cwd": home_a.display().to_string(), "message": { "role": "user", "content": format!("look at {}\\notes.txt", home_a.display()) } });
        std::fs::write(dir.join(format!("{sid}.jsonl")), format!("{line}\n")).unwrap();
        a.wake();
        let want = claude_b.join("projects").join(canon::encode_dir(&home_b.display().to_string())).join(format!("{sid}.jsonl"));
        let t0 = Instant::now();
        while !want.exists() {
            assert!(t0.elapsed() < Duration::from_secs(60), "session didn't arrive; B's log: {:?}", b.log.lock_anyway());
            std::thread::sleep(Duration::from_millis(250));
            a.wake();
            b.wake();
        }
        std::thread::sleep(Duration::from_millis(500));
        let got = std::fs::read_to_string(&want).unwrap();
        // in the JSON text each backslash is written twice
        let json_path = |p: &PathBuf| p.display().to_string().replace('\\', "\\\\");
        assert!(got.contains(&json_path(&home_b)), "paths rewritten to B's home: {got}");
        assert!(!got.contains(&json_path(&home_a)), "no trace of A's home: {got}");
        assert!(!claude_a.join("settings.json").exists() && !claude_b.join("settings.json").exists(), "no hooks added");
        // a settings.json from A that makes Claude Code run a program waits on B for B's OK
        let risky = r#"{"model":"opus","statusLine":{"type":"command","command":"notepad.exe"}}"#;
        std::fs::write(claude_a.join("settings.json"), risky).unwrap();
        let t0 = Instant::now();
        while b.undecided().is_empty() {
            assert!(t0.elapsed() < Duration::from_secs(60), "B never held it; B's log: {:?}", b.log.lock_anyway());
            std::thread::sleep(Duration::from_millis(250));
            a.wake();
            b.wake();
        }
        assert!(!claude_b.join("settings.json").exists(), "not written before B allows it");
        b.decide_held(true);
        let t0 = Instant::now();
        while !claude_b.join("settings.json").exists() {
            assert!(t0.elapsed() < Duration::from_secs(60), "not applied after the OK");
            std::thread::sleep(Duration::from_millis(250));
            b.wake();
        }
        assert!(std::fs::read_to_string(claude_b.join("settings.json")).unwrap().contains("notepad.exe"));
        // switched off: B stops syncing
        b.off.store(true, Ordering::Relaxed);
        b.wake();
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(b.status.lock_anyway().phase, "Off");
        let _ = std::fs::remove_dir_all(&root);
    }
}
