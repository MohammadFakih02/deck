//! CPU profiles: each one is a Windows power plan (switching is instant and keeps working
//! with Deck closed) plus extras Deck applies itself: RAPL power limits, a minimum-clock lock,
//! a turbo-ratio overclock and turning Windows Game Mode off so caps hold in games.

use super::plans::{self, BOOST_MODE, EPP, FREQ_MAX, MIN_CORES, STATE_MAX, STATE_MIN};
use super::{CpuPower, CpuProfile};
use crate::app::App;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::GUID;

const GAMEBAR: &str = "Software\\Microsoft\\GameBar";
const AUTO_GAME_MODE: &str = "AutoGameModeEnabled";

/// More than one kind of core (Intel P + E cores): two clock caps instead of one.
pub fn hybrid() -> bool {
    use std::sync::OnceLock;
    static H: OnceLock<bool> = OnceLock::new();
    *H.get_or_init(|| {
        use windows_sys::Win32::System::SystemInformation::{GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX};
        unsafe {
            let mut len = 0u32;
            GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut len);
            if len == 0 {
                return false;
            }
            let mut buf = vec![0u8; len as usize];
            if GetLogicalProcessorInformationEx(RelationProcessorCore, buf.as_mut_ptr() as *mut _, &mut len) == 0 {
                return false;
            }
            let mut off = 0usize;
            let mut classes = std::collections::HashSet::new();
            while off + 8 <= len as usize {
                let p = buf.as_ptr().add(off) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
                let size = (*p).Size as usize;
                classes.insert((*p).Anonymous.Processor.EfficiencyClass);
                if size == 0 {
                    break;
                }
                off += size;
            }
            classes.len() > 1
        }
    })
}

/// The plan's current values as a profile (for adopting existing plans and for showing edits
/// made outside Deck).
pub fn read_plan(g: &GUID, into: &mut CpuProfile) {
    let ac = |s: &GUID| plans::read(g, s).map(|v| v.0);
    let cap = |v: Option<u32>| v.filter(|m| *m > 0);
    // class 0 = E-cores on hybrid CPUs (all cores otherwise), class 1 = P-cores
    let (c0, c1) = (cap(ac(&FREQ_MAX[0])), cap(ac(&FREQ_MAX[1])));
    if hybrid() {
        into.cap_e = c0;
        into.cap_p = c1;
    } else {
        into.cap_p = c0;
        into.cap_e = c0;
    }
    if let Some(v) = ac(&STATE_MAX[0]) {
        into.max_state = v.clamp(1, 100);
    }
    if let Some(v) = ac(&STATE_MIN[0]) {
        into.min_state = v.min(100);
    }
    if let Some(v) = ac(&BOOST_MODE) {
        into.turbo = v.min(6);
    }
    if let Some(v) = ac(&MIN_CORES[0]) {
        into.min_cores = v.min(100);
    }
    into.epp = ac(&EPP[0]).map(|v| v.min(100));
}

/// Writes a profile's plan values into its plan: plugged-in (AC) always; the battery (DC) value
/// only for fields that differ from `prev` (what the user changed), so a laptop keeps its own
/// battery settings. `prev` None = this profile is new: nothing on battery changes.
pub fn write_plan(p: &CpuProfile, prev: Option<&CpuProfile>) -> Result<GUID, String> {
    let g = plans::parse(&p.plan).ok_or("This profile has no Windows power plan.")?;
    let caps = |q: &CpuProfile| if hybrid() { (q.cap_e, q.cap_p) } else { (q.cap_p.or(q.cap_e), q.cap_p.or(q.cap_e)) };
    let (c0, c1) = caps(p);
    let old = prev.map(|q| (caps(q), q.max_state, q.min_state, q.turbo, q.min_cores, q.epp));
    let put = |s: &GUID, v: u32, changed: bool| if changed { plans::write(&g, s, v) } else { plans::write_ac(&g, s, v) };
    let ch = |f: &dyn Fn(&((Option<u32>, Option<u32>), u32, u32, u32, u32, Option<u32>)) -> bool| old.as_ref().map(f).unwrap_or(false);
    put(&FREQ_MAX[0], c0.unwrap_or(0), ch(&|o| o.0 .0 != c0))?;
    put(&FREQ_MAX[1], c1.unwrap_or(0), ch(&|o| o.0 .1 != c1))?;
    for s in STATE_MAX {
        put(&s, p.max_state.clamp(1, 100), ch(&|o| o.1 != p.max_state))?;
    }
    for s in STATE_MIN {
        put(&s, p.min_state.min(p.max_state), ch(&|o| o.2 != p.min_state))?;
    }
    put(&BOOST_MODE, p.turbo.min(6), ch(&|o| o.3 != p.turbo))?;
    for s in MIN_CORES {
        put(&s, p.min_cores.min(100), ch(&|o| o.4 != p.min_cores))?;
    }
    if let Some(e) = p.epp {
        for s in EPP {
            put(&s, e.min(100), ch(&|o| o.5 != p.epp))?;
        }
    }
    Ok(g)
}

/// A profile has a clock cap that Game Mode would undo.
fn capped(p: &CpuProfile) -> bool {
    p.cap_p.is_some() || p.cap_e.is_some() || p.max_state < 100 || p.turbo == 0
}

/// Turns Windows Game Mode off while `want_off`, remembering and later restoring the user's
/// own setting (`game_mode_orig`: -1 = untouched, 2 = the value did not exist).
pub fn game_mode(app: &App, want_off: bool) {
    let orig = app.settings.lock().game_mode_orig;
    if want_off {
        if orig == -1 {
            let cur = crate::display::read_dword(GAMEBAR, AUTO_GAME_MODE).map(|v| v as i32).unwrap_or(2);
            app.settings.lock().game_mode_orig = cur;
            app.save_settings();
        }
        let _ = crate::display::write_dword(GAMEBAR, AUTO_GAME_MODE, 0);
    } else if orig != -1 {
        if orig == 2 {
            delete_value(GAMEBAR, AUTO_GAME_MODE);
        } else {
            let _ = crate::display::write_dword(GAMEBAR, AUTO_GAME_MODE, orig as u32);
        }
        app.settings.lock().game_mode_orig = -1;
        app.save_settings();
    }
}

/// Puts Game Mode back to a remembered value (for `--release-fans` after a forced quit).
pub fn restore_game_mode(orig: i32) {
    if orig == 2 {
        delete_value(GAMEBAR, AUTO_GAME_MODE);
    } else if orig >= 0 {
        let _ = crate::display::write_dword(GAMEBAR, AUTO_GAME_MODE, orig as u32);
    }
}

fn delete_value(key: &str, value: &str) {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(value));
    }
}

/// The hardware engine's key/value settings, once it is running (admin only).
fn hw(app: &App) -> Option<Arc<crate::hw::Shared>> {
    app.hw.lock().clone()
}

/// What the profile's power limits mean for the hardware engine's power-mode thread.
fn power_mode(p: &Option<CpuPower>) -> (i32, i32, i32) {
    use crate::hw::powermode::*;
    match p {
        None => (BALANCED, 0, 0),
        Some(pw) => match pw.mode.as_str() {
            "saver" => (SAVER, 0, 0),
            "performance" => (PERFORMANCE, 0, 0),
            "custom" if pw.pl1 >= 5 => (CUSTOM, pw.pl1 as i32, pw.pl2.max(pw.pl1) as i32),
            _ => (BALANCED, 0, 0),
        },
    }
}

/// Applies the extras (everything that is not a plan value). Called after the plan switched,
/// by Deck or by something else. Returns notes for the toast.
pub fn apply_extras(app: &App, p: Option<&CpuProfile>) -> Vec<String> {
    let mut notes = vec![];
    game_mode(app, p.map(|p| p.hold_in_games && capped(p)).unwrap_or(false));
    let Some(sh) = hw(app) else {
        if p.map(|p| p.power.is_some() || p.min_lock.is_some() || p.oc.is_some()).unwrap_or(false) {
            notes.push("power limits, clock lock and overclock need Deck to run as admin".into());
        }
        return notes;
    };
    let s = &sh.s;
    // power limits: the hardware engine's power thread applies and re-asserts them
    let (mode, pl1, pl2) = power_mode(&p.and_then(|p| p.power.clone()));
    s.set_int("PowerMode", mode);
    s.set_int("PowerPl1", pl1);
    s.set_int("PowerPl2", pl2);
    // minimum clock: the lock is written into the active plan, and the plan just changed
    let lim = crate::hw::cpupower::limits();
    let top = p.and_then(|p| p.cap_p.or(p.cap_e)).map(|m| m as i32).unwrap_or(lim.turbo.max(lim.base));
    match p.and_then(|p| p.min_lock) {
        Some(m) => {
            // an older lock (in the plan it was written to) goes first, then this plan's own
            // values are the ones remembered
            if s.int("CpuMinSet", 0) == 1 {
                crate::hw::cpupower::apply_min(None, top, s);
            }
            crate::hw::cpupower::apply_min(Some((m as i32).min(top)), top, s);
            s.set_bool("CpuMinOn", true);
        }
        None => {
            if s.int("CpuMinSet", 0) == 1 {
                crate::hw::cpupower::apply_min(None, top, s);
            }
            s.set_bool("CpuMinOn", false);
        }
    }
    // turbo-ratio overclock (lost at restart, so re-applied whenever the profile is)
    let saved = crate::hw::cpupower::saved_oc(s);
    let want = p.and_then(|p| p.oc).map(|m| m as i32);
    if want.is_some() || saved.is_some() {
        let (keep, msg) = crate::hw::cpupower::apply_oc(want, saved);
        crate::hw::cpupower::store_oc(s, keep);
        crate::gpu::guard::cpu_record(keep.is_some());
        if want.is_some() && keep.is_none() {
            notes.push(msg);
        }
    }
    s.save();
    notes
}

/// The profile's plan was deleted (powercfg, another tool, an old plan cleaned up): a new one is
/// made from Balanced and the profile's own values are written into it, so the profile keeps working.
fn ensure_plan(app: &App, p: &CpuProfile) -> Result<CpuProfile, String> {
    if plans::parse(&p.plan).is_some_and(|g| plans::list().contains(&g)) {
        return Ok(p.clone());
    }
    let g = plans::duplicate(&plans::BALANCED, &format!("Deck · {}", p.name))?;
    let mut q = p.clone();
    q.plan = plans::guid_str(&g);
    if let Some(x) = app.settings.lock().cpu_profiles.iter_mut().find(|x| x.id == p.id) {
        x.plan = q.plan.clone();
    }
    app.save_settings();
    Ok(q)
}

/// Switches to a CPU profile: its plan becomes active, then the extras are applied.
pub fn apply(app: &App, p: &CpuProfile) -> Result<String, String> {
    let _one = SWITCH.lock();
    // undo an earlier minimum-clock lock (in the plan it was written to) first, so the values it
    // remembered can't overwrite this profile's own
    if let Some(sh) = hw(app) {
        if sh.s.int("CpuMinSet", 0) == 1 {
            let lim = crate::hw::cpupower::limits();
            crate::hw::cpupower::apply_min(None, lim.turbo.max(lim.base), &sh.s);
        }
    }
    let p = &ensure_plan(app, p)?;
    // the profile is the source of truth: put its values back into the plan
    let g = write_plan(p, Some(p))?;
    activate(&g)?;
    let notes = apply_extras(app, Some(p));
    let sum = super::cpu_summary(p);
    Ok(if notes.is_empty() { sum } else { format!("{sum} ({})", notes.join("; ")) })
}

/// Saves edits to a profile: writes the plan values and re-activates the plan when it is the
/// active one (Windows only picks changed values up then).
pub fn save(app: &App, p: &CpuProfile, prev: Option<&CpuProfile>) -> Result<(), String> {
    let _one = SWITCH.lock();
    let p = &ensure_plan(app, p)?;
    let g = write_plan(p, prev)?;
    plans::rename(&g, &format!("Deck · {}", p.name)).ok();
    if plans::active() == Some(g) {
        activate(&g)?;
        apply_extras(app, Some(p));
    }
    Ok(())
}

/// Makes `g` active (switching away and back when it already is). Callers hold `SWITCH`, so the
/// watcher never sees the plan in between.
fn activate(g: &GUID) -> Result<(), String> {
    let r = if plans::active() == Some(*g) { plans::reactivate(g) } else { plans::set_active(g) };
    *WATCH.lock() = Some(*g);
    r
}

/// New profile: a copy of the active plan, renamed.
pub fn create(name: &str) -> Result<CpuProfile, String> {
    let src = plans::active().unwrap_or(plans::BALANCED);
    let g = plans::duplicate(&src, &format!("Deck · {name}"))?;
    let mut p = CpuProfile { id: super::new_id(), name: name.into(), plan: plans::guid_str(&g), ..Default::default() };
    read_plan(&g, &mut p);
    Ok(p)
}

/// Turns existing plans into profiles (same GUIDs, so old shortcuts keep working).
pub fn adopt(g: &GUID) -> CpuProfile {
    let full = plans::name(g);
    let name = full.trim_start_matches("Deck · ").trim_start_matches("PT-").trim().to_string();
    let lower = name.to_lowercase();
    let icon = ["ultra", "eco", "mid", "full"].iter().find(|k| lower.contains(*k)).map(|k| format!("cpu-{k}.ico")).unwrap_or_else(|| "cpu-mid.ico".into());
    let mut p = CpuProfile { id: super::new_id(), name: if name.is_empty() { full } else { name }, icon, plan: plans::guid_str(g), ..Default::default() };
    read_plan(g, &mut p);
    p.hold_in_games = capped(&p);
    p
}

/// Every Windows power plan, for "Use my existing plans".
pub fn plan_list(app: &App) -> Vec<Value> {
    let act = plans::active();
    let used: Vec<String> = app.settings.lock().cpu_profiles.iter().map(|p| p.plan.to_lowercase()).collect();
    plans::list()
        .iter()
        .map(|g| {
            let mut p = CpuProfile::default();
            read_plan(g, &mut p);
            let id = plans::guid_str(g);
            json!({ "guid": id, "name": plans::name(g), "active": Some(*g) == act, "used": used.contains(&id), "summary": super::cpu_summary(&p) })
        })
        .collect()
}

/// Deletes a profile; with `delete_plan` its Windows plan too (switching away first).
pub fn remove(app: &App, id: &str, delete_plan: bool) -> Result<(), String> {
    let _one = SWITCH.lock();
    let p = app.settings.lock().cpu_profiles.iter().find(|p| p.id == id).cloned().ok_or("No such profile.")?;
    if delete_plan {
        if let Some(g) = plans::parse(&p.plan) {
            if plans::active() == Some(g) {
                let other = plans::list().into_iter().find(|x| *x != g).unwrap_or(plans::BALANCED);
                plans::set_active(&other)?;
            }
            plans::delete(&g)?;
        }
    }
    let mut s = app.settings.lock();
    s.cpu_profiles.retain(|x| x.id != id);
    if s.active_cpu == id {
        s.active_cpu.clear();
    }
    Ok(())
}

/// The plan the watcher saw last.
static WATCH: Mutex<Option<GUID>> = Mutex::new(None);
/// Held for a whole switch, save or delete (plan, then extras), so two switches can't interleave
/// and the watcher can't act on a plan Deck passes through on the way.
static SWITCH: Mutex<()> = Mutex::new(());

/// In-game clock check: the average real clock while a full-screen game is in front.
#[derive(Default, Clone)]
struct GameClock {
    on: bool,
    n: u32,
    sum: u64,
    max: u32,
    cap: Option<u32>,
    profile: String,
    game: String,
    ended: Option<Instant>,
}

static CLOCK: Mutex<Option<GameClock>> = Mutex::new(None);

pub fn clock_check() -> Value {
    match CLOCK.lock().clone() {
        Some(c) if c.n > 0 => json!({
            "on": c.on, "avg": c.sum / c.n as u64, "max": c.max, "cap": c.cap, "profile": c.profile, "game": c.game,
            "samples": c.n, "ago_s": c.ended.map(|t| t.elapsed().as_secs()),
        }),
        _ => Value::Null,
    }
}

/// Watches the active plan (2 s) and samples the clock in games.
pub fn start_watch(app: Arc<App>) {
    std::thread::Builder::new()
        .name("cpu-profiles".into())
        .spawn(move || {
            // the power engine (admin) starts in the background; give it a moment so the
            // first apply can set the power limits
            let t0 = Instant::now();
            while crate::elevation::is_admin() && app.hw.lock().is_none() && t0.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(500));
            }
            loop {
                tick(&app);
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .ok();
}

fn tick(app: &App) {
    if let Some(_one) = SWITCH.try_lock() {
        follow_plan(app);
    }
    clock_tick(app);
}

/// Picks up a plan change made outside Deck (Windows settings, another tool, a shortcut).
fn follow_plan(app: &App) {
    let cur = plans::active();
    let seen = *WATCH.lock();
    if cur.is_some() && cur != seen {
        *WATCH.lock() = cur;
        let g = cur.unwrap();
        let (prof, active) = {
            let s = app.settings.lock();
            (s.cpu_profiles.iter().find(|p| plans::parse(&p.plan) == Some(g)).cloned(), s.active_cpu.clone())
        };
        match prof {
            Some(p) => {
                // at start after an unexpected restart with the turbo overclock on: everything
                // but the overclock (it comes back when the profile is switched to by hand)
                if seen.is_none() && p.oc.is_some() && crate::gpu::guard::cpu_hold() {
                    let mut q = p.clone();
                    q.oc = None;
                    apply_extras(app, Some(&q));
                    app.toast(crate::app::Toast::simple("cpu-held", "CPU overclock held back", &format!(
                        "{} while {} raised the turbo clock. Deck started it without the overclock; switch to the profile to turn it on again.",
                        if crate::gpu::guard::cpu_held_after_quit() { "Deck was closed and the PC then restarted" } else { "The PC restarted unexpectedly" },
                        p.name
                    )));
                } else {
                    apply_extras(app, Some(&p));
                }
                if p.id != active {
                    app.settings.lock().active_cpu = p.id.clone();
                    app.save_settings();
                }
                app.changed("profiles");
            }
            None => {
                let had = app.settings.lock().cpu_profiles.iter().any(|p| p.power.is_some() || p.min_lock.is_some() || p.oc.is_some() || p.hold_in_games);
                if had {
                    apply_extras(app, None);
                }
                if !active.is_empty() {
                    app.settings.lock().active_cpu.clear();
                    app.save_settings();
                }
                app.changed("profiles");
            }
        }
    }
}

/// In-game clock check.
fn clock_tick(app: &App) {
    let in_game = crate::hw::game::foreground_game();
    let mhz = app.hw.lock().as_ref().and_then(|sh| sh.live.lock().ok().and_then(|l| l.cpu_mhz));
    let mut c = CLOCK.lock();
    match (in_game, mhz) {
        (Some(pid), Some(m)) => {
            let (cap, profile) = {
                let s = app.settings.lock();
                let p = s.cpu_profiles.iter().find(|p| p.id == s.active_cpu);
                (p.and_then(|p| p.cap_p.or(p.cap_e)), p.map(|p| p.name.clone()).unwrap_or_default())
            };
            let game = crate::util::process_path(pid).map(|p| crate::util::exe_stem(&p)).unwrap_or_default();
            let fresh = c.as_ref().map(|x| !x.on || x.game != game || x.profile != profile).unwrap_or(true);
            if fresh {
                *c = Some(GameClock { on: true, cap, profile, game, ..Default::default() });
            }
            let g = c.as_mut().unwrap();
            g.n += 1;
            g.sum += m as u64;
            g.max = g.max.max(m);
        }
        _ => {
            if let Some(g) = c.as_mut() {
                if g.on {
                    g.on = false;
                    g.ended = Some(Instant::now());
                }
            }
        }
    }
}

/// `--cpu-probe`: every plan with its caps and turbo, and the active one.
pub fn probe() -> String {
    let act = plans::active();
    let mut o = format!("hybrid: {}\nlimits: {:?}\n", hybrid(), crate::hw::cpupower::limits());
    for g in plans::list() {
        let mut p = CpuProfile::default();
        read_plan(&g, &mut p);
        o += &format!("{}{} {}  {}\n", if Some(g) == act { "* " } else { "  " }, plans::guid_str(&g), plans::name(&g), super::cpu_summary(&p));
    }
    o += &format!("Game Mode (AutoGameModeEnabled): {:?}\n", crate::display::read_dword(GAMEBAR, AUTO_GAME_MODE));
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_modes() {
        use crate::hw::powermode::*;
        assert_eq!(power_mode(&None), (BALANCED, 0, 0));
        assert_eq!(power_mode(&Some(CpuPower { mode: "custom".into(), pl1: 35, pl2: 20 })), (CUSTOM, 35, 35));
        assert_eq!(power_mode(&Some(CpuPower { mode: "custom".into(), pl1: 2, pl2: 20 })), (BALANCED, 0, 0), "under 5 W is not a limit");
        assert_eq!(power_mode(&Some(CpuPower { mode: "saver".into(), pl1: 0, pl2: 0 })), (SAVER, 0, 0));
    }

    #[test]
    fn cap_detection() {
        assert!(!capped(&CpuProfile::default()));
        assert!(capped(&CpuProfile { cap_p: Some(1500), ..Default::default() }));
        assert!(capped(&CpuProfile { turbo: 0, ..Default::default() }));
    }

    /// Reads this PC's plans (changes nothing).
    #[test]
    #[ignore]
    fn cpu_probe() {
        println!("{}", probe());
    }
}
