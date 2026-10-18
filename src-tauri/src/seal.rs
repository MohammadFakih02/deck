//! Hardware settings changed outside Deck. Deck runs with admin rights and drives the fans, the
//! GPU and the CPU from settings that live in the user's own folders (`settings.json`, the
//! hardware `.ini` files), which any program of the user can edit. So whenever Deck itself saves
//! them it records a fingerprint where only administrators can write (HKLM); at an admin start, a
//! file whose fingerprint doesn't match was changed by something else, and the hardware waits for
//! the user's "Apply" instead of acting on it.

use sha2::{Digest, Sha256};
use windows_sys::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ};

/// What a settings part looks like compared with what Deck last saved.
#[derive(Debug, PartialEq, Eq)]
pub enum Seal {
    /// as Deck saved it (or Deck has no admin rights: nothing to protect)
    Same,
    /// nothing recorded yet (first admin start with this version): trusted and recorded
    First,
    /// changed since Deck last saved it
    Changed,
}

pub fn fingerprint(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The verdict for `current` against the fingerprint recorded for it.
pub fn verdict(recorded: Option<&str>, current: &[u8]) -> Seal {
    match recorded {
        None => Seal::First,
        Some(r) if r == fingerprint(current) => Seal::Same,
        Some(_) => Seal::Changed,
    }
}

fn key_path() -> Vec<u16> {
    let profile = std::env::var("DECK_PROFILE").ok().filter(|p| !p.is_empty()).map(|p| format!("-{p}")).unwrap_or_default();
    format!("SOFTWARE\\Deck{profile}\\Seals").encode_utf16().chain([0]).collect()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn read(name: &str) -> Option<String> {
    let mut buf = vec![0u16; 80];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key_path().as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut len) };
    (r == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

/// Deck saved `name` itself: its fingerprint becomes the trusted one (admin only; without admin
/// rights Deck can't write there and has nothing to protect).
pub fn record(name: &str, data: &[u8]) {
    if !crate::elevation::is_admin() {
        return;
    }
    let mut k: HKEY = std::ptr::null_mut();
    unsafe {
        if RegCreateKeyExW(HKEY_LOCAL_MACHINE, key_path().as_ptr(), 0, std::ptr::null(), 0, KEY_SET_VALUE, std::ptr::null(), &mut k, std::ptr::null_mut()) != 0 {
            crate::log::warn(format!("seal: can't open the registry key for {name}"));
            return;
        }
        let v = wide(&fingerprint(data));
        RegSetValueExW(k, wide(name).as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
        RegCloseKey(k);
    }
}

/// `name` as it is now (`data`) against what Deck last saved. A first look records it.
pub fn check(name: &str, data: &[u8]) -> Seal {
    if !crate::elevation::is_admin() {
        return Seal::Same;
    }
    let v = verdict(read(name).as_deref(), data);
    if v == Seal::First {
        record(name, data);
    }
    if v == Seal::Changed {
        crate::log::warn(format!("{name} was changed outside Deck"));
    }
    v
}

/// The hardware part of the settings: profiles and what is applied at start.
pub fn hardware_part(s: &crate::settings::Settings) -> Vec<u8> {
    let mut v = serde_json::json!({
        "cpu_profiles": s.cpu_profiles,
        "gpu_profiles": s.gpu_profiles,
        "gpu_startup": s.gpu_startup,
        "boost_cpu": s.boost_cpu,
        "boost_gpu": s.boost_gpu,
    });
    // added later: only once chosen, so fingerprints recorded before they existed still match
    for (k, x) in [("cpu_battery_profile", &s.cpu_battery_profile), ("cpu_plugged_profile", &s.cpu_plugged_profile)] {
        if !x.is_empty() {
            v[k] = serde_json::json!(x);
        }
    }
    serde_json::to_vec(&v).unwrap_or_default()
}

/// The hardware `.ini` files as they are on disk.
fn ini(name: &str) -> Vec<u8> {
    std::fs::read(crate::hw::kv::dir().join(name)).unwrap_or_default()
}

/// Everything sealed, with its current content.
fn parts(s: &crate::settings::Settings) -> [(&'static str, Vec<u8>); 3] {
    [("settings", hardware_part(s)), ("hardware-settings", ini("settings.ini")), ("hardware-curves", ini("curves.ini"))]
}

/// At an admin start: the names of the hardware settings changed outside Deck (empty = all as Deck
/// left them).
pub fn changed_outside(s: &crate::settings::Settings) -> Vec<&'static str> {
    parts(s).into_iter().filter(|(n, d)| check(n, d) == Seal::Changed).map(|(n, _)| n).collect()
}

/// The user looked at them and said Apply: the current files become the trusted ones.
pub fn accept_all(s: &crate::settings::Settings) {
    for (n, d) in parts(s) {
        record(n, &d);
    }
}

/// After Deck wrote one of the `.ini` files.
pub fn record_ini(name: &str) {
    let part = match name {
        "settings.ini" => "hardware-settings",
        "curves.ini" => "hardware-curves",
        _ => return,
    };
    record(part, &ini(name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_made_elsewhere_is_noticed() {
        let saved = br#"{"fan":"40"}"#;
        let fp = fingerprint(saved);
        assert_eq!(verdict(Some(&fp), saved), Seal::Same);
        assert_eq!(verdict(Some(&fp), br#"{"fan":"0"}"#), Seal::Changed);
        assert_eq!(verdict(None, saved), Seal::First, "nothing recorded yet: trusted once");
    }

    #[test]
    fn power_source_profiles_are_sealed_once_chosen() {
        let mut a = crate::settings::Settings::default();
        let before = hardware_part(&a);
        let t = String::from_utf8_lossy(&before).to_string();
        assert!(!t.contains("cpu_battery") && !t.contains("cpu_plugged"), "unset: fingerprints recorded before these settings existed still match");
        a.cpu_battery_profile = "eco".into();
        assert_ne!(hardware_part(&a), before);
        let b = hardware_part(&a);
        a.cpu_plugged_profile = "full".into();
        assert_ne!(hardware_part(&a), b);
    }

    #[test]
    fn only_the_hardware_part_counts() {
        let mut a = crate::settings::Settings::default();
        let before = hardware_part(&a);
        a.theme = "other".into();
        assert_eq!(hardware_part(&a), before, "a theme change is not a hardware change");
        a.gpu_startup = "oc".into();
        assert_ne!(hardware_part(&a), before);
    }
}
