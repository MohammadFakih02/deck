//! Monitors (refresh rate, brightness over DDC/CI, HDR) and the Windows
//! light/dark theme with an optional schedule.

use crate::app::App;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use windows::core::{BOOL, HSTRING, PCWSTR};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes,
    GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    QueryDisplayConfig, SetMonitorBrightness, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    PHYSICAL_MONITOR, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, CDS_UPDATEREGISTRY,
    DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE, HDC,
    HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

#[derive(Serialize, Clone)]
pub struct Monitor {
    pub device: String,
    pub name: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    pub rates: Vec<u32>,
    pub brightness: Option<u32>,
    pub hdr_supported: bool,
    pub hdr_on: bool,
    pub rect: [i32; 4],
    pub work: [i32; 4],
}

unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, l: LPARAM) -> BOOL {
    let v = &mut *(l.0 as *mut Vec<HMONITOR>);
    v.push(h);
    BOOL(1)
}

pub fn hmonitors() -> Vec<HMONITOR> {
    let mut v: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

fn dm() -> DEVMODEW {
    DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() }
}

fn with_physical<T>(h: HMONITOR, f: impl FnOnce(&PHYSICAL_MONITOR) -> Option<T>) -> Option<T> {
    unsafe {
        let mut n = 0u32;
        GetNumberOfPhysicalMonitorsFromHMONITOR(h, &mut n).ok()?;
        if n == 0 {
            return None;
        }
        let mut pm = vec![PHYSICAL_MONITOR::default(); n as usize];
        GetPhysicalMonitorsFromHMONITOR(h, &mut pm).ok()?;
        let r = f(&pm[0]);
        let _ = DestroyPhysicalMonitors(&pm);
        r
    }
}

fn brightness(h: HMONITOR) -> Option<u32> {
    with_physical(h, |p| unsafe {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        if GetMonitorBrightness(p.hPhysicalMonitor, &mut min, &mut cur, &mut max) == 0 || max <= min {
            return None;
        }
        Some(((cur - min) * 100) / (max - min))
    })
}

/// Friendly monitor names ("MAG 255XF") and HDR state keyed by GDI device name.
fn display_config() -> Vec<(String, String, bool, bool, windows::Win32::Foundation::LUID, u32)> {
    let mut out = Vec::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).is_err() {
            return out;
        }
        for p in paths.iter().take(np as usize) {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: p.sourceInfo.adapterId,
                id: p.sourceInfo.id,
            };
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: p.targetInfo.adapterId,
                id: p.targetInfo.id,
            };
            let name = if DisplayConfigGetDeviceInfo(&mut tgt.header) == 0 { crate::util::from_wide(&tgt.monitorFriendlyDeviceName) } else { String::new() };
            let mut hdr = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            hdr.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                adapterId: p.targetInfo.adapterId,
                id: p.targetInfo.id,
            };
            let (sup, on) = if DisplayConfigGetDeviceInfo(&mut hdr.header) == 0 {
                let bits = hdr.Anonymous.value;
                (bits & 1 != 0, bits & 2 != 0)
            } else {
                (false, false)
            };
            out.push((crate::util::from_wide(&src.viewGdiDeviceName), name, sup, on, p.targetInfo.adapterId, p.targetInfo.id));
        }
    }
    out
}

pub fn monitors() -> Vec<Monitor> {
    let cfg = display_config();
    let mut out = Vec::new();
    for h in hmonitors() {
        unsafe {
            let mut mi = MONITORINFOEXW::default();
            mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO).as_bool() {
                continue;
            }
            let device = crate::util::from_wide(&mi.szDevice);
            let dev_w = HSTRING::from(device.as_str());
            let mut cur = dm();
            let _ = EnumDisplaySettingsW(&dev_w, ENUM_CURRENT_SETTINGS, &mut cur);
            let mut rates = Vec::new();
            let mut i = 0u32;
            loop {
                let mut m = dm();
                if !EnumDisplaySettingsW(&dev_w, ENUM_DISPLAY_SETTINGS_MODE(i), &mut m).as_bool() {
                    break;
                }
                if m.dmPelsWidth == cur.dmPelsWidth && m.dmPelsHeight == cur.dmPelsHeight && m.dmBitsPerPel == cur.dmBitsPerPel && !rates.contains(&m.dmDisplayFrequency) {
                    rates.push(m.dmDisplayFrequency);
                }
                i += 1;
                if i > 2000 {
                    break;
                }
            }
            rates.sort();
            let c = cfg.iter().find(|c| c.0 == device);
            let r = mi.monitorInfo.rcMonitor;
            let w = mi.monitorInfo.rcWork;
            out.push(Monitor {
                name: c.map(|c| c.1.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| device.trim_start_matches("\\\\.\\").to_string()),
                primary: mi.monitorInfo.dwFlags & 1 != 0,
                width: cur.dmPelsWidth,
                height: cur.dmPelsHeight,
                hz: cur.dmDisplayFrequency,
                rates,
                brightness: brightness(h),
                hdr_supported: c.map(|c| c.2).unwrap_or(false),
                hdr_on: c.map(|c| c.3).unwrap_or(false),
                rect: [r.left, r.top, r.right - r.left, r.bottom - r.top],
                work: [w.left, w.top, w.right - w.left, w.bottom - w.top],
                device,
            });
        }
    }
    out.sort_by_key(|m| !m.primary);
    out
}

pub fn set_refresh(device: &str, hz: u32) -> Result<(), String> {
    unsafe {
        let dev_w = HSTRING::from(device);
        let mut m = dm();
        if !EnumDisplaySettingsW(&dev_w, ENUM_CURRENT_SETTINGS, &mut m).as_bool() {
            return Err("Couldn't read the current display mode".into());
        }
        m.dmDisplayFrequency = hz;
        m.dmFields = DM_DISPLAYFREQUENCY;
        // ask the driver first, so a mode it can't show is refused without the screen going dark
        let t = ChangeDisplaySettingsExW(&dev_w, Some(&m), None, windows::Win32::Graphics::Gdi::CDS_TEST, None);
        if t != DISP_CHANGE_SUCCESSFUL {
            return Err(format!("This monitor can't run at {hz} Hz at its current resolution (code {})", t.0));
        }
        let r = ChangeDisplaySettingsExW(&dev_w, Some(&m), None, CDS_UPDATEREGISTRY, None);
        if r == DISP_CHANGE_SUCCESSFUL {
            crate::log::info(format!("display: {device} set to {hz} Hz"));
            Ok(())
        } else {
            Err(format!("Windows refused {hz} Hz (code {})", r.0))
        }
    }
}

pub fn set_brightness(device: &str, pct: u32) -> Result<(), String> {
    for h in hmonitors() {
        unsafe {
            let mut mi = MONITORINFOEXW::default();
            mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO).as_bool() && crate::util::from_wide(&mi.szDevice) == device {
                return with_physical(h, |p| {
                    let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
                    if GetMonitorBrightness(p.hPhysicalMonitor, &mut min, &mut cur, &mut max) == 0 {
                        return None;
                    }
                    let v = min + (max - min) * pct.min(100) / 100;
                    (SetMonitorBrightness(p.hPhysicalMonitor, v) != 0).then_some(())
                })
                .ok_or_else(|| "This monitor doesn't accept brightness changes (turn on DDC/CI in its menu)".to_string());
            }
        }
    }
    Err("Monitor not found".into())
}

pub fn set_hdr(device: &str, on: bool) -> Result<(), String> {
    let cfg = display_config();
    let c = cfg.iter().find(|c| c.0 == device).ok_or("Monitor not found")?;
    if !c.2 {
        return Err("This monitor doesn't support HDR".into());
    }
    unsafe {
        let mut s = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::default();
        s.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
            size: std::mem::size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() as u32,
            adapterId: c.4,
            id: c.5,
        };
        s.Anonymous.value = if on { 1 } else { 0 };
        let r = DisplayConfigSetDeviceInfo(&s.header);
        if r == 0 {
            crate::log::info(format!("display: HDR {} on {device}", if on { "on" } else { "off" }));
            Ok(())
        } else {
            Err(format!("Windows couldn't switch HDR (code {r})"))
        }
    }
}

// ------------------------------------------------------------------ theme

const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";

pub fn read_dword(key: &str, value: &str) -> Option<u32> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut v = 0u32;
    let mut len = 4u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_DWORD, None, Some(&mut v as *mut _ as *mut _), Some(&mut len)).ok().ok()?;
    }
    Some(v)
}

pub fn write_dword(key: &str, value: &str, v: u32) -> Result<(), String> {
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD};
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(value), REG_DWORD.0, Some(&v as *const _ as *const _), 4)
            .ok()
            .map_err(|e| e.to_string())
    }
}

pub fn is_dark() -> bool {
    read_dword(PERSONALIZE, "AppsUseLightTheme").map(|v| v == 0).unwrap_or(true)
}

pub fn set_dark(dark: bool) -> Result<(), String> {
    let v = if dark { 0 } else { 1 };
    write_dword(PERSONALIZE, "AppsUseLightTheme", v)?;
    write_dword(PERSONALIZE, "SystemUsesLightTheme", v)?;
    crate::log::info(format!("display: Windows set to {} mode", if dark { "dark" } else { "light" }));
    let s = crate::util::wide("ImmersiveColorSet");
    unsafe {
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(s.as_ptr() as isize), SMTO_ABORTIFHUNG, 200, None);
    }
    let _ = PCWSTR::null();
    Ok(())
}

/// Sunrise and sunset (local minutes after midnight) for today (NOAA formula).
pub fn sun_times(lat: f64, lon: f64) -> Option<(u32, u32)> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as f64;
    let day = (now / 86400.0).floor();
    let n = day + 2440587.5 - 2451545.0 + 0.0008;
    let js = n - lon / 360.0;
    let m = (357.5291 + 0.98560028 * js).rem_euclid(360.0).to_radians();
    let c = 1.9148 * m.sin() + 0.02 * (2.0 * m).sin() + 0.0003 * (3.0 * m).sin();
    let l = (m.to_degrees() + c + 180.0 + 102.9372).rem_euclid(360.0).to_radians();
    let jt = 2451545.0 + js + 0.0053 * m.sin() - 0.0069 * (2.0 * l).sin();
    let decl = (l.sin() * 23.4397f64.to_radians().sin()).asin();
    let cos_w = ((-0.833f64).to_radians().sin() - lat.to_radians().sin() * decl.sin()) / (lat.to_radians().cos() * decl.cos());
    if !(-1.0..=1.0).contains(&cos_w) {
        return None;
    }
    let w = cos_w.acos().to_degrees();
    let rise = jt - w / 360.0;
    let set = jt + w / 360.0;
    let to_local_min = |j: f64| -> u32 {
        let unix = (j - 2440587.5) * 86400.0;
        let off = local_offset_secs() as f64;
        (((unix + off).rem_euclid(86400.0)) / 60.0) as u32
    };
    Some((to_local_min(rise), to_local_min(set)))
}

fn local_offset_secs() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let r = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias };
    -(bias as i64) * 60
}

pub fn local_minutes_now() -> u32 {
    let now = crate::util::now_s() as i64 + local_offset_secs();
    (now.rem_euclid(86400) / 60) as u32
}

fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let (h, m) = (h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Whether the schedule says dark at `now` (all local minutes after midnight).
fn scheduled_dark(now: u32, light: u32, dark: u32) -> bool {
    if light < dark {
        now < light || now >= dark
    } else {
        now >= dark && now < light
    }
}

/// The schedule acts when its period changes (or right after Deck starts or
/// the schedule is edited), not every minute, so switching the theme by hand
/// holds until the next switch time.
#[derive(Default)]
struct Schedule {
    last: Option<bool>,
}

impl Schedule {
    fn should_apply(&mut self, want_dark: bool) -> bool {
        let changed = self.last != Some(want_dark);
        self.last = Some(want_dark);
        changed
    }
    fn reset(&mut self) {
        self.last = None;
    }
}

static SCHEDULE: Mutex<Schedule> = Mutex::new(Schedule { last: None });

/// (light_at, dark_at) in local minutes, from the settings.
pub fn schedule(app: &App) -> Option<(u32, u32)> {
    let s = app.settings.lock();
    if s.theme_mode == "sun" {
        sun_times(s.lat?, s.lon?)
    } else {
        Some((parse_hm(&s.light_at)?, parse_hm(&s.dark_at)?))
    }
}

/// Applies the theme schedule when it reaches a switch time (called every minute).
pub fn theme_tick(app: &App) {
    if !app.settings.lock().theme_auto {
        return;
    }
    let Some((light, dark)) = schedule(app) else { return };
    let want_dark = scheduled_dark(local_minutes_now(), light, dark);
    if !SCHEDULE.lock().should_apply(want_dark) {
        return;
    }
    if want_dark != is_dark() {
        let _ = set_dark(want_dark);
        app.changed("display");
    }
}

/// After the schedule settings change: apply them now.
pub fn theme_schedule_changed(app: &App) {
    SCHEDULE.lock().reset();
    theme_tick(app);
}

pub fn state(app: &App) -> Value {
    let sched = schedule(app);
    json!({
        "monitors": monitors(),
        "dark": is_dark(),
        "schedule": sched.map(|(l, d)| json!({ "light": format!("{:02}:{:02}", l / 60, l % 60), "dark": format!("{:02}:{:02}", d / 60, d % 60) })),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_hm, scheduled_dark, Schedule};

    #[test]
    fn a_manual_theme_switch_holds_until_the_next_change() {
        // light at 07:00, dark at 19:00
        assert!(scheduled_dark(20 * 60, 7 * 60, 19 * 60));
        assert!(!scheduled_dark(12 * 60, 7 * 60, 19 * 60));
        assert!(scheduled_dark(3 * 60, 7 * 60, 19 * 60));
        // night shift: dark at 06:00, light at 22:00
        assert!(scheduled_dark(8 * 60, 22 * 60, 6 * 60));
        let mut s = Schedule::default();
        assert!(s.should_apply(true), "first tick after start applies the schedule");
        assert!(!s.should_apply(true), "same period: leave the user's choice alone");
        assert!(!s.should_apply(true));
        assert!(s.should_apply(false), "the switch time passed: apply");
        s.reset();
        assert!(s.should_apply(false), "after a schedule edit it applies again at once");
    }

    #[test]
    fn times_must_be_real() {
        assert_eq!(parse_hm("07:30"), Some(450));
        assert_eq!(parse_hm(" 7 : 05 "), Some(425));
        assert_eq!(parse_hm("25:00"), None);
        assert_eq!(parse_hm("12:60"), None);
        assert_eq!(parse_hm("noon"), None);
    }
}

/// Every font family installed on this PC, sorted (for the overlay's font list).
pub fn fonts() -> Vec<String> {
    use windows::Win32::Graphics::Gdi::{EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW};
    unsafe extern "system" fn each(lf: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, out: LPARAM) -> i32 {
        let names = &mut *(out.0 as *mut Vec<String>);
        let face = &(*lf).lfFaceName;
        let n = face.iter().position(|&c| c == 0).unwrap_or(face.len());
        names.push(String::from_utf16_lossy(&face[..n]));
        1
    }
    let mut names: Vec<String> = Vec::new();
    unsafe {
        let dc = GetDC(None);
        let lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
        EnumFontFamiliesExW(dc, &lf, Some(each), LPARAM(&mut names as *mut _ as isize), 0);
        ReleaseDC(None, dc);
    }
    tidy_fonts(names)
}

/// Drops the sideways "@" variants and repeats, and sorts by name.
fn tidy_fonts(mut names: Vec<String>) -> Vec<String> {
    names.retain(|n| !n.is_empty() && !n.starts_with('@'));
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

#[cfg(test)]
mod font_tests {
    #[test]
    fn font_list_is_sorted_without_repeats_or_sideways_fonts() {
        let got = super::tidy_fonts(["Segoe UI", "@MS Gothic", "arial", "Segoe UI", "Consolas", ""].iter().map(|s| s.to_string()).collect());
        assert_eq!(got, ["arial", "Consolas", "Segoe UI"]);
    }

    #[test]
    #[ignore = "reads this PC's fonts"]
    fn this_pc_has_fonts() {
        let f = super::fonts();
        println!("{} fonts: {:?}", f.len(), &f[..f.len().min(10)]);
        assert!(f.iter().any(|n| n == "Segoe UI"));
    }
}
