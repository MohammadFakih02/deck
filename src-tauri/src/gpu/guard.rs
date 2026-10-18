//! Crash guard: this PC has had hard resets. If it restarted while a GPU overclock was on,
//! Deck starts the GPU at stock instead of applying that overclock again. The CPU's turbo
//! overclock gets the same rule (its own file).

use crate::profiles::GpuProfile;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
struct Mark {
    id: String,
    risky: bool,
    /// unix ms when applied
    at: u64,
    /// Deck quit normally since
    clean: bool,
    /// Deck held this overclock back after a crash; it stays held until applied by hand
    #[serde(default)]
    held: bool,
    /// Deck was closed by hand while this was on (the card keeps it until a restart), so Deck
    /// could not see whether the PC later shut down cleanly
    #[serde(default)]
    quit: bool,
}

fn path() -> std::path::PathBuf {
    crate::settings::Settings::dir().join("gpu-applied.json")
}

fn cpu_path() -> std::path::PathBuf {
    crate::settings::Settings::dir().join("cpu-oc-applied.json")
}

fn read(p: &std::path::Path) -> Option<Mark> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// How a mark file reads: no file (nothing was on), a readable mark, or a file that exists but
/// can't be parsed (a save cut off by the crash this guard is for).
enum MarkFile {
    Missing,
    Mark(Box<Mark>),
    Damaged,
}

fn read_file(p: &std::path::Path) -> MarkFile {
    match std::fs::read_to_string(p) {
        Err(_) => MarkFile::Missing,
        Ok(t) => match serde_json::from_str::<Mark>(&t) {
            Ok(m) => MarkFile::Mark(Box::new(m)),
            Err(_) => MarkFile::Damaged,
        },
    }
}

fn write(p: &std::path::Path, m: &Mark) {
    let _ = crate::util::write_atomic(p, serde_json::to_string(m).unwrap_or_default().as_bytes());
}

/// The CPU turbo overclock went on (`on`) or off.
pub fn cpu_record(on: bool) {
    if !on && read(&cpu_path()).map(|m| !m.risky).unwrap_or(true) {
        return;
    }
    write(&cpu_path(), &Mark { id: "cpu-oc".into(), risky: on, at: crate::util::now_ms(), ..Default::default() });
}

/// True when the CPU overclock must not come back by itself: the PC restarted without Deck
/// quitting while it was on.
pub fn cpu_hold() -> bool {
    cpu_hold_at(&cpu_path())
}

/// `cpu_hold` for the mark file `file`.
fn cpu_hold_at(file: &std::path::Path) -> bool {
    match read_file(file) {
        MarkFile::Missing => false,
        MarkFile::Damaged => true, // can't tell: hold the overclock back to be safe
        MarkFile::Mark(m) => decide(&(*m).into(), boot_ms()),
    }
}

/// Voltages from which a raised curve point is an overclock. Below it, raised points are the
/// usual undervolt shape (a flat line that is lower than stock at the top).
const OC_FROM_MV: f32 = 850.0;

/// Positive offsets, a curve above stock where the card boosts, or a voltage boost could crash a
/// PC; lower power and undervolt curves are treated as safe.
pub fn risky(p: &GpuProfile) -> bool {
    p.core_mhz > 0 || p.mem_mhz > 0 || p.volt_pct.unwrap_or(0) > 0 || p.curve.as_ref().map(|c| c.iter().any(|x| x.0 >= OC_FROM_MV && x.1 > 0.0)).unwrap_or(false)
}

pub fn record(p: &GpuProfile, _ok: bool) {
    let m = Mark { id: p.id.clone(), risky: risky(p), at: crate::util::now_ms(), ..Default::default() };
    write(&path(), &m);
}

/// Deck is exiting. `session_end`: Windows is shutting down, restarting or signing out, so the
/// overclock ends cleanly with it. Otherwise Deck was closed by hand and the card (and CPU)
/// keep the overclock until the next restart, so a crash after this must still count.
pub fn on_exit(session_end: bool) {
    if let Some(m) = read(&cpu_path()) {
        write(&cpu_path(), &exit_mark(m, session_end));
    }
    if let Some(m) = read(&path()) {
        write(&path(), &exit_mark(m, session_end));
    }
}

/// The mark after Deck exits (separated for tests).
fn exit_mark(mut m: Mark, session_end: bool) -> Mark {
    if m.held {
        return m;
    }
    if session_end {
        m.clean = true;
        m.quit = false;
    } else if m.risky && !m.clean {
        m.quit = true;
    }
    m
}

/// Why the last hold happened, for the toast: true when Deck had been closed by hand while the
/// overclock was on (Deck can't tell whether the PC then crashed or shut down normally).
pub fn held_after_quit() -> bool {
    read(&path()).map(|m| m.quit).unwrap_or(false)
}

/// The same for the CPU turbo overclock.
pub fn cpu_held_after_quit() -> bool {
    read(&cpu_path()).map(|m| m.quit).unwrap_or(false)
}

fn boot_ms() -> u64 {
    crate::util::now_ms().saturating_sub(unsafe { windows::Win32::System::SystemInformation::GetTickCount64() })
}

/// At start: a profile applied earlier in this boot is still on the card, so a crash from now
/// on must count again (the last quit marked it clean).
pub fn rearm() {
    if let Some(mut m) = read(&cpu_path()) {
        if m.clean && m.at > boot_ms() {
            m.clean = false;
            write(&cpu_path(), &m);
        }
    }
    if let Some(mut m) = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str::<Mark>(&t).ok()) {
        if m.clean && !m.held && m.at > boot_ms() {
            m.clean = false;
            let _ = std::fs::write(path(), serde_json::to_string(&m).unwrap_or_default());
        }
    }
}

/// The profile Deck applied since the PC last started, if any.
pub fn applied_this_boot() -> Option<String> {
    let m: Mark = serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()?;
    (m.at > boot_ms()).then_some(m.id)
}

/// True when `p` must not be applied automatically now: the PC restarted without Deck
/// quitting while a risky profile was on, and `p` is risky too.
pub fn hold_back(p: &GpuProfile) -> bool {
    hold_back_at(&path(), p)
}

/// `hold_back` for the mark file `file`.
fn hold_back_at(file: &std::path::Path, p: &GpuProfile) -> bool {
    // only a risky profile is ever held; a safe one is fine to re-apply whatever the file says
    if !risky(p) {
        return false;
    }
    let mut m = match read_file(file) {
        MarkFile::Missing => return false,
        MarkFile::Damaged => return true, // a cut-off mark: this is the crash the guard is for
        MarkFile::Mark(m) => *m,
    };
    let hold = m.held || decide(&m.clone().into(), boot_ms());
    if hold && !m.held {
        m.held = true;
        write(file, &m);
    }
    hold
}

pub struct Last {
    pub risky: bool,
    pub at: u64,
    pub clean: bool,
}

impl From<Mark> for Last {
    fn from(m: Mark) -> Self {
        Last { risky: m.risky, at: m.at, clean: m.clean }
    }
}

/// The rule, separated for tests: unclean, risky, and the PC booted after it was applied.
pub fn decide(last: &Last, boot: u64) -> bool {
    !last.clean && last.risky && boot > last.at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_rule() {
        let l = Last { risky: true, at: 1000, clean: false };
        assert!(decide(&l, 2000), "restarted after a risky apply without a clean quit");
        assert!(!decide(&Last { clean: true, ..l }, 2000));
        assert!(!decide(&Last { risky: false, at: 1000, clean: false }, 2000));
        assert!(!decide(&Last { risky: true, at: 3000, clean: false }, 2000), "Deck crashed but the PC didn't restart");
        let oc = GpuProfile { core_mhz: 150, ..Default::default() };
        let low = GpuProfile { core_mhz: -493, power_pct: 41, ..Default::default() };
        assert!(risky(&oc) && !risky(&low));
        // a flattened undervolt: raised at low voltage, far below stock at the top
        let uv = GpuProfile { curve: Some(vec![(700.0, 101.0), (762.0, 62.0), (850.0, -200.0), (1000.0, -430.0)]), ..Default::default() };
        let curve_oc = GpuProfile { curve: Some(vec![(700.0, 0.0), (900.0, 60.0), (1000.0, 90.0)]), ..Default::default() };
        assert!(!risky(&uv) && risky(&curve_oc));
        // old guard files (no "held") still read
        let m: Mark = serde_json::from_str(r#"{"id":"a","risky":true,"at":5,"clean":false}"#).unwrap();
        assert!(!m.held && !m.quit);
    }

    #[test]
    fn closing_deck_keeps_the_guard_armed() {
        let on = Mark { id: "oc".into(), risky: true, at: 1000, ..Default::default() };
        // closed by hand: the card keeps the overclock, so a later crash must still hold it back
        let quit = exit_mark(on.clone(), false);
        assert!(!quit.clean && quit.quit);
        assert!(decide(&quit.into(), 2000), "a restart after closing Deck by hand must still hold the overclock");
        // Windows shutting down: the overclock ends with the session
        let end = exit_mark(on.clone(), true);
        assert!(end.clean && !end.quit);
        assert!(!decide(&end.into(), 2000));
        // a held overclock stays held either way
        let held = Mark { held: true, ..on.clone() };
        assert!(!exit_mark(held.clone(), true).clean && !exit_mark(held, false).clean);
        // a safe profile is never marked as quit
        assert!(!exit_mark(Mark { risky: false, ..on }, false).quit);
    }
}

#[cfg(test)]
mod damaged_file_tests {
    use super::*;

    fn file(name: &str, content: Option<&[u8]>) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("deck-guard-{name}-{}", crate::util::now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("gpu-applied.json");
        if let Some(c) = content {
            std::fs::write(&f, c).unwrap();
        }
        f
    }

    fn oc() -> GpuProfile {
        GpuProfile { id: "oc".into(), core_mhz: 150, ..Default::default() }
    }

    /// A crash while the mark was being written leaves it empty or cut off. That is exactly the
    /// case the guard exists for, so an unreadable mark holds a risky profile back.
    #[test]
    fn an_unreadable_mark_holds_the_overclock() {
        for (name, c) in [("empty", &b""[..]), ("cut", &br#"{"id":"oc","risky":tr"#[..]), ("junk", &b"\0\0\0"[..])] {
            let f = file(name, Some(c));
            assert!(hold_back_at(&f, &oc()), "{name}: GPU overclock must be held");
            assert!(cpu_hold_at(&f), "{name}: CPU overclock must be held");
            let _ = std::fs::remove_dir_all(f.parent().unwrap());
        }
    }

    #[test]
    fn a_safe_profile_is_never_held() {
        let f = file("safe", Some(b""));
        let low = GpuProfile { id: "low".into(), power_pct: 41, ..Default::default() };
        assert!(!hold_back_at(&f, &low));
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn no_mark_at_all_holds_nothing() {
        let f = file("none", None);
        assert!(!hold_back_at(&f, &oc()));
        assert!(!cpu_hold_at(&f));
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }
}
