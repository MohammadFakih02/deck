//! Apps that start with Windows, with the same on/off switch Task Manager
//! uses (the StartupApproved registry keys). Entries for all users need
//! administrator rights to change: they are read-only unless Deck is elevated.

use serde::Serialize;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_BINARY, REG_VALUE_TYPE,
};

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub exe: Option<String>,
    /// "HKCU Run" | "HKLM Run" | "Startup folder" | ...
    pub source: String,
    pub enabled: bool,
    pub can_change: bool,
}

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN32: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";

fn values(root: HKEY, path: &str) -> Vec<(String, Vec<u8>, u32)> {
    let mut out = Vec::new();
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ | KEY_WOW64_64KEY, &mut k) != ERROR_SUCCESS {
            return out;
        }
        let mut i = 0u32;
        loop {
            let mut name = vec![0u16; 512];
            let mut name_len = name.len() as u32;
            let mut ty = 0u32;
            let mut data = vec![0u8; 8192];
            let mut data_len = data.len() as u32;
            let r = RegEnumValueW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut data_len));
            if r != ERROR_SUCCESS {
                break;
            }
            data.truncate(data_len as usize);
            out.push((String::from_utf16_lossy(&name[..name_len as usize]), data, ty));
            i += 1;
        }
        let _ = RegCloseKey(k);
    }
    out
}

fn approved(root: HKEY, sub: &str, name: &str) -> bool {
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(format!("{APPROVED}\\{sub}")), None, KEY_READ, &mut k) != ERROR_SUCCESS {
            return true;
        }
        let mut data = [0u8; 16];
        let mut len = data.len() as u32;
        let mut ty = REG_VALUE_TYPE(0);
        let r = RegQueryValueExW(k, &HSTRING::from(name), None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut len));
        let _ = RegCloseKey(k);
        // odd first byte (03, 07...) = disabled
        r != ERROR_SUCCESS || len == 0 || data[0] & 1 == 0
    }
}

fn wide_str(data: &[u8]) -> String {
    let w: Vec<u16> = data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    crate::util::from_wide(&w)
}

/// "\"C:\\x\\app.exe\" --min" -> C:\x\app.exe
fn exe_of(cmd: &str) -> Option<String> {
    let c = cmd.trim();
    let raw = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        let lower = c.to_lowercase();
        match lower.find(".exe") {
            Some(i) => c[..i + 4].to_string(),
            None => c.split_whitespace().next()?.to_string(),
        }
    };
    let expanded = expand_env(&raw);
    Some(expanded)
}

fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) => {
                let var = &after[..b];
                out.push_str(&std::env::var(var).unwrap_or_else(|_| format!("%{var}%")));
                rest = &after[b + 1..];
            }
            None => {
                out.push_str(&rest[a..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn pretty(name: &str, exe: Option<&str>) -> String {
    // "Update.exe --processStart Discord.exe": the file name says nothing,
    // the registry name ("Discord") does.
    const GENERIC: &[&str] = &["update", "updater", "launcher", "setup", "helper", "service", "agent", "run", "start", "app"];
    if let Some(e) = exe {
        if std::path::Path::new(e).exists() {
            let n = crate::audio::friendly_name(e);
            if !n.is_empty() && !GENERIC.contains(&n.to_lowercase().as_str()) {
                return n;
            }
        }
    }
    // "MicrosoftEdgeAutoLaunch_0A59C4..." -> "MicrosoftEdgeAutoLaunch"
    match name.rsplit_once('_') {
        Some((a, b)) if b.len() >= 16 && b.chars().all(|c| c.is_ascii_hexdigit()) => a.to_string(),
        _ => name.to_string(),
    }
}

fn startup_folder(dir: std::path::PathBuf, root: HKEY, source: &str, can: bool, out: &mut Vec<Entry>) {
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let fname = e.file_name().to_string_lossy().to_string();
        if fname.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let path = e.path().to_string_lossy().to_string();
        let stem = e.path().file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(fname.clone());
        out.push(Entry {
            id: format!("{source}|{fname}"),
            name: stem,
            command: path.clone(),
            exe: Some(path),
            source: source.into(),
            enabled: approved(root, "StartupFolder", &fname),
            can_change: can,
        });
    }
}

pub fn list() -> Vec<Entry> {
    let mut out = Vec::new();
    // Deck normally runs as administrator: then it can switch all-users entries too, like Task Manager
    let admin = crate::elevation::is_admin();
    for (root, path, sub, source, can) in [
        (HKEY_CURRENT_USER, RUN, "Run", "You (registry)", true),
        (HKEY_LOCAL_MACHINE, RUN, "Run", "All users (registry)", admin),
        (HKEY_LOCAL_MACHINE, RUN32, "Run32", "All users (registry)", admin),
    ] {
        for (name, data, _ty) in values(root, path) {
            let command = wide_str(&data);
            let exe = exe_of(&command);
            out.push(Entry {
                id: format!("{source}|{sub}|{name}"),
                name: pretty(&name, exe.as_deref()),
                command,
                exe,
                source: source.into(),
                enabled: approved(root, sub, &name),
                can_change: can,
            });
        }
    }
    startup_folder(crate::util::appdata().join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"), HKEY_CURRENT_USER, "You (Startup folder)", true, &mut out);
    startup_folder(crate::util::program_data().join("Microsoft\\Windows\\Start Menu\\Programs\\StartUp"), HKEY_LOCAL_MACHINE, "All users (Startup folder)", admin, &mut out);
    out.sort_by(|a, b| b.enabled.cmp(&a.enabled).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// Where an entry's on/off switch lives: (for all users, StartupApproved subkey, value name).
fn approval_of(id: &str) -> Option<(bool, &str, &str)> {
    let parts: Vec<&str> = id.splitn(3, '|').collect();
    match parts.as_slice() {
        ["You (registry)", sub @ ("Run" | "Run32"), name] => Some((false, *sub, *name)),
        ["All users (registry)", sub @ ("Run" | "Run32"), name] => Some((true, *sub, *name)),
        ["You (Startup folder)", name] => Some((false, "StartupFolder", *name)),
        ["All users (Startup folder)", name] => Some((true, "StartupFolder", *name)),
        _ => None,
    }
}

/// Turns a startup entry on or off (the same switch Task Manager uses).
pub fn set_enabled(id: &str, on: bool) -> Result<(), String> {
    let (all_users, sub, name) = approval_of(id).ok_or("Unknown startup entry")?;
    if all_users && !crate::elevation::is_admin() {
        return Err("Changing apps that start for all users needs administrator rights".into());
    }
    let root = if all_users { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let mut data = [0u8; 12];
    if on {
        data[0] = 2;
    } else {
        data[0] = 3;
        // FILETIME of when it was disabled (Task Manager shows it)
        let ft = (crate::util::now_ms() * 10_000) + 116_444_736_000_000_000;
        data[4..].copy_from_slice(&ft.to_le_bytes());
    }
    let r = unsafe {
        RegSetKeyValueW(root, &HSTRING::from(format!("{APPROVED}\\{sub}")), &HSTRING::from(name), REG_BINARY.0, Some(data.as_ptr() as *const _), data.len() as u32)
            .ok()
            .map_err(|e| e.to_string())
    };
    match &r {
        Ok(()) => crate::log::info(format!("startup apps: {name} ({}) turned {}", if all_users { "all users" } else { "this user" }, if on { "on" } else { "off" })),
        Err(e) => crate::log::warn(format!("startup apps: {name} couldn't be turned {}: {e}", if on { "on" } else { "off" })),
    }
    r
}

#[cfg(test)]
mod tests {
    use super::approval_of;

    #[test]
    fn each_entry_maps_to_its_own_switch() {
        assert_eq!(approval_of("You (registry)|Run|Discord"), Some((false, "Run", "Discord")));
        assert_eq!(approval_of("All users (registry)|Run32|Steam"), Some((true, "Run32", "Steam")));
        assert_eq!(approval_of("You (Startup folder)|Notes.lnk"), Some((false, "StartupFolder", "Notes.lnk")));
        assert_eq!(approval_of("All users (Startup folder)|Tool.lnk"), Some((true, "StartupFolder", "Tool.lnk")));
        // a name with the separator in it stays whole
        assert_eq!(approval_of("You (registry)|Run|A|B"), Some((false, "Run", "A|B")));
        // anything else is refused rather than guessed
        assert_eq!(approval_of("You (registry)|RunOnce|X"), None);
        assert_eq!(approval_of("garbage"), None);
    }
}
