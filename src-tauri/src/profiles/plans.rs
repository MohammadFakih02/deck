//! Windows power plans through powrprof (no powercfg.exe windows): list, switch, copy,
//! rename, delete, and read/write processor settings, including the hidden ones
//! (the per-core-class clock caps).

use windows::core::GUID;
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::System::Power::*;

pub const BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);
pub const HIGH: GUID = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
pub const ULTIMATE: GUID = GUID::from_u128(0xe9a42b02_d5df_448d_aa00_03f14749eb61);
const NO_SUBGROUP: GUID = GUID::from_u128(0xfea3413e_7e05_4911_9a71_700331f1c294);
const PERSONALITY: GUID = GUID::from_u128(0x245d8541_3943_4422_b025_13a784f679b7);
/// Windows 10/11 "power mode" slider positions (overlay schemes)
pub const OVERLAY_BEST: GUID = GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238);
pub const OVERLAY_EFFICIENCY: GUID = GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a);
pub const OVERLAY_BALANCED: GUID = GUID::zeroed();

pub const SUB_PROCESSOR: GUID = GUID::from_u128(0x54533251_82be_4824_96c1_47b60b740d00);
/// maximum processor frequency, MHz (0 = no cap), for core class 0 and 1
pub const FREQ_MAX: [GUID; 2] = [GUID::from_u128(0x75b0ae3f_bce0_45a7_8c89_c9611c25e100), GUID::from_u128(0x75b0ae3f_bce0_45a7_8c89_c9611c25e101)];
/// maximum / minimum processor state, % (minimum: core classes 0, 1 and 2)
pub const STATE_MAX: [GUID; 2] = [GUID::from_u128(0xbc5038f7_23e0_4960_96da_33abaf5935ec), GUID::from_u128(0xbc5038f7_23e0_4960_96da_33abaf5935ed)];
pub const STATE_MIN: [GUID; 3] = [GUID::from_u128(0x893dee8e_2bef_41e0_89c6_b55d0929964c), GUID::from_u128(0x893dee8e_2bef_41e0_89c6_b55d0929964d), GUID::from_u128(0x893dee8e_2bef_41e0_89c6_b55d0929964e)];
/// 1 = never enter idle states (only ever put back to 0)
pub const IDLE_DISABLE: GUID = GUID::from_u128(0x5d76a2ca_e8c0_402f_a133_2158492d58ad);
pub const BOOST_MODE: GUID = GUID::from_u128(0xbe337238_0d82_4146_a960_4f3749d470c7);
/// core parking: minimum cores unparked, %
pub const MIN_CORES: [GUID; 2] = [GUID::from_u128(0x0cc5b647_c1df_4637_891a_dec35c318583), GUID::from_u128(0x0cc5b647_c1df_4637_891a_dec35c318584)];
/// energy performance preference, 0..100
pub const EPP: [GUID; 2] = [GUID::from_u128(0x36687f9e_e3a5_4dbf_b1dc_15eb381c6863), GUID::from_u128(0x36687f9e_e3a5_4dbf_b1dc_15eb381c6864)];

pub fn guid_str(g: &GUID) -> String {
    format!("{g:?}").to_lowercase()
}

pub fn parse(s: &str) -> Option<GUID> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (h.len() == 32).then(|| GUID::from_u128(u128::from_str_radix(&h, 16).unwrap_or(0)))
}

fn from_bytes(b: &[u8; 16]) -> GUID {
    GUID {
        data1: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        data2: u16::from_le_bytes([b[4], b[5]]),
        data3: u16::from_le_bytes([b[6], b[7]]),
        data4: [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
    }
}

pub fn active() -> Option<GUID> {
    unsafe {
        let mut p: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut p).0 != 0 || p.is_null() {
            return None;
        }
        let g = *p;
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(p as *mut _)));
        Some(g)
    }
}

pub fn set_active(g: &GUID) -> Result<(), String> {
    let r = unsafe { PowerSetActiveScheme(None, Some(g)) };
    if r.0 == 0 {
        Ok(())
    } else {
        Err(format!("Windows refused to switch power plans (error {}).", r.0))
    }
}

pub fn name(g: &GUID) -> String {
    unsafe {
        let mut size = 0u32;
        let _ = PowerReadFriendlyName(None, Some(g), None, None, None, &mut size);
        if size == 0 {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize];
        if PowerReadFriendlyName(None, Some(g), None, None, Some(buf.as_mut_ptr()), &mut size).0 != 0 {
            return String::new();
        }
        let w: Vec<u16> = buf.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        crate::util::from_wide(&w)
    }
}

pub fn rename(g: &GUID, name: &str) -> Result<(), String> {
    let w = crate::util::wide(name);
    let bytes: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    let r = unsafe { PowerWriteFriendlyName(None, g, None, None, &bytes) };
    if r.0 == 0 {
        Ok(())
    } else {
        Err(format!("Couldn't rename the power plan (error {}).", r.0))
    }
}


pub fn list() -> Vec<GUID> {
    let mut out = Vec::new();
    for i in 0..64u32 {
        let mut buf = [0u8; 16];
        let mut size = 16u32;
        let r = unsafe { PowerEnumerate(None, None, None, ACCESS_SCHEME, i, Some(buf.as_mut_ptr()), &mut size) };
        if r.0 != 0 {
            break;
        }
        out.push(from_bytes(&buf));
    }
    out
}

/// A copy of `src` (all its settings), renamed.
pub fn duplicate(src: &GUID, new_name: &str) -> Result<GUID, String> {
    unsafe {
        let mut p: *mut GUID = std::ptr::null_mut();
        let r = PowerDuplicateScheme(None, src, &mut p);
        if r.0 != 0 || p.is_null() {
            return Err(format!("Couldn't copy the power plan (error {}).", r.0));
        }
        let g = *p;
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(p as *mut _)));
        rename(&g, new_name)?;
        Ok(g)
    }
}

pub fn delete(g: &GUID) -> Result<(), String> {
    if active() == Some(*g) {
        return Err("The active power plan can't be deleted.".into());
    }
    let r = unsafe { PowerDeleteScheme(None, g) };
    if r.0 == 0 {
        Ok(())
    } else {
        Err(format!("Couldn't delete the power plan (error {}).", r.0))
    }
}

/// (AC, DC) value of a setting in a plan.
pub fn read(g: &GUID, setting: &GUID) -> Option<(u32, u32)> {
    unsafe {
        let mut ac = 0u32;
        let mut dc = 0u32;
        let a = PowerReadACValueIndex(None, Some(g), Some(&SUB_PROCESSOR), Some(setting), &mut ac);
        let d = PowerReadDCValueIndex(None, Some(g), Some(&SUB_PROCESSOR), Some(setting), &mut dc);
        (a.0 == 0 && d == 0).then_some((ac, dc))
    }
}

/// (AC, DC) value of a setting in any subgroup.
pub fn read_in(g: &GUID, sub: &GUID, setting: &GUID) -> Option<(u32, u32)> {
    unsafe {
        let mut ac = 0u32;
        let mut dc = 0u32;
        let a = PowerReadACValueIndex(None, Some(g), Some(sub), Some(setting), &mut ac);
        let d = PowerReadDCValueIndex(None, Some(g), Some(sub), Some(setting), &mut dc);
        (a.0 == 0 && d == 0).then_some((ac, dc))
    }
}

/// Sets a setting for both AC and DC.
pub fn write(g: &GUID, setting: &GUID, v: u32) -> Result<(), String> {
    unsafe {
        let a = PowerWriteACValueIndex(None, g, Some(&SUB_PROCESSOR), Some(setting), v);
        let d = PowerWriteDCValueIndex(None, g, Some(&SUB_PROCESSOR), Some(setting), v);
        if a.0 == 0 && d == 0 {
            Ok(())
        } else {
            Err(format!("Windows refused a power plan setting (error {}).", a.0.max(d)))
        }
    }
}

/// Sets a setting for plugged-in (AC) only.
pub fn write_ac(g: &GUID, setting: &GUID, v: u32) -> Result<(), String> {
    let a = unsafe { PowerWriteACValueIndex(None, g, Some(&SUB_PROCESSOR), Some(setting), v) };
    if a.0 == 0 {
        Ok(())
    } else {
        Err(format!("Windows refused a power plan setting (error {}).", a.0))
    }
}

/// Sets AC and DC to their own values (putting remembered originals back).
pub fn write_pair(g: &GUID, setting: &GUID, ac: u32, dc: u32) -> Result<(), String> {
    unsafe {
        let a = PowerWriteACValueIndex(None, g, Some(&SUB_PROCESSOR), Some(setting), ac);
        let d = PowerWriteDCValueIndex(None, g, Some(&SUB_PROCESSOR), Some(setting), dc);
        if a.0 == 0 && d == 0 {
            Ok(())
        } else {
            Err(format!("Windows refused a power plan setting (error {}).", a.0.max(d)))
        }
    }
}

/// Which built-in plan this one is based on.
pub fn personality(g: &GUID) -> Option<GUID> {
    unsafe {
        let mut buf = [0u8; 16];
        let mut size = 16u32;
        let r = PowerReadACValue(None, Some(g), Some(&NO_SUBGROUP), Some(&PERSONALITY), None, Some(buf.as_mut_ptr()), Some(&mut size));
        (r.0 == 0 && size == 16).then(|| from_bytes(&buf))
    }
}

fn powrprof(name: &str) -> Option<unsafe extern "system" fn() -> isize> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::GetProcAddress;
    unsafe {
        let m = HMODULE(crate::util::load_trusted_dll("powrprof.dll"));
        if m.is_invalid() {
            return None;
        }
        GetProcAddress(m, windows::core::PCSTR(format!("{name}\0").as_ptr()))
    }
}

/// The "power mode" slider (only exists on some PCs, only on Balanced).
pub fn overlay() -> Option<GUID> {
    let f: unsafe extern "system" fn(*mut GUID) -> u32 = unsafe { std::mem::transmute(powrprof("PowerGetEffectiveOverlayScheme")?) };
    let mut g = GUID::zeroed();
    (unsafe { f(&mut g) } == 0).then_some(g)
}

pub fn set_overlay(g: GUID) -> bool {
    let Some(p) = powrprof("PowerSetActiveOverlayScheme") else { return false };
    let f: unsafe extern "system" fn(GUID) -> u32 = unsafe { std::mem::transmute(p) };
    unsafe { f(g) == 0 }
}

/// What Full power would do: switch to this plan, or set this power mode.
pub enum Choice {
    Already(String),
    Scheme(GUID, String),
    Overlay,
    Nothing,
}

/// The fastest plan this PC has (Ultimate, then High performance, then a copy of it), or the
/// power-mode slider on Balanced.
pub fn choose() -> Choice {
    let Some(cur) = active() else { return Choice::Nothing };
    let cur_p = personality(&cur).unwrap_or(cur);
    if cur_p == HIGH || cur == ULTIMATE {
        return Choice::Already(name(&cur));
    }
    let highs: Vec<GUID> = list().into_iter().filter(|g| *g == HIGH || *g == ULTIMATE || personality(g) == Some(HIGH)).collect();
    let pick = highs
        .iter()
        .find(|g| **g == ULTIMATE || name(g).to_lowercase().contains("ultimate"))
        .or_else(|| highs.iter().find(|g| **g == HIGH))
        .or_else(|| highs.first())
        .copied();
    if let Some(g) = pick {
        return Choice::Scheme(g, name(&g));
    }
    if cur_p == BALANCED {
        if let Some(o) = overlay() {
            return if o == OVERLAY_BEST { Choice::Already("Best performance".into()) } else { Choice::Overlay };
        }
    }
    Choice::Nothing
}

/// Changed values of the active plan only take effect after it is activated again.
pub fn reactivate(g: &GUID) -> Result<(), String> {
    if active() == Some(*g) {
        // any other plan for a moment (High performance is missing on many laptops)
        let other = if *g == BALANCED { list().into_iter().find(|x| x != g).unwrap_or(HIGH) } else { BALANCED };
        let _ = set_active(&other);
    }
    set_active(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_text_round_trip() {
        let g = FREQ_MAX[1];
        assert_eq!(guid_str(&g), "75b0ae3f-bce0-45a7-8c89-c9611c25e101");
        assert_eq!(parse("{75B0AE3F-BCE0-45A7-8C89-C9611C25E101}"), Some(g));
        assert_eq!(parse("nope"), None);
    }

    /// Reads this PC's plans (changes nothing).
    #[test]
    #[ignore]
    fn list_plans() {
        let act = active();
        for g in list() {
            println!("{} {} active={} freq={:?} max={:?} boost={:?}", guid_str(&g), name(&g), Some(g) == act, read(&g, &FREQ_MAX[0]), read(&g, &STATE_MAX[0]), read(&g, &BOOST_MODE));
        }
    }
}
