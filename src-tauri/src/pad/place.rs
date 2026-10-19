//! Where the overlay goes: monitors (leftmost first), the one the game is on, and the corner +
//! offset maths. Offsets are stored in 96-DPI units from the nearest edge, so a position
//! survives resolution and scale changes.

use super::cfg::PadSettings;

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub handle: isize,
    /// left, top, right, bottom in physical pixels
    pub rect: (i32, i32, i32, i32),
    pub dpi: u32,
    pub primary: bool,
}

impl Monitor {
    pub fn dk(&self) -> f32 {
        self.dpi.max(48) as f32 / 96.0
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.rect.0..self.rect.2).contains(&x) && (self.rect.1..self.rect.3).contains(&y)
    }
}

/// Monitors sorted left to right, so "monitor 1" is the leftmost.
pub fn monitors() -> Vec<Monitor> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> i32 {
        let out = &mut *(lp as *mut Vec<Monitor>);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(h, &mut mi) != 0 {
            let (mut x, mut y) = (96u32, 96u32);
            if GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut x, &mut y) != 0 {
                x = 96;
            }
            let r = mi.rcMonitor;
            out.push(Monitor { handle: h as isize, rect: (r.left, r.top, r.right, r.bottom), dpi: x, primary: mi.dwFlags & 1 != 0 });
        }
        1
    }
    let mut out: Vec<Monitor> = Vec::new();
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(cb), &mut out as *mut _ as LPARAM) };
    out.sort_by_key(|m| (m.rect.0, m.rect.1));
    out
}

/// Taskbars, the tray flyout and the desktop: focusing them is not "the game moved".
pub const SHELL_CLASSES: [&str; 6] = ["Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Progman", "WorkerW", "NotifyIconOverflowWindow", "TopLevelWindowForOverflowXamlIsland"];

/// Whether a foreground window says nothing about where the game is: one of Deck's own windows
/// (the overlay, its toast, Deck's page) or the shell.
pub fn ignore_fg(class: &str, deck_own: bool) -> bool {
    deck_own || SHELL_CLASSES.contains(&class)
}

/// The monitor to use: a fixed index, else the one the last real foreground window is on
/// (`auto_hmon`), else the primary.
pub fn pick<'a>(mons: &'a [Monitor], s: &PadSettings, auto_hmon: isize) -> Option<&'a Monitor> {
    let primary = mons.iter().find(|m| m.primary).or(mons.first())?;
    Some(match s.monitor_index() {
        Some(i) => mons.get(i).unwrap_or(primary),
        None => mons.iter().find(|m| m.handle == auto_hmon).unwrap_or(primary),
    })
}

/// Top-left corner for a `w` × `h` overlay on `mon`, never off-screen.
pub fn geometry(s: &PadSettings, mon: &Monitor, w: i32, h: i32) -> (i32, i32) {
    let (l, t, r, b) = mon.rect;
    let dk = mon.dk() as f64;
    let (ox, oy) = (s.off_x * dk, s.off_y * dk);
    let (l, t, r, b, w, h) = (l as f64, t as f64, r as f64, b as f64, w as f64, h as f64);
    let x = match s.anchor_h.as_str() {
        "left" => l + ox,
        "right" => r - w - ox,
        _ => (l + r - w) / 2.0 + ox,
    };
    let y = if s.anchor_v == "top" { t + oy } else { b - h - oy };
    let x = x.min(r - w).max(l);
    let y = y.min(b - h).max(t);
    (x.round() as i32, y.round() as i32)
}

/// A drop point as anchors and offsets from the nearest edge: (anchor_h, anchor_v, off_x, off_y),
/// offsets in 96-DPI units rounded to 0.1.
pub fn anchor_from(x: i32, y: i32, w: i32, h: i32, mon: &Monitor) -> (&'static str, &'static str, f64, f64) {
    let (l, t, r, b) = (mon.rect.0 as f64, mon.rect.1 as f64, mon.rect.2 as f64, mon.rect.3 as f64);
    let (x, y, w, h) = (x as f64, y as f64, w as f64, h as f64);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let dk = mon.dk() as f64;
    let fx = (cx - l) / (r - l).max(1.0);
    let (ah, ox) = if fx < 1.0 / 3.0 {
        ("left", x - l)
    } else if fx > 2.0 / 3.0 {
        ("right", r - (x + w))
    } else {
        ("center", cx - (l + r) / 2.0)
    };
    let (av, oy) = if cy - t < (b - t) / 2.0 { ("top", y - t) } else { ("bottom", b - (y + h)) };
    let r1 = |v: f64| (v / dk * 10.0).round() / 10.0;
    (ah, av, r1(ox), r1(oy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(l: i32, dpi: u32, primary: bool) -> Monitor {
        Monitor { handle: l as isize + 1, rect: (l, 0, l + 1920, 1080), dpi, primary }
    }

    #[test]
    fn clamped_inside_monitor() {
        let m = mon(0, 96, true);
        let mut s = PadSettings { off_x: 4000.0, off_y: -300.0, ..Default::default() };
        let (x, y) = geometry(&s, &m, 409, 272);
        assert_eq!((x, y), (0, 1080 - 272));
        s.off_x = 24.0;
        s.off_y = 24.0;
        assert_eq!(geometry(&s, &m, 409, 272), (1920 - 409 - 24, 1080 - 272 - 24));
        // a second monitor to the right, at 150%
        let m2 = mon(1920, 144, false);
        s.anchor_h = "left".into();
        s.anchor_v = "top".into();
        assert_eq!(geometry(&s, &m2, 600, 400), (1920 + 36, 36));
    }

    #[test]
    fn anchor_round_trip() {
        let m = mon(0, 120, true);
        for (x, y) in [(10, 10), (900, 500), (1500, 700), (100, 800)] {
            let (ah, av, ox, oy) = anchor_from(x, y, 300, 200, &m);
            let s = PadSettings { anchor_h: ah.into(), anchor_v: av.into(), off_x: ox, off_y: oy, ..Default::default() };
            let (gx, gy) = geometry(&s, &m, 300, 200);
            assert!((gx - x).abs() <= 1 && (gy - y).abs() <= 1, "{x},{y} -> {gx},{gy}");
        }
    }

    #[test]
    fn shell_and_own_windows_ignored() {
        assert!(ignore_fg("Shell_TrayWnd", false));
        assert!(ignore_fg("WorkerW", false));
        assert!(ignore_fg("UnrealWindow", true));
        assert!(!ignore_fg("UnrealWindow", false));
    }

    #[test]
    fn pick_falls_back_to_primary() {
        let mons = vec![mon(0, 96, false), mon(1920, 96, true)];
        let s = PadSettings::default();
        assert_eq!(pick(&mons, &s, 0).unwrap().rect.0, 1920);
        assert_eq!(pick(&mons, &s, 1).unwrap().rect.0, 0);
        let s = PadSettings { monitor: serde_json::json!(5), ..Default::default() };
        assert_eq!(pick(&mons, &s, 1).unwrap().rect.0, 1920, "a missing monitor index uses the primary");
        assert!(pick(&[], &s, 0).is_none());
    }
}
