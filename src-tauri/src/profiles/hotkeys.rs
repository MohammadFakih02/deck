//! Keyboard shortcuts for profiles and the overlay: `RegisterHotKey` on one thread (no hooks,
//! so anti-cheat has nothing to object to). Keys another program holds are retried every 30 s.
//! Games can switch registered shortcuts off while they are in front (raw input with
//! `RIDEV_NOHOTKEYS`, League of Legends for one), so while a full-screen game is in front the same
//! thread also polls the keys with `GetAsyncKeyState` (every ~16 ms, so quick taps are seen) and runs a shortcut that
//! `WM_HOTKEY` didn't deliver within 100 ms. Without a game it only looks once a second for one.

use crate::app::App;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};

pub const MOD_ALT: u32 = 1;
pub const MOD_CONTROL: u32 = 2;
pub const MOD_SHIFT: u32 = 4;
pub const MOD_WIN: u32 = 8;
const MOD_NOREPEAT: u32 = 0x4000;

const WM_HOTKEY: u32 = 0x0312;
const WM_TIMER: u32 = 0x0113;
const WM_APP_RELOAD: u32 = 0x8000 + 1;

/// Key names Deck shows and accepts, beside letters, digits and F1–F24.
const NAMED: &[(&str, u32)] = &[
    ("Space", 0x20), ("PageUp", 0x21), ("PageDown", 0x22), ("End", 0x23), ("Home", 0x24), ("Left", 0x25), ("Up", 0x26), ("Right", 0x27), ("Down", 0x28),
    ("Insert", 0x2D), ("Delete", 0x2E), ("Pause", 0x13), ("ScrollLock", 0x91), ("Backspace", 0x08), ("Tab", 0x09), ("Enter", 0x0D),
    ("Num0", 0x60), ("Num1", 0x61), ("Num2", 0x62), ("Num3", 0x63), ("Num4", 0x64), ("Num5", 0x65), ("Num6", 0x66), ("Num7", 0x67), ("Num8", 0x68), ("Num9", 0x69),
    ("Num*", 0x6A), ("Num+", 0x6B), ("Num-", 0x6D), ("Num.", 0x6E), ("Num/", 0x6F),
    (";", 0xBA), ("=", 0xBB), (",", 0xBC), ("-", 0xBD), (".", 0xBE), ("/", 0xBF), ("`", 0xC0), ("[", 0xDB), ("\\", 0xDC), ("]", 0xDD), ("'", 0xDE),
];

pub fn key_name(vk: u32) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => Some(char::from_u32(vk).unwrap().to_string()),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6F)),
        _ => NAMED.iter().find(|n| n.1 == vk).map(|n| n.0.to_string()),
    }
}

fn key_vk(name: &str) -> Option<u32> {
    let n = name.trim();
    if n.len() == 1 {
        let c = n.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(f) = n.strip_prefix('F').or_else(|| n.strip_prefix('f')).and_then(|x| x.parse::<u32>().ok()) {
        if (1..=24).contains(&f) {
            return Some(0x6F + f);
        }
    }
    NAMED.iter().find(|x| x.0.eq_ignore_ascii_case(n)).map(|x| x.1)
}

/// "Ctrl+Alt+]" style text.
pub fn format(mods: u32, vk: u32) -> String {
    let mut v: Vec<String> = vec![];
    for (m, n) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
        if mods & m != 0 {
            v.push(n.into());
        }
    }
    v.push(key_name(vk).unwrap_or_else(|| format!("0x{vk:02X}")));
    v.join("+")
}

/// (modifiers, virtual key). A plain key must be one that isn't typed or used to move around
/// text (Page Up / Page Down, Home, End, Insert, Delete, Pause, Scroll Lock, F-keys, numpad
/// keys), as MSI Afterburner allowed; letters, digits, Space, Enter, Tab and arrows need Ctrl, Alt or Win.
pub fn parse(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // the key itself may be "+" ... no: "Num+" is the only plus key, and it is spelled out
    let (mods_part, key) = match s.rfind('+') {
        Some(i) if s[..i].ends_with("Num") => match s[..i - 3].rfind('+') {
            Some(j) => (&s[..j], &s[j + 1..]),
            None => ("", s),
        },
        Some(i) => (&s[..i], &s[i + 1..]),
        None => ("", s),
    };
    let mut mods = 0;
    for m in mods_part.split('+').map(str::trim).filter(|m| !m.is_empty()) {
        mods |= match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "alt" => MOD_ALT,
            "shift" => MOD_SHIFT,
            "win" => MOD_WIN,
            _ => return None,
        };
    }
    let vk = key_vk(key)?;
    let bare_ok = matches!(vk, 0x21..=0x24 | 0x2D | 0x2E | 0x13 | 0x91 | 0x70..=0x87 | 0x60..=0x6F);
    (mods & (MOD_CONTROL | MOD_ALT | MOD_WIN) != 0 || (mods == 0 && bare_ok)).then_some((mods, vk))
}

/// What a key does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Switch(String, String),
    Overlay,
    /// starts the framerate figures over
    ResetStats,
    /// RivaTuner's framerate limiter on/off
    Limiter,
    /// opens / closes Deck (works even when profile keys are switched off)
    Open,
    /// the death counter: +1 death / undo the last one
    Death,
    DeathUndo,
}

/// The keys Deck wants right now: (canonical key text, action).
pub fn wanted(app: &App) -> Vec<(String, Action)> {
    let s = app.settings.lock();
    let mut v = vec![(s.open_hotkey.clone(), Action::Open)];
    if s.deaths.on {
        v.push((s.deaths.hotkey_death.clone(), Action::Death));
        v.push((s.deaths.hotkey_undo.clone(), Action::DeathUndo));
    }
    if s.hotkeys_on {
        for p in &s.cpu_profiles {
            v.push((p.hotkey.clone(), Action::Switch("cpu".into(), p.id.clone())));
        }
        for p in &s.gpu_profiles {
            v.push((p.hotkey.clone(), Action::Switch("gpu".into(), p.id.clone())));
        }
        v.push((s.osd_hotkey.clone(), Action::Overlay));
        v.push((s.osd_reset_hotkey.clone(), Action::ResetStats));
        v.push((s.limiter_hotkey.clone(), Action::Limiter));
    }
    v.into_iter().filter_map(|(k, a)| parse(&k).map(|(m, vk)| (format(m, vk), a))).collect()
}

/// Another profile already uses `key` (for refusing duplicates in the editor).
pub fn taken_by(app: &App, key: &str, except_id: &str) -> Option<String> {
    let (m, vk) = parse(key)?;
    let k = format(m, vk);
    let s = app.settings.lock();
    let same = |h: &str| parse(h).map(|(a, b)| format(a, b) == k).unwrap_or(false);
    s.cpu_profiles.iter().filter(|p| p.id != except_id).find(|p| same(&p.hotkey)).map(|p| format!("CPU profile {}", p.name))
        .or_else(|| s.gpu_profiles.iter().filter(|p| p.id != except_id).find(|p| same(&p.hotkey)).map(|p| format!("GPU profile {}", p.name)))
        .or_else(|| (except_id != "overlay" && same(&s.osd_hotkey)).then(|| "the overlay key".to_string()))
        .or_else(|| (except_id != "overlay_reset" && same(&s.osd_reset_hotkey)).then(|| "the key that resets the framerate figures".to_string()))
        .or_else(|| (except_id != "limiter" && same(&s.limiter_hotkey)).then(|| "the framerate limit key".to_string()))
        .or_else(|| (except_id != "open" && same(&s.open_hotkey)).then(|| "the key that opens Deck".to_string()))
        .or_else(|| (except_id != "death" && same(&s.deaths.hotkey_death)).then(|| "the death counter's +1 key".to_string()))
        .or_else(|| (except_id != "death_undo" && same(&s.deaths.hotkey_undo)).then(|| "the death counter's undo key".to_string()))
}

/// A key for profile `id` (or "overlay" / "open"): Ok(normalized text, "" for none) or why not.
pub fn check(app: &App, key: &str, id: &str) -> Result<String, String> {
    if key.trim().is_empty() {
        return Ok(String::new());
    }
    let (m, v) = parse(key).ok_or("Use Ctrl, Alt or Win with a key. On its own it can be Page Up, Page Down, Home, End, Insert, Delete, Pause, Scroll Lock or an F-key.")?;
    let k = format(m, v);
    match taken_by(app, &k, id) {
        Some(who) => Err(format!("{k} is already used by {who}.")),
        None => Ok(k),
    }
}

/// key text → "ok" | "busy" (another program has it)
static STATUS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static THREAD: AtomicU32 = AtomicU32::new(0);
static APP: Mutex<Option<Weak<App>>> = Mutex::new(None);

pub fn status() -> Value {
    json!(STATUS.lock().clone().unwrap_or_default())
}

/// Re-reads the keys (call after any profile or setting change).
pub fn reload() {
    let t = THREAD.load(Ordering::SeqCst);
    if t != 0 {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(t, WM_APP_RELOAD, 0, 0);
        }
    }
}

/// Runs a shortcut's action off the hotkey thread (switching can take a moment: plans, NvAPI).
fn run(a: Option<Action>) {
    let app = APP.lock().as_ref().and_then(|w| w.upgrade());
    if let (Some(a), Some(app)) = (a, app) {
        std::thread::spawn(move || match a {
            Action::Switch(kind, id) => super::switch(&app, &kind, &id, "hotkey"),
            Action::Overlay => crate::rtss::toggle(&app),
            Action::ResetStats => crate::rtss::reset_stats(),
            Action::Limiter => crate::rtss::toggle_limiter(&app),
            Action::Open => app.toggle_window(),
            Action::Death => {
                // with no game yet nothing is in front, so a note can't land on top of a game
                if let Err(e) = crate::deaths::act(&app, "death", None, None, "hotkey") {
                    if crate::deaths::no_game_yet(&e) {
                        app.toast(crate::app::Toast::simple("deaths", "Death counter", &e));
                    }
                }
            }
            Action::DeathUndo => {
                let _ = crate::deaths::act(&app, "undo", None, None, "hotkey");
            }
        });
    }
}

/// Which bound shortcuts are down right now (after a reload or when a game comes to the front, so
/// a key that is already held isn't taken for a new press).
fn held_now(bound: &[(i32, String, Action, bool)]) -> HashMap<i32, bool> {
    bound.iter().filter_map(|(id, k, _, _)| parse(k).map(|(m, vk)| (*id, combo_down(m, vk)))).collect()
}

/// The key and exactly its modifiers are down right now (Ctrl+PageUp must not count as PageUp).
fn combo_down(m: u32, vk: u32) -> bool {
    let down = |k: i32| unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(k) as u16 & 0x8000 != 0 };
    down(vk as i32)
        && (m & MOD_CONTROL != 0) == down(0x11)
        && (m & MOD_ALT != 0) == down(0x12)
        && (m & MOD_SHIFT != 0) == down(0x10)
        && (m & MOD_WIN != 0) == (down(0x5B) || down(0x5C))
}

pub fn start(app: Arc<App>) {
    *APP.lock() = Some(Arc::downgrade(&app));
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, PeekMessageW, SetTimer, MSG, PM_NOREMOVE};
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE); // creates the queue
            THREAD.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::SeqCst);
            let retry_timer = SetTimer(std::ptr::null_mut(), 0, 30_000, None);
            // once a second until a full-screen game is in front, then every 30 ms
            let mut poll_timer = SetTimer(std::ptr::null_mut(), 0, 1000, None);
            let mut fast = false;
            let mut game_checked = std::time::Instant::now() - std::time::Duration::from_secs(5);
            // per shortcut id: key held at the last poll, a press seen by polling only, and when each path last ran it
            let mut held: HashMap<i32, bool> = HashMap::new();
            let mut pending: HashMap<i32, std::time::Instant> = HashMap::new();
            let mut by_message: HashMap<i32, std::time::Instant> = HashMap::new();
            let mut by_poll: HashMap<i32, std::time::Instant> = HashMap::new();
            let mut bound: Vec<(i32, String, Action, bool)> = vec![];
            let register = |bound: &mut Vec<(i32, String, Action, bool)>, retry_only: bool| {
                let Some(app) = APP.lock().as_ref().and_then(|w| w.upgrade()) else { return };
                if !retry_only {
                    for (id, _, _, ok) in bound.iter() {
                        if *ok {
                            UnregisterHotKey(std::ptr::null_mut(), *id);
                        }
                    }
                    bound.clear();
                    let mut seen = std::collections::HashSet::new();
                    for (i, (k, a)) in wanted(&app).into_iter().enumerate() {
                        if seen.insert(k.clone()) {
                            bound.push((i as i32 + 1, k, a, false));
                        }
                    }
                }
                let mut st = HashMap::new();
                for (id, k, _, ok) in bound.iter_mut() {
                    if !*ok {
                        let (m, vk) = parse(k).unwrap();
                        *ok = RegisterHotKey(std::ptr::null_mut(), *id, m | MOD_NOREPEAT, vk) != 0;
                    }
                    st.insert(k.clone(), if *ok { "ok" } else { "busy" }.to_string());
                }
                let changed = STATUS.lock().as_ref() != Some(&st);
                if changed {
                    let mut busy: Vec<&String> = st.iter().filter(|(_, v)| *v == "busy").map(|(k, _)| k).collect();
                    busy.sort();
                    if busy.is_empty() {
                        crate::log::info(format!("hotkeys: {} key(s) registered", st.len()));
                    } else {
                        crate::log::warn(format!("hotkeys: another program holds {} (tried again every 30 s); {} other key(s) registered", busy.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", "), st.len() - busy.len()));
                    }
                }
                *STATUS.lock() = Some(st);
                if changed {
                    app.changed("hotkeys");
                }
            };
            register(&mut bound, false);
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_HOTKEY => {
                        let id = msg.wParam as i32;
                        pending.remove(&id);
                        // the poll already ran this press (the message came late)
                        if by_poll.get(&id).is_some_and(|t| t.elapsed().as_millis() < 600) {
                            continue;
                        }
                        by_message.insert(id, std::time::Instant::now());
                        let a = bound.iter().find(|b| b.0 == id).map(|b| b.2.clone());
                        if a == Some(Action::Overlay) {
                            // the message carries when Windows posted it: how long it waited in this thread's queue
                            let queued = windows_sys::Win32::System::SystemInformation::GetTickCount().wrapping_sub(msg.time).min(60_000);
                            let at = std::time::Instant::now().checked_sub(std::time::Duration::from_millis(queued as u64)).unwrap_or_else(std::time::Instant::now);
                            crate::rtss::trace_key(at, format!("shortcut message, {queued} ms in the queue, game in front: {fast}"));
                        }
                        run(a);
                    }
                    WM_APP_RELOAD => {
                        register(&mut bound, false);
                        held = held_now(&bound);
                        pending.clear();
                    }
                    WM_TIMER if msg.wParam == poll_timer => {
                        if game_checked.elapsed().as_millis() >= 1000 {
                            game_checked = std::time::Instant::now();
                            let game = crate::hw::game::foreground_game().is_some();
                            if game != fast {
                                fast = game;
                                // the same id replaces the timer's interval; 15 ms is one timer tick (~15.6 ms)
                                let t = SetTimer(std::ptr::null_mut(), poll_timer, if fast { 15 } else { 1000 }, None);
                                if t != 0 {
                                    poll_timer = t;
                                }
                                held = held_now(&bound);
                                pending.clear();
                            }
                        }
                        if !fast {
                            continue;
                        }
                        for (id, k, a, ok) in bound.iter() {
                            if !*ok {
                                continue; // another program owns this key
                            }
                            let Some((m, vk)) = parse(k) else { continue };
                            let down = combo_down(m, vk);
                            let was = held.insert(*id, down).unwrap_or(false);
                            if down && !was && !by_message.get(id).is_some_and(|t| t.elapsed().as_millis() < 300) {
                                pending.insert(*id, std::time::Instant::now());
                            }
                            if let Some(seen) = pending.get(id).copied().filter(|t| t.elapsed().as_millis() >= 100) {
                                pending.remove(id);
                                by_poll.insert(*id, std::time::Instant::now());
                                if *a == Action::Overlay {
                                    crate::rtss::trace_key(seen, "no shortcut message within 100 ms, seen by polling".into());
                                }
                                run(Some(a.clone()));
                            }
                        }
                    }
                    WM_TIMER if msg.wParam == retry_timer && bound.iter().any(|b| !b.3) => {
                        register(&mut bound, true);
                    }
                    _ => {}
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for s in ["Ctrl+Alt+]", "Ctrl+Alt+O", "Shift+Win+F5", "Alt+Num+", "Ctrl+PageUp", "F13", "Ctrl+Alt+\\"] {
            let (m, vk) = parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(format(m, vk), s);
        }
        assert_eq!(parse("ctrl + alt + o"), Some((MOD_CONTROL | MOD_ALT, 0x4F)));
    }

    #[test]
    fn plain_page_keys_like_afterburner() {
        for s in ["PageUp", "PageDown", "Home", "F5", "Insert", "Pause"] {
            let (m, vk) = parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!((m, format(m, vk).as_str()), (0, s));
        }
    }

    #[test]
    fn refuses_bare_keys() {
        assert_eq!(parse("O"), None);
        assert_eq!(parse("Shift+O"), None);
        assert_eq!(parse("Shift+PageUp"), None, "Shift alone doesn't make a typing key safe");
        assert_eq!(parse("Space"), None);
        assert_eq!(parse("Up"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("Hyper+O"), None);
    }
}
