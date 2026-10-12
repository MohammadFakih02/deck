//! Window tools: list of open windows, pin on top, see-through, snap into a
//! layout's zones (also by dragging, FancyZones-style) and workspaces
//! (save where your apps are, bring them back with one click).

use crate::app::App;
use crate::settings::{Layout, Workspace, WsWindow, Zone};
use crate::util::{hwnd_from, hwnd_to, process_path};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Serialize, Clone)]
pub struct Win {
    pub hwnd: i64,
    pub title: String,
    pub exe: Option<String>,
    pub app: String,
    pub pid: u32,
    pub topmost: bool,
    pub opacity: u8,
    pub minimized: bool,
    pub maximized: bool,
    pub hung: bool,
    pub rect: [i32; 4],
}

unsafe extern "system" fn enum_cb(h: HWND, l: LPARAM) -> BOOL {
    let v = &mut *(l.0 as *mut Vec<HWND>);
    v.push(h);
    BOOL(1)
}

fn cloaked(h: HWND) -> bool {
    let mut c = 0u32;
    unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut c as *mut _ as *mut _, 4).is_ok() && c != 0 }
}

fn title_of(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..got as usize])
    }
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    pid
}

/// Top-level app windows, like the ones Alt+Tab shows.
pub fn list() -> Vec<Win> {
    let mut hs: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut hs as *mut _ as isize));
    }
    let me = std::process::id();
    let mut out = Vec::new();
    for h in hs {
        unsafe {
            if !IsWindowVisible(h).as_bool() || cloaked(h) {
                continue;
            }
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 {
                continue;
            }
            if GetWindow(h, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false) && ex & WS_EX_APPWINDOW.0 == 0 {
                continue;
            }
            let title = title_of(h);
            if title.is_empty() {
                continue;
            }
            let pid = pid_of(h);
            if pid == me {
                continue;
            }
            let exe = process_path(pid);
            let app = exe.as_deref().map(crate::audio::friendly_name).unwrap_or_else(|| title.clone());
            if exe.as_deref().map(|e| e.to_lowercase().ends_with("\\explorer.exe") && title == "Program Manager").unwrap_or(false) {
                continue;
            }
            let mut opacity = 255u8;
            if ex & WS_EX_LAYERED.0 != 0 {
                let mut a = 0u8;
                let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
                if GetLayeredWindowAttributes(h, None, Some(&mut a), Some(&mut flags)).is_ok() && flags.0 & LWA_ALPHA.0 != 0 {
                    opacity = a;
                }
            }
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            out.push(Win {
                hwnd: hwnd_to(h),
                title,
                app,
                exe,
                pid,
                topmost: ex & WS_EX_TOPMOST.0 != 0,
                opacity,
                minimized: IsIconic(h).as_bool(),
                maximized: IsZoomed(h).as_bool(),
                hung: IsHungAppWindow(h).as_bool(),
                rect: [r.left, r.top, r.right - r.left, r.bottom - r.top],
            });
        }
    }
    out
}

pub fn focus(h: HWND) {
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
}

/// Windows Deck made see-through (it added WS_EX_LAYERED), so 100% can take it off again.
static MADE_LAYERED: Mutex<Vec<i64>> = Mutex::new(Vec::new());

pub fn action(hwnd: i64, action: &str, value: Option<f64>) -> Result<(), String> {
    let h = hwnd_from(hwnd);
    unsafe {
        if !IsWindow(Some(h)).as_bool() {
            return Err("That window is gone".into());
        }
        match action {
            "focus" => focus(h),
            "min" => {
                let _ = ShowWindow(h, SW_MINIMIZE);
            }
            "max" => {
                let _ = ShowWindow(h, if IsZoomed(h).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
            }
            "close" => {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            "top" => {
                let on = value.map(|v| v > 0.5).unwrap_or_else(|| GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0);
                SetWindowPos(h, Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
                    .map_err(|_| "Windows didn't allow it (the app may run as administrator)".to_string())?;
            }
            "opacity" => {
                let a = (value.unwrap_or(255.0).clamp(40.0, 255.0)) as u8;
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                let mut ours = MADE_LAYERED.lock();
                if a == 255 && ours.contains(&hwnd) {
                    // back to solid: take off the layering Deck added. A layered window is drawn
                    // the slow way (no VRR / fullscreen optimisations, e.g. for a game).
                    ours.retain(|x| *x != hwnd);
                    SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as isize));
                    let _ = RedrawWindow(Some(h), None, None, RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN);
                    return Ok(());
                }
                if ex as u32 & WS_EX_LAYERED.0 == 0 {
                    SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
                    ours.retain(|x| IsWindow(Some(hwnd_from(*x))).as_bool());
                    ours.push(hwnd);
                }
                SetLayeredWindowAttributes(h, windows::Win32::Foundation::COLORREF(0), a, LWA_ALPHA).map_err(|e| e.to_string())?;
            }
            "center" => {
                let mon = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(mon, &mut mi);
                let w = mi.rcWork;
                let mut r = RECT::default();
                let _ = GetWindowRect(h, &mut r);
                let (ww, hh) = (r.right - r.left, r.bottom - r.top);
                let _ = SetWindowPos(h, None, w.left + (w.right - w.left - ww) / 2, w.top + (w.bottom - w.top - hh) / 2, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            }
            _ => return Err("unknown window action".into()),
        }
    }
    Ok(())
}

/// Work area of the monitor a window (or point) is on.
fn work_area_of(h: Option<HWND>, p: Option<POINT>) -> RECT {
    unsafe {
        let mon = match (h, p) {
            (Some(h), _) => MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST),
            (None, Some(p)) => MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST),
            _ => MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTONEAREST),
        };
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}

pub fn zone_rect(work: RECT, z: &Zone) -> RECT {
    let (w, h) = ((work.right - work.left) as f32, (work.bottom - work.top) as f32);
    RECT {
        left: work.left + (z.x * w).round() as i32,
        top: work.top + (z.y * h).round() as i32,
        right: work.left + ((z.x + z.w) * w).round() as i32,
        bottom: work.top + ((z.y + z.h) * h).round() as i32,
    }
}

/// Moves a window so its visible frame fills `r` (Windows 10/11 windows
/// have invisible resize borders around them).
pub fn place(h: HWND, r: RECT) {
    unsafe {
        if IsZoomed(h).as_bool() || IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let mut outer = RECT::default();
        let mut frame = RECT::default();
        let _ = GetWindowRect(h, &mut outer);
        let ok = DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut frame as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_ok();
        let (l, t, rr, b) = if ok {
            (frame.left - outer.left, frame.top - outer.top, outer.right - frame.right, outer.bottom - frame.bottom)
        } else {
            (0, 0, 0, 0)
        };
        let _ = SetWindowPos(h, None, r.left - l, r.top - t, (r.right - r.left) + l + rr, (r.bottom - r.top) + t + b, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn snap(hwnd: i64, layout: &Layout, zone: usize) -> Result<(), String> {
    let h = hwnd_from(hwnd);
    let z = layout.zones.get(zone).ok_or("No such zone")?;
    let work = work_area_of(Some(h), None);
    place(h, zone_rect(work, z));
    Ok(())
}

// ------------------------------------------------------------ drag to zones

/// While a window is dragged: the zone under the cursor (-1 = none).
pub static DRAG_ZONE: AtomicI32 = AtomicI32::new(-1);
pub static DRAGGING: AtomicBool = AtomicBool::new(false);
static DRAG_HWND: AtomicIsize = AtomicIsize::new(0);
static ZONES_ARMED: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<Arc<App>> = OnceLock::new();
type OverlayFn = Box<dyn Fn(Option<Value>) + Send + Sync>;
static OVERLAY: OnceLock<Mutex<Option<OverlayFn>>> = OnceLock::new();

fn overlay(v: Option<Value>) {
    if let Some(m) = OVERLAY.get() {
        if let Some(f) = m.lock().as_ref() {
            f(v);
        }
    }
}

unsafe extern "system" fn win_event(_: HWINEVENTHOOK, event: u32, h: HWND, id_obj: i32, _: i32, _: u32, _: u32) {
    if id_obj != 0 {
        return;
    }
    let Some(app) = APP.get() else { return };
    if event == EVENT_SYSTEM_MOVESIZESTART {
        let (on, trigger) = {
            let s = app.settings.lock();
            (s.zones_on, s.zones_trigger.clone())
        };
        if !on {
            return;
        }
        DRAG_HWND.store(h.0 as isize, Ordering::Relaxed);
        DRAGGING.store(true, Ordering::Relaxed);
        DRAG_ZONE.store(-1, Ordering::Relaxed);
        ZONES_ARMED.store(trigger == "always", Ordering::Relaxed);
        let app = app.clone();
        std::thread::spawn(move || drag_loop(app));
    } else if event == EVENT_SYSTEM_MOVESIZEEND && DRAGGING.swap(false, Ordering::Relaxed) {
        let zone = DRAG_ZONE.swap(-1, Ordering::Relaxed);
        let armed = ZONES_ARMED.swap(false, Ordering::Relaxed);
        overlay(None);
        if armed && zone >= 0 {
            let s = app.settings.lock();
            let layout = s.layouts.iter().find(|l| l.id == s.layout).cloned();
            drop(s);
            if let Some(l) = layout {
                let hw = HWND(DRAG_HWND.load(Ordering::Relaxed) as *mut _);
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let work = work_area_of(None, Some(p));
                if let Some(z) = l.zones.get(zone as usize) {
                    // after Windows finishes its own move
                    let r = zone_rect(work, z);
                    let hv = hw.0 as isize;
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(30));
                        place(HWND(hv as *mut _), r);
                    });
                }
            }
        }
    }
}

/// Follows the cursor during a drag: arms the zones when the right mouse
/// button is pressed (or always), and highlights the zone under the cursor.
fn drag_loop(app: Arc<App>) {
    let mut last: Option<(i32, bool)> = None;
    let mut rb_was = false;
    while DRAGGING.load(Ordering::Relaxed) {
        let rb = unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000 != 0 };
        let lb = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000 != 0 };
        if rb && !rb_was {
            // tap the right button to toggle
            let a = !ZONES_ARMED.load(Ordering::Relaxed);
            ZONES_ARMED.store(a, Ordering::Relaxed);
        }
        rb_was = rb;
        if !lb && !rb {
            // keyboard move/size or released: stop tracking; END will clean up
        }
        let armed = ZONES_ARMED.load(Ordering::Relaxed);
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        let work = work_area_of(None, Some(p));
        let (layout, zones_on) = {
            let s = app.settings.lock();
            (s.layouts.iter().find(|l| l.id == s.layout).cloned(), s.zones_on)
        };
        let Some(layout) = layout else { break };
        if !zones_on {
            break;
        }
        let mut zone = -1;
        if armed {
            for (i, z) in layout.zones.iter().enumerate() {
                let r = zone_rect(work, z);
                if p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom {
                    zone = i as i32;
                }
            }
        }
        DRAG_ZONE.store(zone, Ordering::Relaxed);
        if last != Some((zone, armed)) {
            last = Some((zone, armed));
            if armed {
                overlay(Some(json!({
                    "work": [work.left, work.top, work.right - work.left, work.bottom - work.top],
                    "zones": layout.zones,
                    "active": zone,
                    "name": layout.name,
                })));
            } else {
                overlay(None);
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    overlay(None);
}

pub fn start_zones(app: Arc<App>, show: OverlayFn) {
    let _ = APP.set(app);
    let _ = OVERLAY.set(Mutex::new(Some(show)));
    std::thread::Builder::new()
        .name("win-events".into())
        .spawn(|| unsafe {
            let _hook = SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, None, Some(win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
            let mut m = MSG::default();
            while GetMessageW(&mut m, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        })
        .ok();
}

// -------------------------------------------------------------- workspaces

/// Snapshot of the open app windows (exe + position) as a workspace.
pub fn capture() -> Vec<WsWindow> {
    list()
        .into_iter()
        .filter(|w| w.exe.is_some())
        .filter(|w| !w.exe.as_deref().unwrap_or("").to_lowercase().contains("\\windows\\systemapps\\"))
        .map(|w| {
            let h = hwnd_from(w.hwnd);
            let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            unsafe {
                let _ = GetWindowPlacement(h, &mut wp);
            }
            let r = wp.rcNormalPosition;
            WsWindow {
                exe: w.exe.unwrap_or_default(),
                title: w.title,
                x: r.left,
                y: r.top,
                w: r.right - r.left,
                h: r.bottom - r.top,
                state: if w.minimized { "min" } else if w.maximized { "max" } else { "normal" }.into(),
            }
        })
        .collect()
}

fn apply_to(h: HWND, w: &WsWindow) {
    unsafe {
        let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        let _ = GetWindowPlacement(h, &mut wp);
        let saved = RECT { left: w.x, top: w.y, right: w.x + w.w, bottom: w.y + w.h };
        // saved on a monitor that isn't connected now: keep the window where it is rather than
        // move it somewhere nobody can see it
        if !windows::Win32::Graphics::Gdi::MonitorFromRect(&saved, windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONULL).is_invalid() {
            wp.rcNormalPosition = saved;
        }
        wp.showCmd = match w.state.as_str() {
            "max" => SW_SHOWMAXIMIZED.0 as u32,
            "min" => SW_SHOWMINIMIZED.0 as u32,
            _ => SW_SHOWNORMAL.0 as u32,
        };
        let _ = SetWindowPlacement(h, &wp);
    }
}

fn exe_matches(w: &Win, name: &str) -> bool {
    w.exe.as_deref().map(|e| e.rsplit(['\\', '/']).next().unwrap_or(e).eq_ignore_ascii_case(name)).unwrap_or(false)
}

/// Closes apps the way clicking X does (they can save their work), then ends what is left in
/// the background after a few seconds. Apps that still show a window (probably "save
/// changes?") are left open; their names are returned.
fn close_gracefully(app: &App, names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut asked = false;
    for w in list() {
        if names.iter().any(|n| exe_matches(&w, n)) {
            unsafe {
                let _ = PostMessageW(Some(hwnd_from(w.hwnd)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            asked = true;
        }
    }
    if asked {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(400));
            if !names.iter().any(|n| app.sys.running(n)) {
                break;
            }
        }
    }
    let open = list();
    let mut asking = Vec::new();
    for n in names {
        if !app.sys.running(n) {
            continue;
        }
        if open.iter().any(|w| exe_matches(w, n)) {
            asking.push(app_name(n));
        } else {
            let _ = app.sys.kill_name(n); // only background processes are left: tray icons, helpers
        }
    }
    asking
}

fn app_name(exe: &str) -> String {
    let f = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    f.strip_suffix(".exe").or_else(|| f.strip_suffix(".EXE")).unwrap_or(f).to_string()
}

/// The toast after opening a workspace: says plainly what didn't come back.
fn ws_summary(name: &str, total: usize, placed: usize, not_started: &[String], slow: &[String]) -> (String, String) {
    let mut slow: Vec<String> = slow.iter().map(|e| app_name(e)).collect();
    slow.dedup();
    let mut parts = vec![format!("{placed} of {total} windows placed.")];
    if !not_started.is_empty() {
        parts.push(format!("Couldn't start {} (moved or uninstalled?).", not_started.iter().map(|e| app_name(e)).collect::<Vec<_>>().join(", ")));
    }
    if !slow.is_empty() {
        parts.push(format!("{} didn't open a window within 20 s.", slow.join(", ")));
    }
    if placed == total {
        (format!("{name} is ready"), format!("{total} windows placed."))
    } else {
        (format!("{name} is partly ready"), parts.join(" "))
    }
}

/// Opens a workspace: closes the apps it lists, switches audio, starts
/// missing apps and puts every window in its saved place.
pub fn open_workspace(app: &Arc<App>, ws: Workspace) {
    let app = app.clone();
    std::thread::spawn(move || {
        let asking = close_gracefully(&app, &ws.close);
        if !ws.audio_out.is_empty() {
            let _ = crate::audio::set_output_by_name(&ws.audio_out);
        }
        if ws.awake {
            let mut a = app.power.awake.lock();
            a.mode = "on".into();
        }
        let mut pending: Vec<&WsWindow> = Vec::new();
        let mut not_started: Vec<String> = Vec::new();
        let current = list();
        let mut used: Vec<i64> = Vec::new();
        for w in &ws.windows {
            let hit = current
                .iter()
                .filter(|c| !used.contains(&c.hwnd))
                .find(|c| c.exe.as_deref().map(|e| e.eq_ignore_ascii_case(&w.exe)).unwrap_or(false));
            match hit {
                Some(c) => {
                    used.push(c.hwnd);
                    apply_to(hwnd_from(c.hwnd), w);
                }
                None => {
                    // one launch per exe
                    if not_started.iter().any(|n| n.eq_ignore_ascii_case(&w.exe)) {
                        continue;
                    }
                    if !pending.iter().any(|p| p.exe.eq_ignore_ascii_case(&w.exe)) && (!std::path::Path::new(&w.exe).exists() || crate::util::launch_as_user(&w.exe, "", "").is_err()) {
                        not_started.push(w.exe.clone());
                        continue;
                    }
                    pending.push(w);
                }
            }
        }
        // Wait up to 20 s for launched apps' windows.
        for _ in 0..40 {
            if pending.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            let now = list();
            pending.retain(|w| {
                let hit = now
                    .iter()
                    .filter(|c| !used.contains(&c.hwnd))
                    .find(|c| c.exe.as_deref().map(|e| e.eq_ignore_ascii_case(&w.exe)).unwrap_or(false));
                if let Some(c) = hit {
                    used.push(c.hwnd);
                    apply_to(hwnd_from(c.hwnd), w);
                    false
                } else {
                    true
                }
            });
        }
        app.changed("power");
        let slow: Vec<String> = pending.iter().map(|w| w.exe.clone()).collect();
        let placed = ws.windows.len() - pending.len() - ws.windows.iter().filter(|w| not_started.iter().any(|n| n.eq_ignore_ascii_case(&w.exe))).count();
        let (title, mut body) = ws_summary(&ws.name, ws.windows.len(), placed, &not_started, &slow);
        if !asking.is_empty() {
            body.push_str(&format!(" Left open (asking to save?): {}.", asking.join(", ")));
        }
        app.toast(crate::app::Toast::simple("ws", &title, &body));
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_toast_says_what_went_wrong() {
        let (t, b) = ws_summary("Coding", 3, 3, &[], &[]);
        assert_eq!((t.as_str(), b.as_str()), ("Coding is ready", "3 windows placed."));
        let (t, b) = ws_summary("Coding", 4, 1, &["C:/Old/Tool.exe".into()], &["C:/Apps/Spotify.exe".into(), "C:/Apps/Spotify.exe".into()]);
        assert_eq!(t, "Coding is partly ready");
        assert_eq!(b, "1 of 4 windows placed. Couldn't start Tool (moved or uninstalled?). Spotify didn't open a window within 20 s.");
    }

    #[test]
    fn closing_matches_the_exe_file_name() {
        let w = |exe: &str| Win { hwnd: 1, title: String::new(), exe: Some(exe.into()), app: String::new(), pid: 1, topmost: false, opacity: 255, minimized: false, maximized: false, hung: false, rect: [0; 4] };
        assert!(exe_matches(&w("C:/Users/x/AppData/Local/Discord/app-1.0/Discord.exe"), "discord.exe"));
        assert!(exe_matches(&w(r"C:\Program Files\Spotify\Spotify.exe"), "Spotify.exe"));
        assert!(!exe_matches(&w("C:/Apps/DiscordPTB.exe"), "discord.exe"), "only the exact exe");
    }
}
