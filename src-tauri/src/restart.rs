//! Coming back after Deck was ended from outside (a crash, a driver installer, Task Manager).
//!
//! The watchdog process (`watchdog.rs`) hands the hardware back to the BIOS, then calls
//! `after_unclean_end`: wait until no driver installer is running, then start Deck again, hidden,
//! through the admin `\Deck\Background` task. Three unclean ends within ten minutes mean something
//! keeps killing Deck: it stops restarting and the next start comes up in safe mode (fans on the BIOS,
//! no overclock) until the user says otherwise, so a crash can never turn into a loop.

use std::path::PathBuf;
use std::time::Duration;

/// How far back unclean ends count towards giving up.
const WINDOW_S: u64 = 600;
/// This many unclean ends within the window: stop restarting.
const MAX_IN_WINDOW: usize = 3;
/// Wait this long after the end before looking for a driver installer (it may be what ended Deck).
const SETTLE: Duration = Duration::from_secs(15);
/// Never wait longer than this for a driver install to finish.
const DRIVER_WAIT_MAX: Duration = Duration::from_secs(15 * 60);
/// Safe mode left behind this long ago no longer applies (the loop it stopped is over).
const SAFE_FOR_S: u64 = 30 * 60;
/// An update mark this old no longer explains an end.
const UPDATE_FOR_S: u64 = 30 * 60;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Restart,
    GiveUp,
}

/// `history`: when Deck ended unclean (seconds), including the end just seen.
pub fn decide(history: &[u64], now: u64) -> Verdict {
    let recent = history.iter().filter(|&&t| now.saturating_sub(t) <= WINDOW_S).count();
    if recent >= MAX_IN_WINDOW {
        Verdict::GiveUp
    } else {
        Verdict::Restart
    }
}

fn file(name: &str) -> PathBuf {
    crate::settings::Settings::dir().join(name)
}

/// Adds `now` to the saved list of unclean ends (a day's worth is kept) and returns it.
fn record(now: u64) -> Vec<u64> {
    let mut h: Vec<u64> = std::fs::read_to_string(file("crashes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    h.retain(|&t| now.saturating_sub(t) < 24 * 3600);
    h.push(now);
    let _ = std::fs::create_dir_all(crate::settings::Settings::dir());
    let _ = crate::util::write_atomic(&file("crashes.json"), serde_json::to_string(&h).unwrap_or_default().as_bytes());
    h
}

/// Safe mode is on: the last start gave up after repeated unclean ends (and it was recent).
pub fn safe_mode() -> bool {
    let Ok(t) = std::fs::read_to_string(file("safe.json")) else { return false };
    let at: u64 = t.trim().parse().unwrap_or(0);
    crate::util::now_s().saturating_sub(at) < SAFE_FOR_S
}

/// Leaves safe mode (the user chose to) and forgets the crash history.
pub fn clear_safe() {
    let _ = std::fs::remove_file(file("safe.json"));
    let _ = std::fs::remove_file(file("crashes.json"));
}

/// Deck's own update is about to close Deck (written by the updater just before it runs the setup):
/// that end is not a crash, and the setup starts Deck again itself.
pub fn mark_updating() {
    let _ = std::fs::create_dir_all(crate::settings::Settings::dir());
    let _ = crate::util::write_atomic(&file("updating"), crate::util::now_s().to_string().as_bytes());
}

/// The update mark is gone (Deck started again) or old.
pub fn clear_updating() {
    let _ = std::fs::remove_file(file("updating"));
}

fn updating_recently() -> bool {
    let Ok(t) = std::fs::read_to_string(file("updating")) else { return false };
    crate::util::now_s().saturating_sub(t.trim().parse().unwrap_or(0)) < UPDATE_FOR_S
}

/// Another Deck (not this process) is running: the setup or the user already started it again.
pub fn other_deck_running() -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let me = std::process::id();
    let name = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "deck.exe".into());
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().any(|p| p.pid().as_u32() != me && p.name().to_string_lossy().eq_ignore_ascii_case(&name))
}

fn set_safe() {
    let _ = std::fs::create_dir_all(crate::settings::Settings::dir());
    let _ = crate::util::write_atomic(&file("safe.json"), crate::util::now_s().to_string().as_bytes());
}

/// Deck's own setup or uninstaller: while one runs, starting Deck again would fight it for the files
/// (it ended Deck on purpose, or had to end it).
pub fn is_own_setup(path: &str) -> bool {
    let p = path.to_lowercase().replace('/', "\\");
    let name = p.rsplit('\\').next().unwrap_or("");
    (name.starts_with("deck_") && name.ends_with("-setup.exe")) || (name == "uninstall.exe" && p.contains("\\deck\\"))
}

pub fn own_setup_running(sys: &mut sysinfo::System) -> bool {
    crate::driver::running(sys, is_own_setup, |n| {
        let n = n.to_lowercase();
        n.ends_with("-setup.exe") || n == "uninstall.exe"
    })
}

/// In the watchdog process, after the hardware was released: decides and, when it should, waits for
/// the driver installer and starts Deck again.
pub fn after_unclean_end(code: Option<u32>) {
    use crate::{elevation, log};
    let s = crate::settings::Settings::load();
    if !s.restart_after_crash {
        log::info("restart after a crash is off: Deck stays closed");
        return;
    }
    if !elevation::is_admin() || !elevation::task_exists(elevation::BACKGROUND_TASK) {
        log::warn("Deck ended unexpectedly and has no admin task to start it again (use Set up once)");
        return;
    }
    // Deck's own update closed it: not a crash, and the setup starts Deck again itself (this only
    // steps in when the setup ends without doing so)
    let updating = updating_recently();
    if updating {
        log::info("Deck was closed by its update");
    } else {
        let now = crate::util::now_s();
        let history = record(now);
        if decide(&history, now) == Verdict::GiveUp {
            set_safe();
            log::warn(format!("Deck ended unexpectedly {} times in 10 minutes (last: {}): not restarting; the next start is a safe one", history.len(), crate::watchdog::describe_exit(code)));
            return;
        }
    }
    let mut sys = sysinfo::System::new();
    let busy = |sys: &mut sysinfo::System| crate::driver::installing() || own_setup_running(sys);
    // a setup seen at once is waited for from the start; otherwise give a driver installer time to show
    if !busy(&mut sys) {
        std::thread::sleep(SETTLE);
    }
    let waited = std::time::Instant::now();
    while busy(&mut sys) && waited.elapsed() < DRIVER_WAIT_MAX {
        std::thread::sleep(Duration::from_secs(3));
    }
    if waited.elapsed() > Duration::from_secs(4) {
        // the setup is done replacing files (and may be starting Deck): give it a moment
        std::thread::sleep(Duration::from_secs(10));
    }
    if other_deck_running() {
        log::info("Deck ended and is already running again");
        return;
    }
    if updating {
        clear_updating();
    }
    if elevation::run_task(elevation::BACKGROUND_TASK) {
        log::info(format!("Deck ended unexpectedly ({}); started again, hidden", crate::watchdog::describe_exit(code)));
    } else {
        log::warn("Deck ended unexpectedly and the admin task would not start it again");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_or_two_unclean_ends_restart() {
        assert_eq!(decide(&[1000], 1000), Verdict::Restart);
        assert_eq!(decide(&[900, 1000], 1000), Verdict::Restart);
    }

    #[test]
    fn three_within_ten_minutes_give_up() {
        assert_eq!(decide(&[500, 700, 1000], 1000), Verdict::GiveUp);
    }

    #[test]
    fn decks_own_setup_is_recognised() {
        assert!(is_own_setup(r"C:\Users\x\AppData\Local\Temp\Deck_2.6.0_x64-setup.exe"));
        assert!(is_own_setup(r"C:\Program Files\Deck\updates\Deck_2.6.0_x64-setup.exe"));
        assert!(is_own_setup(r"C:\Program Files\Deck\uninstall.exe"));
        assert!(!is_own_setup(r"C:\Program Files\Other\uninstall.exe"));
        assert!(!is_own_setup(r"C:\Program Files\Deck\deck.exe"));
        assert!(!is_own_setup(r"C:\Games\Foo-setup.exe"));
    }

    #[test]
    fn old_ones_do_not_count() {
        // two ten-minute-old ends and a new one: a quiet hour between is a new story
        assert_eq!(decide(&[0, 100, 5000], 5000), Verdict::Restart);
        assert_eq!(decide(&[0, 100, 200, 5000], 5000), Verdict::Restart);
        assert_eq!(decide(&[4500, 4700, 5000], 5000), Verdict::GiveUp);
    }
}
