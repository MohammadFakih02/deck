//! The support report (This PC → Copy report / Save report): everything needed to fix a problem
//! someone has with Deck, on any PC or laptop, with or without admin rights and whether or not
//! the hardware engine runs. Pasted into a chat or an issue, it should answer "what is this
//! machine and what did Deck find" without a second round of questions.
//! Nothing personal: the user name in paths is replaced and drive serial numbers are left out.

use crate::app::App;
use std::fmt::Write;

pub fn report(app: &App) -> String {
    let mut r = String::new();
    let sec = |r: &mut String, title: &str| {
        let _ = write!(r, "\r\n== {title} ==\r\n");
    };
    let _ = write!(r, "Deck support report · Deck {} · {}\r\n", env!("CARGO_PKG_VERSION"), now());

    // ---- Deck itself
    sec(&mut r, "Deck");
    let admin = crate::elevation::is_admin();
    let engine = app.hw.lock().clone();
    let _ = write!(r, "Running as admin: {}\r\n", if admin { "yes" } else { "no (fans, lighting, sensors, drive health and GPU tuning need it)" });
    let _ = write!(r, "Hardware engine: {}\r\n", match (&engine, admin) {
        (Some(_), _) => "running".to_string(),
        (None, false) => "not running: Deck isn't admin".to_string(),
        (None, true) => "not running (still starting, or it failed to start)".to_string(),
    });
    let _ = write!(r, "PawnIO driver (fans, CPU temperature, RAM lighting): {}\r\n", if crate::hw::pawn::device_present() { "running" } else { "not running" });
    let _ = write!(r, "GPU engine (NVIDIA): {}\r\n", app.gpu.lock().as_ref().map(|g| format!("{} · driver {}", g.caps.name, g.caps.driver)).unwrap_or_else(|| "not loaded (no NVIDIA card, or not admin)".into()));
    if let Ok(exe) = std::env::current_exe() {
        let _ = write!(r, "Installed at: {}\r\n", private(&exe.to_string_lossy()));
    }
    {
        let s = app.settings.lock();
        let _ = write!(r, "Overlay: {} · key {} · reset key {} · shortcut keys {}\r\n", on(s.osd_on), or_none(&s.osd_hotkey), or_none(&s.osd_reset_hotkey), on(s.hotkeys_on));
    }
    let keys = crate::profiles::hotkeys::status();
    if let Some(m) = keys.as_object().filter(|m| !m.is_empty()) {
        let list: Vec<String> = m.iter().map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or("?"))).collect();
        let _ = write!(r, "Shortcut keys (busy = another program has it): {}\r\n", list.join(", "));
    }

    // ---- Windows
    sec(&mut r, "Windows");
    let cv = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build: u32 = reg_str(cv, "CurrentBuild").and_then(|b| b.parse().ok()).unwrap_or(0);
    let product = reg_str(cv, "ProductName").unwrap_or_else(|| "Windows".into());
    // ProductName still says "Windows 10" on Windows 11
    let product = if build >= 22000 { product.replace("Windows 10", "Windows 11") } else { product };
    let _ = write!(r, "{} {} (build {}.{}) · {}\r\n", product, reg_str(cv, "DisplayVersion").unwrap_or_default(), build, reg_dword(cv, "UBR").unwrap_or(0), std::env::consts::ARCH);

    // ---- the machine
    sec(&mut r, "Computer");
    let fw = crate::hw::smbios::read();
    let _ = write!(r, "Type: {} · {} {}\r\n", fw.form_factor(), fw.sys_vendor, fw.sys_product);
    let _ = write!(r, "Motherboard: {} {} · BIOS {}\r\n", fw.board_vendor, fw.board_name, fw.bios);
    let _ = write!(r, "Battery: {}\r\n", battery());
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let _ = write!(r, "Processor: {} · {} threads\r\n", brand, sys.cpus().len());
    let _ = write!(r, "Memory: {:.1} GB", sys.total_memory() as f64 / 1_073_741_824.0);
    if !fw.dimms.is_empty() {
        let list: Vec<String> = fw.dimms.iter().map(|(m, p)| format!("{m} {p}").trim().to_string()).collect();
        let _ = write!(r, " · {}", list.join(", "));
    }
    r.push_str("\r\n");
    for (name, vram) in adapters() {
        let _ = write!(r, "Graphics: {} · {:.1} GB\r\n", name, vram as f64 / 1_073_741_824.0);
    }

    // ---- what the hardware engine found (fans, sensors, lighting)
    sec(&mut r, "Hardware");
    match &engine {
        Some(sh) => r.push_str(&crate::hw::report_body(sh)),
        None => r.push_str("The hardware engine isn't running, so fans, sensors and lighting weren't read.\r\n"),
    }

    // ---- the in-game overlay
    sec(&mut r, "Overlay (RivaTuner)");
    match crate::rtss::rtss_exe() {
        None => r.push_str("RivaTuner isn't installed\r\n"),
        Some(exe) => {
            let ver = reg_str(r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\RTSS", "DisplayVersion").unwrap_or_else(|| "?".into());
            let _ = write!(r, "Installed: {} · version {}\r\n", private(&exe.to_string_lossy()), ver);
            let _ = write!(r, "Running: {}\r\n", on(crate::rtss::rtss_running()));
            // the switch follows Deck's overlay key: off is only a problem while Deck's overlay is on
            let (v32, v64) = crate::rtss::osd_switch();
            let deck_on = app.settings.lock().osd_on;
            let show = |v: Option<bool>| match (v, deck_on) {
                (Some(true), _) => "on",
                (Some(false), false) => "off (follows Deck's overlay, which is off)",
                (Some(false), true) => "OFF while Deck's overlay is on: nothing will show in games",
                (None, _) => "unknown",
            };
            let _ = write!(r, "Show On-Screen Display switch: {} · 64-bit games: {}\r\n", show(v32), show(v64));
        }
    }

    // ---- drives
    sec(&mut r, "Drives");
    let drives = crate::hw::smart::cached(300);
    if drives.is_empty() {
        r.push_str("No drives read (drive health needs admin)\r\n");
    }
    for d in drives {
        let _ = write!(r, "{} · {}{} · {:.0} GB · {}{}", d.model, d.bus, if d.ssd { " SSD" } else { "" }, d.size as f64 / 1e9, d.status, d.health.map(|h| format!(" {h}%")).unwrap_or_default());
        if let Some(t) = d.temp {
            let _ = write!(r, " · {t:.0} °C");
        }
        if let Some(h) = d.hours {
            let _ = write!(r, " · {h} h");
        }
        r.push_str("\r\n");
        for why in &d.reasons {
            let _ = write!(r, "  - {why}\r\n");
        }
        for past in &d.history {
            let _ = write!(r, "  - earlier (fine now): {past}\r\n");
        }
    }
    private(&r)
}

/// "2026-10-14 05:20" in local time.
fn now() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{}-{:02}-{:02} {:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute)
}

fn on(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}
fn or_none(s: &str) -> &str {
    if s.is_empty() {
        "none"
    } else {
        s
    }
}

/// The user's name out of paths: C:\Users\<name> → %USERPROFILE%.
fn private(s: &str) -> String {
    let mut out = s.to_string();
    if let Ok(home) = std::env::var("USERPROFILE") {
        if !home.is_empty() {
            out = replace_ci(&out, &home, "%USERPROFILE%");
        }
    }
    if let Ok(user) = std::env::var("USERNAME") {
        if user.len() > 2 {
            out = replace_ci(&out, &format!("\\{user}\\"), "\\<user>\\");
        }
    }
    out
}

fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    let (lh, ln) = (hay.to_lowercase(), needle.to_lowercase());
    if ln.is_empty() || lh.len() != hay.len() {
        return hay.replace(needle, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = lh[i..].find(&ln) {
        out.push_str(&hay[i..i + p]);
        out.push_str(with);
        i += p + ln.len();
    }
    out.push_str(&hay[i..]);
    out
}

fn reg_str(key: &str, value: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = vec![0u16; 512];
    let mut len = (buf.len() * 2) as u32;
    unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) }.ok().ok()?;
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]).trim().to_string())
}

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    let mut v = 0u32;
    let mut len = 4u32;
    unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_DWORD, None, Some(&mut v as *mut _ as *mut _), Some(&mut len)) }.ok().ok()?;
    Some(v)
}

fn battery() -> String {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return "unknown".into();
    }
    if s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 {
        return "none".into();
    }
    format!("{}% · {}", s.BatteryLifePercent, if s.ACLineStatus == 1 { "plugged in" } else { "on battery" })
}

/// Every graphics adapter Windows knows (integrated and dedicated), with its own memory.
fn adapters() -> Vec<(String, u64)> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let mut out = vec![];
    let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return out };
    let mut i = 0;
    while let Ok(a) = unsafe { f.EnumAdapters1(i) } {
        i += 1;
        let Ok(d) = (unsafe { a.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let n = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        out.push((String::from_utf16_lossy(&d.Description[..n]), d.DedicatedVideoMemory as u64));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn user_name_is_taken_out_of_paths() {
        let home = std::env::var("USERPROFILE").unwrap();
        let s = super::private(&format!("{}\\AppData\\Local\\Deck\\deck.exe", home.to_uppercase()));
        assert_eq!(s, "%USERPROFILE%\\AppData\\Local\\Deck\\deck.exe");
    }

    /// Live: prints the report's machine part (no App needed for these).
    #[test]
    #[ignore]
    fn live_bits() {
        let fw = crate::hw::smbios::read();
        println!("{} · {} {} · battery {}", fw.form_factor(), fw.sys_vendor, fw.sys_product, super::battery());
        println!("{:?}", super::adapters());
        println!("{:?} {:?}", super::reg_str(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "DisplayVersion"), super::reg_dword(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "UBR"));
    }
}
