//! Keep awake, "do X when Y" (shutdown/sleep/lock when a download finishes,
//! an app closes, at a time ...), timers, a stopwatch and break reminders.

use crate::app::{App, Toast};
use crate::util::{now_ms, now_s};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::System::Power::{SetSuspendState, SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
use windows::Win32::System::Shutdown::{ExitWindowsEx, LockWorkStation, EWX_LOGOFF, SHUTDOWN_REASON};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND};
use windows::Win32::Foundation::{LPARAM, WPARAM};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Awake {
    /// "off" | "on" | "until" | "app" (while an app runs) | "downloads"
    pub mode: String,
    pub until: u64,
    pub app: String,
    /// Keep the screen on too (not just the PC).
    pub display: bool,
    /// "downloads": a download was seen since it was turned on
    #[serde(skip_deserializing)]
    pub dl_seen: bool,
    /// "downloads": nothing downloading since (unix s)
    #[serde(skip)]
    pub dl_quiet_since: Option<u64>,
}

impl Default for Awake {
    fn default() -> Self {
        Self { mode: "off".into(), until: 0, app: String::new(), display: true, dl_seen: false, dl_quiet_since: None }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct Plan {
    /// "shutdown" | "restart" | "sleep" | "hibernate" | "lock" | "signout" | "screen_off" | "notify"
    pub action: String,
    /// "time" | "app_closes" | "downloads" | "cpu_idle" | "net_idle" | "media_stops" | "idle"
    pub cond: String,
    /// unix seconds for "time"
    pub at: u64,
    /// exe name for "app_closes"
    pub app: String,
    /// quiet minutes for idle-type conditions
    pub minutes: u32,
    pub created: u64,
    /// "waiting" | "countdown"
    pub state: String,
    pub countdown_until: u64,
    /// When the condition started being true (idle-type conditions).
    #[serde(skip)]
    pub quiet_since: Option<u64>,
    /// Human description of progress ("Network quiet for 2 of 5 min").
    pub progress: String,
    /// "downloads" / "media_stops": a download (or playback) was seen since the plan was set.
    /// Until then nothing can "finish", so the plan keeps waiting.
    #[serde(skip_deserializing)]
    pub seen: bool,
}

/// What a plan should do this check.
#[derive(Debug, PartialEq)]
pub enum Step {
    Wait,
    Fire,
    /// a set time passed long ago (the PC was asleep or off): don't act on it now
    Missed,
}

/// A set time this late is not acted on (a 23:00 shutdown must not hit the PC the next morning).
const MISSED_AFTER_S: u64 = 10 * 60;

/// One check of a plan's condition. `met_now`: the condition holds right now. `active`: for
/// "downloads" and "media_stops", whether something is downloading / playing.
pub fn step(plan: &mut Plan, now: u64, met_now: bool, instant: bool, active: Option<bool>) -> Step {
    if plan.cond == "time" && now > plan.at + MISSED_AFTER_S {
        return Step::Missed;
    }
    if let Some(a) = active {
        if a {
            plan.seen = true;
        }
        if !plan.seen {
            plan.quiet_since = None;
            plan.progress = if plan.cond == "downloads" { "Waiting for a download to start".into() } else { "Waiting for something to play".into() };
            return Step::Wait;
        }
    }
    if instant {
        return if met_now { Step::Fire } else { Step::Wait };
    }
    if met_now {
        let since = *plan.quiet_since.get_or_insert(now);
        let need = if plan.cond == "media_stops" { 60 } else { plan.minutes.max(1) as u64 * 60 };
        let what = match plan.cond.as_str() {
            "downloads" => "Nothing downloading",
            "media_stops" => "Nothing playing",
            "cpu_idle" => "PC idle",
            _ => "Network quiet",
        };
        plan.progress = format!("{what} for {} of {}", fmt_dur(now - since), fmt_dur(need));
        if now - since >= need { Step::Fire } else { Step::Wait }
    } else {
        plan.quiet_since = None;
        plan.progress = match plan.cond.as_str() {
            "downloads" => "Downloading".into(),
            "media_stops" => "Playing".into(),
            _ => String::new(),
        };
        Step::Wait
    }
}

/// Something is downloading: a browser's unfinished file, a Steam update, or steady traffic.
fn downloading(app: &App) -> bool {
    partial_download() || steam_downloading() || app.sys.down_now() > 200_000.0
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Timer {
    pub id: u64,
    pub label: String,
    pub secs: u64,
    pub end_ms: u64,
    /// Remaining ms while paused.
    pub paused_left: Option<u64>,
    pub done: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Stopwatch {
    pub running: bool,
    pub start_ms: u64,
    pub acc_ms: u64,
    pub laps: Vec<u64>,
}

#[derive(Default)]
struct Breaks {
    active_since: Option<u64>,
    snooze_until: u64,
}

pub struct Power {
    pub awake: Mutex<Awake>,
    pub plan: Mutex<Option<Plan>>,
    pub timers: Mutex<Vec<Timer>>,
    pub stopwatch: Mutex<Stopwatch>,
    breaks: Mutex<Breaks>,
    next_timer: Mutex<u64>,
}

pub const COUNTDOWN_S: u64 = 60;

pub fn action_label(a: &str) -> &'static str {
    match a {
        "shutdown" => "Shut down",
        "restart" => "Restart",
        "sleep" => "Sleep",
        "hibernate" => "Hibernate",
        "lock" => "Lock",
        "signout" => "Sign out",
        "screen_off" => "Turn the screen off",
        _ => "Notify me",
    }
}

/// Seconds since the last keyboard/mouse input.
pub fn idle_secs() -> u64 {
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if GetLastInputInfo(&mut li).as_bool() {
            let now = GetTickCount64();
            return now.saturating_sub(li.dwTime as u64) / 1000;
        }
    }
    0
}

pub fn run_action(action: &str) -> Result<(), String> {
    match action {
        "shutdown" => crate::util::run_hidden("shutdown.exe", &["/s", "/t", "0"]).map(|_| ()).map_err(|e| e.to_string()),
        "restart" => crate::util::run_hidden("shutdown.exe", &["/r", "/t", "0"]).map(|_| ()).map_err(|e| e.to_string()),
        "sleep" => unsafe {
            if SetSuspendState(false, false, false) { Ok(()) } else { Err("Windows refused to sleep".into()) }
        },
        "hibernate" => unsafe {
            if SetSuspendState(true, false, false) { Ok(()) } else { Err("Hibernate is turned off on this PC".into()) }
        },
        "lock" => unsafe { LockWorkStation().map_err(|e| e.to_string()) },
        "signout" => unsafe { ExitWindowsEx(EWX_LOGOFF, SHUTDOWN_REASON(0)).map_err(|e| e.to_string()) },
        "screen_off" => unsafe {
            PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(2)).map_err(|e| e.to_string())
        },
        "notify" => Ok(()),
        _ => Err("unknown action".into()),
    }
}

/// Windows' own "sleep after" and "turn the screen off after" for the active plan and the
/// current power source, in seconds (0 = never). Read only.
pub fn windows_timeouts() -> Option<(u32, u32)> {
    use windows::core::GUID;
    const SUB_SLEEP: GUID = GUID::from_u128(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
    const STANDBY_IDLE: GUID = GUID::from_u128(0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da);
    const SUB_VIDEO: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
    const VIDEO_IDLE: GUID = GUID::from_u128(0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);
    let g = crate::profiles::plans::active()?;
    let on_battery = unsafe {
        let mut st = windows::Win32::System::Power::SYSTEM_POWER_STATUS::default();
        windows::Win32::System::Power::GetSystemPowerStatus(&mut st).is_ok() && st.ACLineStatus == 0
    };
    let pick = |v: (u32, u32)| if on_battery { v.1 } else { v.0 };
    let sleep = pick(crate::profiles::plans::read_in(&g, &SUB_SLEEP, &STANDBY_IDLE)?);
    let screen = pick(crate::profiles::plans::read_in(&g, &SUB_VIDEO, &VIDEO_IDLE)?);
    Some((sleep, screen))
}

/// Files browsers use for unfinished downloads.
fn partial_download() -> bool {
    let exts = ["crdownload", "part", "partial", "download", "opdownload", "!ut", "tmp"];
    let Ok(rd) = std::fs::read_dir(crate::util::downloads()) else { return false };
    rd.flatten().any(|e| {
        e.path().extension().map(|x| exts.contains(&x.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
    })
}

/// Steam keeps in-progress downloads in steamapps\downloading.
fn steam_downloading() -> bool {
    crate::games::steam_libraries().iter().any(|lib| {
        std::fs::read_dir(lib.join("steamapps").join("downloading"))
            .map(|rd| rd.flatten().any(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)))
            .unwrap_or(false)
    })
}

impl Power {
    pub fn new() -> Self {
        Self {
            awake: Mutex::new(Awake::default()),
            plan: Mutex::new(None),
            timers: Mutex::new(Vec::new()),
            stopwatch: Mutex::new(Stopwatch::default()),
            breaks: Mutex::new(Breaks::default()),
            next_timer: Mutex::new(1),
        }
    }

    pub fn state(&self) -> Value {
        json!({
            "awake": *self.awake.lock(),
            "awake_active": self.awake_on_cached(),
            "plan": *self.plan.lock(),
            "timers": *self.timers.lock(),
            "stopwatch": *self.stopwatch.lock(),
            "now": now_ms(),
            "idle": idle_secs(),
            "win_timeouts": windows_timeouts().map(|(s, d)| json!({ "sleep": s, "screen": d })),
        })
    }

    fn awake_on_cached(&self) -> bool {
        let a = self.awake.lock();
        match a.mode.as_str() {
            "on" | "app" | "downloads" => true,
            "until" => now_s() < a.until,
            _ => false,
        }
    }

    // ------------------------------------------------------------- timers

    pub fn add_timer(&self, secs: u64, label: &str) -> u64 {
        let mut n = self.next_timer.lock();
        let id = *n;
        *n += 1;
        self.timers.lock().push(Timer {
            id,
            label: if label.trim().is_empty() { fmt_dur(secs) } else { label.trim().to_string() },
            secs,
            end_ms: now_ms() + secs * 1000,
            paused_left: None,
            done: false,
        });
        id
    }

    pub fn timer_cmd(&self, id: u64, cmd: &str) {
        let mut t = self.timers.lock();
        match cmd {
            "remove" => t.retain(|x| x.id != id),
            "pause" => {
                if let Some(x) = t.iter_mut().find(|x| x.id == id && x.paused_left.is_none() && !x.done) {
                    x.paused_left = Some(x.end_ms.saturating_sub(now_ms()));
                }
            }
            "resume" => {
                if let Some(x) = t.iter_mut().find(|x| x.id == id) {
                    if let Some(left) = x.paused_left.take() {
                        x.end_ms = now_ms() + left;
                    }
                }
            }
            "restart" => {
                if let Some(x) = t.iter_mut().find(|x| x.id == id) {
                    x.end_ms = now_ms() + x.secs * 1000;
                    x.paused_left = None;
                    x.done = false;
                }
            }
            "snooze" => {
                if let Some(x) = t.iter_mut().find(|x| x.id == id) {
                    x.end_ms = now_ms() + 5 * 60 * 1000;
                    x.done = false;
                }
            }
            _ => {}
        }
    }

    pub fn stopwatch_cmd(&self, cmd: &str) {
        let mut s = self.stopwatch.lock();
        let now = now_ms();
        match cmd {
            "start" if !s.running => {
                s.running = true;
                s.start_ms = now;
            }
            "stop" if s.running => {
                s.acc_ms += now - s.start_ms;
                s.running = false;
            }
            "lap" => {
                let total = s.acc_ms + if s.running { now - s.start_ms } else { 0 };
                s.laps.push(total);
            }
            "reset" => *s = Stopwatch::default(),
            _ => {}
        }
    }
}

pub fn fmt_dur(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (h, m, s) {
        (0, 0, s) => format!("{s} s"),
        (0, m, 0) => format!("{m} min"),
        (0, m, s) => format!("{m} min {s} s"),
        (h, 0, _) => format!("{h} h"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

fn plan_file() -> std::path::PathBuf {
    crate::settings::Settings::dir().join("plan.json")
}

/// A plan read back after Deck restarted: it waits again from the start (a countdown that was
/// running gets a fresh 60-second warning; a download or playback has to be seen again).
pub fn restored(mut p: Plan) -> Plan {
    p.state = "waiting".into();
    p.countdown_until = 0;
    p.quiet_since = None;
    p.seen = false;
    p.progress.clear();
    p
}

/// What identifies a plan, to save it only when it changes.
fn plan_key(p: &Option<Plan>) -> Option<(String, String, u64, String, u32, u64)> {
    p.as_ref().map(|p| (p.action.clone(), p.cond.clone(), p.at, p.app.clone(), p.minutes, p.created))
}

/// The background loop: keep-awake flags, plan conditions, timers, breaks.
/// Runs on its own thread for the life of the app (the keep-awake flag
/// belongs to this thread).
pub fn start(app: Arc<App>) {
    std::thread::Builder::new()
        .name("power".into())
        .spawn(move || {
            let mut last_flags = 0u32;
            let mut tick = 0u64;
            // a plan set before Deck restarted (closed, updated, crashed) carries on
            if let Some(p) = std::fs::read_to_string(plan_file()).ok().and_then(|t| serde_json::from_str::<Plan>(&t).ok()) {
                let mut cur = app.power.plan.lock();
                if cur.is_none() {
                    *cur = Some(restored(p));
                }
                drop(cur);
                app.changed("power");
            }
            let mut saved = plan_key(&app.power.plan.lock());
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                tick += 1;
                let p = &app.power;

                // ---- keep awake
                let (active, display) = {
                    let mut a = p.awake.lock();
                    let active = match a.mode.as_str() {
                        "on" => true,
                        "until" => {
                            if now_s() >= a.until {
                                a.mode = "off".into();
                                app.changed("power");
                                false
                            } else {
                                true
                            }
                        }
                        "app" => {
                            if tick % 5 == 0 && !a.app.is_empty() && !app.sys.running(&a.app) {
                                a.mode = "off".into();
                                app.changed("power");
                                false
                            } else {
                                true
                            }
                        }
                        "downloads" => {
                            // Checked with the same rules as the plan below, every 5 s.
                            true
                        }
                        _ => false,
                    };
                    (active, a.display)
                };
                let flags = if active {
                    (ES_CONTINUOUS | ES_SYSTEM_REQUIRED | if display { ES_DISPLAY_REQUIRED } else { Default::default() }).0
                } else {
                    ES_CONTINUOUS.0
                };
                if flags != last_flags {
                    unsafe {
                        SetThreadExecutionState(windows::Win32::System::Power::EXECUTION_STATE(flags));
                    }
                    last_flags = flags;
                }

                // ---- timers
                let now = now_ms();
                let mut fired = Vec::new();
                for t in p.timers.lock().iter_mut() {
                    if !t.done && t.paused_left.is_none() && now >= t.end_ms {
                        t.done = true;
                        fired.push(t.clone());
                    }
                }
                for t in fired {
                    app.toast(Toast {
                        id: format!("timer:{}", t.id),
                        kind: "timer".into(),
                        title: format!("Timer done: {}", t.label),
                        body: format!("{} is up.", fmt_dur(t.secs)),
                        actions: vec![("snooze".into(), "5 more min".into()), ("dismiss".into(), "OK".into())],
                        sticky: true,
                        sound: true,
                    });
                    app.changed("power");
                }

                // ---- plan
                if tick % 3 == 0 {
                    plan_tick(&app);
                }
                // keep plan.json in step with the plan (set, cancelled or done from anywhere)
                let now_key = plan_key(&p.plan.lock());
                if now_key != saved {
                    let plan = p.plan.lock().clone();
                    let _ = match plan {
                        Some(pl) => std::fs::write(plan_file(), serde_json::to_string(&pl).unwrap_or_default()),
                        None => std::fs::remove_file(plan_file()),
                    };
                    saved = now_key;
                }

                // ---- downloads keep-awake: off a minute after the last download it saw ends
                // (not before one starts, and not in the gap between two files)
                if tick % 5 == 0 && p.awake.lock().mode == "downloads" {
                    let busy = downloading(&app);
                    let mut a = p.awake.lock();
                    let was_seen = a.dl_seen;
                    if busy {
                        a.dl_seen = true;
                        a.dl_quiet_since = None;
                    } else if a.dl_seen {
                        let since = *a.dl_quiet_since.get_or_insert(now_s());
                        if now_s() - since >= 60 {
                            a.mode = "off".into();
                            a.dl_seen = false;
                            a.dl_quiet_since = None;
                            drop(a);
                            app.changed("power");
                            app.toast(Toast::simple("awake", "Downloads finished", "Your PC can sleep normally again."));
                            continue;
                        }
                    }
                    let changed = a.dl_seen != was_seen;
                    drop(a);
                    if changed {
                        app.changed("power");
                    }
                }

                // ---- breaks
                if tick % 15 == 0 {
                    breaks_tick(&app);
                }
            }
        })
        .ok();
}

fn plan_tick(app: &App) {
    let mut guard = app.power.plan.lock();
    let Some(plan) = guard.as_mut() else { return };
    let now = now_s();
    if plan.state == "countdown" {
        if now >= plan.countdown_until {
            let action = plan.action.clone();
            *guard = None;
            drop(guard);
            app.changed("power");
            app.dismiss_toast("plan");
            if action == "notify" {
                app.toast(Toast::simple("plan-done", "It's done", "The thing you were waiting for has finished."));
            } else if let Err(e) = run_action(&action) {
                app.toast(Toast::simple("plan-err", "Couldn't do it", &e));
            }
        }
        return;
    }
    // Is the condition met right now?
    let quiet_needed = plan.minutes.max(1) as u64 * 60;
    let (met_now, instant, active) = match plan.cond.as_str() {
        "time" => (now >= plan.at, true, None),
        "app_closes" => (!app.sys.running(&plan.app), true, None),
        "downloads" => {
            let busy = downloading(app);
            (!busy, false, Some(busy))
        }
        "cpu_idle" => (app.sys.cpu_now() < 12.0, false, None),
        "net_idle" => (app.sys.down_now() < 100_000.0, false, None),
        "media_stops" => {
            let playing = app.media.is_playing();
            (!playing, false, Some(playing))
        }
        "idle" => (idle_secs() >= quiet_needed, true, None),
        _ => (false, true, None),
    };
    let before = plan.progress.clone();
    let st = step(plan, now, met_now, instant, active);
    if st == Step::Missed {
        let label = action_label(&plan.action);
        let late = now - plan.at;
        *guard = None;
        drop(guard);
        app.changed("power");
        app.toast(Toast::simple("plan-missed", &format!("{label} skipped"), &format!("It was due {} ago, while the PC was asleep or off, so Deck didn't do it now.", fmt_dur(late / 60 * 60))));
        return;
    }
    let fire = st == Step::Fire;
    if !fire && plan.progress != before {
        drop(guard);
        app.changed("power");
        return;
    }
    if fire {
        plan.state = "countdown".into();
        plan.countdown_until = now + if plan.action == "notify" { 0 } else { COUNTDOWN_S };
        let label = action_label(&plan.action);
        let action = plan.action.clone();
        drop(guard);
        app.changed("power");
        if action != "notify" {
            app.toast(Toast {
                id: "plan".into(),
                kind: "plan".into(),
                title: format!("{label} in {COUNTDOWN_S} s"),
                body: "The condition you set was met.".into(),
                actions: vec![("cancel".into(), "Cancel".into()), ("now".into(), "Do it now".into())],
                sticky: true,
                sound: true,
            });
        }
    }
}

fn breaks_tick(app: &App) {
    let (on, every, len) = {
        let s = app.settings.lock();
        (s.breaks_on, s.breaks_every_min as u64 * 60, s.breaks_len_s)
    };
    let mut b = app.power.breaks.lock();
    if !on {
        b.active_since = None;
        return;
    }
    let now = now_s();
    // Two quiet minutes count as a break.
    if idle_secs() >= 120 {
        b.active_since = None;
        return;
    }
    let since = *b.active_since.get_or_insert(now);
    if now < b.snooze_until || now - since < every {
        return;
    }
    if crate::hotcorner::fullscreen_in_front() {
        return; // never interrupt a game or a video; ask again after it
    }
    b.active_since = Some(now);
    drop(b);
    let tips = [
        "Look at something far away for a bit.",
        "Stand up and stretch your back and shoulders.",
        "Roll your shoulders, unclench your jaw, drink some water.",
        "Walk around for a minute.",
    ];
    let tip = tips[(now as usize / 7) % tips.len()];
    app.toast(Toast {
        id: "break".into(),
        kind: "break".into(),
        title: format!("Break time · {}", fmt_dur(len as u64)),
        body: format!("You've been at it for {}. {tip}", fmt_dur(every)),
        actions: vec![("snooze".into(), "10 more min".into()), ("dismiss".into(), "Done".into())],
        sticky: true,
        sound: false,
    });
}

pub fn snooze_break(app: &App) {
    app.power.breaks.lock().snooze_until = now_s() + 600;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(cond: &str) -> Plan {
        Plan { action: "shutdown".into(), cond: cond.into(), minutes: 2, ..Default::default() }
    }

    #[test]
    fn downloads_must_start_before_they_can_finish() {
        let mut p = plan("downloads");
        // nothing downloading for an hour: still waiting, never fires
        for t in (0..3600).step_by(3) {
            assert_eq!(step(&mut p, t, true, false, Some(false)), Step::Wait);
        }
        assert!(p.progress.contains("Waiting for a download"));
        // a download runs, then stays finished for the quiet time
        assert_eq!(step(&mut p, 4000, false, false, Some(true)), Step::Wait);
        assert_eq!(step(&mut p, 4003, true, false, Some(false)), Step::Wait);
        assert_eq!(step(&mut p, 4003 + 119, true, false, Some(false)), Step::Wait);
        assert_eq!(step(&mut p, 4003 + 120, true, false, Some(false)), Step::Fire);
    }

    #[test]
    fn a_new_download_resets_the_quiet_time() {
        let mut p = plan("downloads");
        step(&mut p, 0, false, false, Some(true));
        step(&mut p, 10, true, false, Some(false));
        step(&mut p, 100, false, false, Some(true)); // the next file starts
        assert_eq!(step(&mut p, 110, true, false, Some(false)), Step::Wait);
        assert_eq!(step(&mut p, 200, true, false, Some(false)), Step::Wait, "only 90 s quiet");
        assert_eq!(step(&mut p, 230, true, false, Some(false)), Step::Fire);
    }

    #[test]
    fn music_must_play_before_it_can_stop() {
        let mut p = plan("media_stops");
        assert_eq!(step(&mut p, 0, true, false, Some(false)), Step::Wait);
        assert_eq!(step(&mut p, 600, true, false, Some(false)), Step::Wait);
        step(&mut p, 700, false, false, Some(true));
        assert_eq!(step(&mut p, 710, true, false, Some(false)), Step::Wait);
        assert_eq!(step(&mut p, 770, true, false, Some(false)), Step::Fire);
    }

    #[test]
    fn a_time_missed_while_asleep_is_skipped() {
        let mut p = Plan { at: 1000, ..plan("time") };
        assert_eq!(step(&mut p, 999, false, true, None), Step::Wait);
        assert_eq!(step(&mut p, 1000, true, true, None), Step::Fire);
        assert_eq!(step(&mut p, 1000 + 300, true, true, None), Step::Fire, "a few minutes late (Deck busy) still runs");
        assert_eq!(step(&mut p, 1000 + 8 * 3600, true, true, None), Step::Missed, "the next morning");
    }

    #[test]
    fn a_restored_plan_starts_waiting_again() {
        let p = Plan { state: "countdown".into(), countdown_until: 99, seen: true, quiet_since: Some(5), progress: "x".into(), ..plan("downloads") };
        let json = serde_json::to_string(&p).unwrap();
        let back = restored(serde_json::from_str(&json).unwrap());
        assert_eq!(back.state, "waiting");
        assert_eq!(back.countdown_until, 0);
        assert!(!back.seen && back.quiet_since.is_none(), "a download has to be seen again");
        assert_eq!((back.action.as_str(), back.cond.as_str(), back.minutes), ("shutdown", "downloads", 2));
        // after a restart with nothing downloading it keeps waiting, it never fires
        let mut back = back;
        assert_eq!(step(&mut back, 10_000, true, false, Some(false)), Step::Wait);
    }

    #[test]
    fn other_quiet_conditions_unchanged() {
        let mut p = Plan { minutes: 5, ..plan("cpu_idle") };
        assert_eq!(step(&mut p, 0, true, false, None), Step::Wait);
        assert_eq!(step(&mut p, 299, true, false, None), Step::Wait);
        assert_eq!(step(&mut p, 300, true, false, None), Step::Fire);
    }
}
