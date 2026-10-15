//! PC to PC from the command line, for scripts and Claude Code:
//!
//!   deck.exe --pc-list                                  the paired PCs Deck can see now
//!   deck.exe --pc-send <path> [--pc-send <path>…] [--to NAME] [--install] [--all] [--exclude a,b] [--sync] [--delete-extra]
//!   deck.exe --pc-get <path inside a shared folder> --from NAME [--all] [--sync]
//!   common: [--wait SECS] (how long to look for the other PC, default 15) [--json]
//!
//! A folder leaves out build output (Deck's "Skipped names") unless `--all` is given; `--all`
//! sends everything, dependencies such as node_modules or .venv included. Runs beside the Deck
//! that is open: it finds the other PCs on its own but never announces itself or accepts
//! anything, so the PCs keep talking to that Deck. It prints progress and ends with one result
//! line. Exit code: 0 done, 1 failed or declined, 2 no such PC found, 3 PC to PC is off or the
//! arguments are wrong.

use super::config::{human, parse_list, Paths, Shared, XState};
use super::net::{self, Lan, ReqOpts, SendOpts};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn wanted(args: &[String]) -> bool {
    args.iter().any(|a| a == "--pc-list" || a == "--pc-send" || a == "--pc-get")
}

/// A program started from a console (cmd, PowerShell) gets its output there; piped output (Claude
/// Code, scripts) already works.
fn console() {
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h.is_null() || h as isize == -1 {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    console();
    let has = |f: &str| args.iter().any(|a| a == f);
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let all = |f: &str| args.windows(2).filter(|w| w[0] == f).map(|w| w[1].clone()).collect::<Vec<_>>();
    let json = has("--json");
    let say = |what: &str, ok: bool, text: String| {
        if json {
            println!("{}", serde_json::json!({ "result": what, "ok": ok, "message": text }));
        } else {
            println!("{text}");
        }
    };
    if !crate::settings::Settings::load().transfer_on {
        say("off", false, "PC to PC is off in Deck (switch it on on the PC to PC page).".into());
        return 3;
    }
    let paths: Vec<PathBuf> = all("--pc-send").into_iter().map(PathBuf::from).collect();
    if let Some(p) = paths.iter().find(|p| !p.exists()) {
        say("bad-path", false, format!("{} doesn't exist", p.display()));
        return 3;
    }
    if !has("--pc-list") && paths.is_empty() && opt("--pc-get").is_none() {
        say("bad-args", false, "Nothing to send".into());
        return 3;
    }
    let sh = Arc::new(Shared::new(Paths::new(None)));
    sh.quiet.store(true, Ordering::Relaxed);
    let lan = Arc::new(Lan::new());
    net::start(sh.clone(), lan.clone(), None);
    let wait = opt("--wait").and_then(|s| s.parse::<u64>().ok()).unwrap_or(15).clamp(1, 600);
    let name = opt("--to").or_else(|| opt("--from"));

    // the other PC: the one named, or the only one there is
    let t0 = Instant::now();
    let peer = loop {
        let peers = lan.live_peers();
        let found = match &name {
            Some(n) => peers.iter().find(|p| p.name.eq_ignore_ascii_case(n)).cloned(),
            None if peers.len() == 1 && !has("--pc-list") => peers.first().cloned(),
            None => None,
        };
        if found.is_some() || t0.elapsed() > Duration::from_secs(if has("--pc-list") { wait.min(5) } else { wait }) {
            if has("--pc-list") {
                if json {
                    println!("{}", serde_json::json!({ "result": "list", "ok": true, "pcs": peers.iter().map(|p| serde_json::json!({ "name": p.name, "address": p.addr.ip().to_string() })).collect::<Vec<_>>() }));
                } else if peers.is_empty() {
                    println!("No paired PC found. Is Deck open on the other PC with PC to PC on and the same pairing code?");
                } else {
                    for p in &peers {
                        println!("{}\t{}", p.name, p.addr.ip());
                    }
                }
                return 0;
            }
            break found.or_else(|| {
                let names: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();
                let why = if names.is_empty() {
                    "No paired PC found. Is Deck open on the other PC with PC to PC on and the same pairing code?".to_string()
                } else if name.is_some() {
                    format!("{} isn't there. Paired PCs: {}", name.clone().unwrap_or_default(), names.join(", "))
                } else {
                    format!("More than one PC: pick one with --to NAME ({})", names.join(", "))
                };
                say("no-pc", false, why);
                None
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let Some(peer) = peer else { return 2 };

    let cfg = sh.cfg();
    let exclude = if has("--all") {
        vec![]
    } else {
        let mut e = parse_list(&cfg.excludes);
        e.extend(opt("--exclude").map(|x| parse_list(&x)).unwrap_or_default());
        e
    };
    if let Some(path) = opt("--pc-get") {
        net::request(sh.clone(), peer.clone(), path, ReqOpts { exclude, include: vec![], sync: has("--sync"), delete_extra: false });
    } else {
        let project = paths.len() == 1 && paths[0].is_dir();
        let o = SendOpts { install: has("--install"), project, exclude, include: vec![], sync: has("--sync"), delete_extra: has("--delete-extra") };
        net::send_with(sh.clone(), peer.clone(), paths, o);
    }

    // progress every 2 s, then the result
    let mut last = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let Some(x) = sh.xfers.lock().unwrap().first().cloned() else { continue };
        if x.state.finished() {
            let secs = x.finished_at.map(|f| f.duration_since(x.started).as_secs_f64()).unwrap_or(0.0).max(0.01);
            let (what, ok, text) = match &x.state {
                XState::Done => ("done", true, format!("Done: {} ({} file{}, {}) {} {} in {:.0} s", x.label, x.files, if x.files == 1 { "" } else { "s" }, human(x.total), if x.send { "to" } else { "from" }, peer.name, secs)),
                XState::Declined => ("declined", false, format!("{} declined it", peer.name)),
                XState::Cancelled => ("cancelled", false, "Cancelled".to_string()),
                XState::Failed(m) => ("failed", false, format!("Failed: {m}")),
                _ => ("failed", false, "Failed".to_string()),
            };
            if x.state == XState::Done && !x.send {
                let dir = sh.inbox.lock().unwrap().first().map(|i| i.dir.clone()).unwrap_or_default();
                say(what, ok, format!("{text}. Saved in {dir}"));
            } else {
                say(what, ok, text);
            }
            return if ok { 0 } else { 1 };
        }
        if last.elapsed() > Duration::from_secs(2) && !json {
            last = Instant::now();
            match x.state {
                XState::Waiting => println!("Waiting for {} to accept…", peer.name),
                _ => {
                    let left = if x.speed > 1.0 { x.total.saturating_sub(x.done) as f64 / x.speed } else { 0.0 };
                    let left = if left <= 0.0 { String::new() } else if left >= 3600.0 { format!(" · {} h {} min left", left as u64 / 3600, left as u64 % 3600 / 60) } else if left >= 60.0 { format!(" · {} min {} s left", left as u64 / 60, left as u64 % 60) } else { format!(" · {} s left", left as u64) };
                    println!("{} of {} · {}/s{left}{}", human(x.done), human(x.total), human(x.speed as u64), if x.note.is_empty() { String::new() } else { format!(" · {}", x.note) })
                }
            }
        }
    }
}
