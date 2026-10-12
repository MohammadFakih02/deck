//! Opens Deck when the mouse rests in a screen corner. Polls the cursor
//! position (no input hooks, so games' anti-cheat has nothing to see) and
//! stays off while a fullscreen game or video is in front.

use crate::app::App;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowLongW, GetWindowRect, GWL_STYLE, WS_CAPTION};

/// A fullscreen game, video or presentation is in front.
pub fn fullscreen_in_front() -> bool {
    unsafe {
        if let Ok(s) = SHQueryUserNotificationState() {
            if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE {
                return true;
            }
        }
        // Borderless games don't always report "busy": compare with the monitor.
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls);
        let class = String::from_utf16_lossy(&cls[..n as usize]);
        if ["Progman", "WorkerW", "Shell_TrayWnd", "Windows.UI.Core.CoreWindow"].contains(&class.as_str()) {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        // A maximized app has a title bar (and with an auto-hiding taskbar it
        // also covers the screen); fullscreen games and videos don't.
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        covers && style & WS_CAPTION.0 != WS_CAPTION.0
    }
}

pub fn start(app: Arc<App>, trigger: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("hot-corner".into())
        .spawn(move || {
            let mut entered: Option<Instant> = None;
            let mut fired = false;
            loop {
                let (corner, dwell, allow_fs) = {
                    let s = app.settings.lock();
                    (s.hot_corner.clone(), s.hot_corner_ms as u64, s.hot_corner_fullscreen)
                };
                if corner == "off" {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                std::thread::sleep(Duration::from_millis(30));
                let mut p = POINT::default();
                if unsafe { GetCursorPos(&mut p) }.is_err() {
                    continue;
                }
                let inside = unsafe {
                    let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                    if !GetMonitorInfoW(mon, &mut mi).as_bool() {
                        false
                    } else {
                        let m = mi.rcMonitor;
                        let e = 3;
                        let left = p.x <= m.left + e;
                        let right = p.x >= m.right - 1 - e;
                        let top = p.y <= m.top + e;
                        let bottom = p.y >= m.bottom - 1 - e;
                        match corner.as_str() {
                            "top-left" => top && left,
                            "top-right" => top && right,
                            "bottom-left" => bottom && left,
                            "bottom-right" => bottom && right,
                            _ => false,
                        }
                    }
                };
                if !inside {
                    entered = None;
                    fired = false;
                    continue;
                }
                if fired {
                    continue;
                }
                // dragging something, or a mouse button held: not a request
                let held = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0 };
                if held {
                    entered = None;
                    continue;
                }
                let t = *entered.get_or_insert_with(Instant::now);
                if t.elapsed() >= Duration::from_millis(dwell) {
                    fired = true;
                    if !allow_fs && !app.is_visible() && fullscreen_in_front() {
                        continue;
                    }
                    trigger();
                }
            }
        })
        .ok();
}
