//! Secrets kept on disk (this PC's PC to PC key, the pairing code, the phone key, the sync key)
//! are sealed with Windows' DPAPI for the signed-in user: a copy of the settings folder (a backup,
//! a synced or roaming profile, a disk read elsewhere) is useless without this Windows account on
//! this PC. Programs running as the same user can still unseal them, as with any app's secrets.

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

/// What a sealed string starts with on disk (anything else is an older, plain value).
const PREFIX: &str = "dpapi:";

fn run(data: &[u8], seal: bool) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if seal {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Some(v)
}

pub fn seal(data: &[u8]) -> Option<Vec<u8>> {
    run(data, true)
}

pub fn unseal(data: &[u8]) -> Option<Vec<u8>> {
    run(data, false)
}

/// A string as it is written to disk: sealed (empty stays empty).
pub fn seal_str(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    match seal(s.as_bytes()) {
        Some(b) => format!("{PREFIX}{}", crate::util::base64(&b)),
        None => {
            crate::log::warn("secret: Windows couldn't seal a secret; it was saved as it is");
            s.to_string()
        }
    }
}

/// A string as read from disk: unsealed, or as it is if it was never sealed (older versions).
/// One that can't be unsealed (another account or PC) reads as empty, so a new one is made.
pub fn unseal_str(s: &str) -> String {
    match s.strip_prefix(PREFIX) {
        None => s.to_string(),
        Some(b) => crate::util::unbase64(b).and_then(|b| unseal(&b)).and_then(|p| String::from_utf8(p).ok()).unwrap_or_default(),
    }
}

/// `#[serde(with = "crate::secret::field")]` on a String field: sealed in the file, plain in memory.
pub mod field {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::seal_str(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(super::unseal_str(&String::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_secret_reads_back_and_is_not_in_the_file() {
        let s = seal_str("hunter2-pairing-CODE");
        assert!(s.starts_with(PREFIX) && !s.contains("hunter2"), "{s}");
        assert_eq!(unseal_str(&s), "hunter2-pairing-CODE");
        // sealing twice gives different text (random salt), both read back
        assert_ne!(seal_str("x"), seal_str("x"));
    }

    #[test]
    fn older_plain_values_still_read_and_damaged_ones_read_as_empty() {
        assert_eq!(unseal_str("PLAIN-OLD-CODE"), "PLAIN-OLD-CODE");
        assert_eq!(unseal_str(""), "");
        assert_eq!(seal_str(""), "");
        assert_eq!(unseal_str("dpapi:AAAA"), "");
        assert_eq!(unseal_str("dpapi:not base64!"), "");
    }

    #[test]
    fn a_field_is_sealed_in_json_and_plain_in_memory() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct C {
            #[serde(with = "super::field")]
            key: String,
        }
        let json = serde_json::to_string(&C { key: "sync-key-123456".into() }).unwrap();
        assert!(!json.contains("sync-key-123456"), "{json}");
        assert_eq!(serde_json::from_str::<C>(&json).unwrap().key, "sync-key-123456");
        assert_eq!(serde_json::from_str::<C>(r#"{"key":"plain-from-2.5"}"#).unwrap().key, "plain-from-2.5");
    }
}
