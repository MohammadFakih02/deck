//! XInput: older Xbox-style pads say only empty / low / medium / full. Used when Windows.Gaming.Input
//! found nothing (it knows the same pads, with a real percent). Wired pads have no battery to show,
//! which also leaves out dongles that always say "wired" and virtual (ViGEm) pads.

use super::{Entry, Reading};
use std::sync::OnceLock;

#[repr(C)]
#[derive(Default)]
struct BatteryInfo {
    kind: u8,
    level: u8,
}

type GetState = unsafe extern "system" fn(u32, *mut [u8; 16]) -> u32;
type GetBattery = unsafe extern "system" fn(u32, u8, *mut BatteryInfo) -> u32;

fn api() -> Option<(GetState, GetBattery)> {
    static API: OnceLock<Option<(usize, usize)>> = OnceLock::new();
    let (s, b) = (*API.get_or_init(|| unsafe {
        use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
        let lib = crate::util::load_trusted_dll("xinput1_4.dll");
        if lib.is_null() {
            return None;
        }
        let s = GetProcAddress(lib as _, c"XInputGetState".as_ptr() as _)?;
        let b = GetProcAddress(lib as _, c"XInputGetBatteryInformation".as_ptr() as _)?;
        Some((s as usize, b as usize))
    }))?;
    unsafe { Some((std::mem::transmute::<usize, GetState>(s), std::mem::transmute::<usize, GetBattery>(b))) }
}

/// BATTERY_TYPE (0 disconnected, 1 wired, 2 alkaline, 3 NiMH, 0xFF unknown) and BATTERY_LEVEL
/// (0 empty … 3 full) as a coarse reading; None when there's nothing to show.
pub fn to_reading(kind: u8, level: u8) -> Option<Reading> {
    if !matches!(kind, 2 | 3) {
        return None;
    }
    let level = ["empty", "low", "medium", "full"].get(level as usize)?;
    Some(Reading { percent: None, level: Some(level), charging: false })
}

/// XINPUT_STATE of a user slot (packet number, then XINPUT_GAMEPAD), or None when nothing is there.
pub fn state(user: u32) -> Option<[u8; 16]> {
    let (get_state, _) = api()?;
    let mut st = [0u8; 16];
    (unsafe { get_state(user, &mut st) } == 0).then_some(st)
}

pub fn poll() -> Vec<Entry> {
    let Some((get_state, get_battery)) = api() else { return vec![] };
    let mut out = Vec::new();
    for user in 0..4u32 {
        let mut st = [0u8; 16];
        if unsafe { get_state(user, &mut st) } != 0 {
            continue;
        }
        let mut info = BatteryInfo::default();
        if unsafe { get_battery(user, 0, &mut info) } != 0 {
            continue;
        }
        if let Some(reading) = to_reading(info.kind, info.level) {
            out.push(Entry {
                id: format!("xinput:{user}"),
                name: format!("Controller {}", user + 1),
                vid: 0,
                pid: 0,
                source: "XInput",
                reading,
                path: String::new(),
                virtual_pad: false,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wired_xinput_skipped() {
        assert_eq!(to_reading(1, 3), None);
        assert_eq!(to_reading(0, 0), None);
        assert_eq!(to_reading(0xFF, 2), None);
        assert_eq!(to_reading(2, 1).unwrap().level, Some("low"));
        assert_eq!(to_reading(3, 3).unwrap().level, Some("full"));
        assert_eq!(to_reading(3, 9), None);
    }
}
