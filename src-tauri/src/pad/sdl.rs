//! SDL3, loaded at run time. The DLL is embedded in deck.exe (gzipped, `vendor/sdl3`) and written
//! next to deck.exe the first time it's needed; Deck's folder in Program Files is admin-only, so
//! the elevated process loads nothing a user could have swapped. When it can't load, controller
//! input falls back to XInput (`input.rs`).
//!
//! Only what the overlay needs is bound. SDL is told to stay passive: background input on, no
//! "enhanced" Sony reports (that switch can break a game's input until the pad reconnects), no
//! lights or rumble.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::OnceLock;

static DLL_GZ: &[u8] = include_bytes!("../../vendor/sdl3/SDL3.dll.gz");

pub const INIT_GAMEPAD: u32 = 0x0000_2000;

#[allow(non_snake_case)]
pub struct Api {
    pub SetHint: unsafe extern "C" fn(*const c_char, *const c_char) -> bool,
    pub Init: unsafe extern "C" fn(u32) -> bool,
    pub GetError: unsafe extern "C" fn() -> *const c_char,
    pub GetGamepads: unsafe extern "C" fn(*mut c_int) -> *mut u32,
    pub GetJoysticks: unsafe extern "C" fn(*mut c_int) -> *mut u32,
    pub GetJoystickNameForID: unsafe extern "C" fn(u32) -> *const c_char,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub OpenGamepad: unsafe extern "C" fn(u32) -> *mut c_void,
    pub CloseGamepad: unsafe extern "C" fn(*mut c_void),
    pub GamepadConnected: unsafe extern "C" fn(*mut c_void) -> bool,
    pub GetGamepadName: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    pub GetGamepadPath: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    pub GetGamepadVendor: unsafe extern "C" fn(*mut c_void) -> u16,
    pub GetGamepadProduct: unsafe extern "C" fn(*mut c_void) -> u16,
    pub GetGamepadVendorForID: unsafe extern "C" fn(u32) -> u16,
    pub GetGamepadProductForID: unsafe extern "C" fn(u32) -> u16,
    pub GetGamepadButton: unsafe extern "C" fn(*mut c_void, c_int) -> bool,
    pub GetGamepadAxis: unsafe extern "C" fn(*mut c_void, c_int) -> i16,
    pub GetGamepadPowerInfo: unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int,
    pub UpdateGamepads: unsafe extern "C" fn(),
    pub SetGamepadEventsEnabled: unsafe extern "C" fn(bool),
    pub SetJoystickEventsEnabled: unsafe extern "C" fn(bool),
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

/// SDL3.dll next to deck.exe: written from the embedded copy when it's missing or different.
fn ensure_dll() -> Option<std::path::PathBuf> {
    use std::io::Read;
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let path = dir.join("SDL3.dll");
    let mut want = Vec::new();
    flate2::read::GzDecoder::new(DLL_GZ).read_to_end(&mut want).ok()?;
    let same = std::fs::read(&path).map(|have| have == want).unwrap_or(false);
    if !same {
        // a fresh name first, then a rename: a loaded old copy can't block the update
        let tmp = dir.join(format!("SDL3.dll.{}.new", std::process::id()));
        std::fs::write(&tmp, &want).ok()?;
        if std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            // in use by another Deck: fine when it is the same file
            if std::fs::read(&path).map(|h| h != want).unwrap_or(true) {
                return None;
            }
        }
    }
    Some(path)
}

#[allow(clippy::missing_transmute_annotations)] // each target type is the Api field it fills
fn load() -> Option<Api> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32};
    let path = ensure_dll()?;
    let w: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let lib = unsafe { LoadLibraryExW(w.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if lib.is_null() {
        return None;
    }
    macro_rules! f {
        ($name:literal) => {
            unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, _>(GetProcAddress(lib, concat!($name, "\0").as_ptr())?) }
        };
    }
    Some(Api {
        SetHint: f!("SDL_SetHint"),
        Init: f!("SDL_Init"),
        GetError: f!("SDL_GetError"),
        GetGamepads: f!("SDL_GetGamepads"),
        GetJoysticks: f!("SDL_GetJoysticks"),
        GetJoystickNameForID: f!("SDL_GetJoystickNameForID"),
        free: f!("SDL_free"),
        OpenGamepad: f!("SDL_OpenGamepad"),
        CloseGamepad: f!("SDL_CloseGamepad"),
        GamepadConnected: f!("SDL_GamepadConnected"),
        GetGamepadName: f!("SDL_GetGamepadName"),
        GetGamepadPath: f!("SDL_GetGamepadPath"),
        GetGamepadVendor: f!("SDL_GetGamepadVendor"),
        GetGamepadProduct: f!("SDL_GetGamepadProduct"),
        GetGamepadVendorForID: f!("SDL_GetGamepadVendorForID"),
        GetGamepadProductForID: f!("SDL_GetGamepadProductForID"),
        GetGamepadButton: f!("SDL_GetGamepadButton"),
        GetGamepadAxis: f!("SDL_GetGamepadAxis"),
        GetGamepadPowerInfo: f!("SDL_GetGamepadPowerInfo"),
        UpdateGamepads: f!("SDL_UpdateGamepads"),
        SetGamepadEventsEnabled: f!("SDL_SetGamepadEventsEnabled"),
        SetJoystickEventsEnabled: f!("SDL_SetJoystickEventsEnabled"),
    })
}

/// SDL, initialised for game pads on the first call (from the overlay thread). None when it
/// can't load; the reason is logged once.
pub fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        let api = load();
        let Some(api) = api else {
            crate::log::warn("controller: SDL3 could not be loaded; using XInput");
            return None;
        };
        unsafe {
            for (k, v) in [
                (c"SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", c"1"),
                (c"SDL_JOYSTICK_ENHANCED_REPORTS", c"0"),
                (c"SDL_JOYSTICK_HIDAPI_PS4_RUMBLE", c"0"),
                (c"SDL_JOYSTICK_HIDAPI_PS5_RUMBLE", c"0"),
                (c"SDL_JOYSTICK_HIDAPI_PS5_PLAYER_LED", c"0"),
                (c"SDL_JOYSTICK_HIDAPI_JOY_CONS", c"1"),
                (c"SDL_JOYSTICK_THREAD", c"1"),
                (c"SDL_VIDEO_DRIVER", c"dummy"),
                (c"SDL_AUDIO_DRIVER", c"dummy"),
            ] {
                (api.SetHint)(k.as_ptr(), v.as_ptr());
            }
            if !(api.Init)(INIT_GAMEPAD) {
                let e = CStr::from_ptr((api.GetError)()).to_string_lossy().into_owned();
                crate::log::warn(format!("controller: SDL3 failed to start ({e}); using XInput"));
                return None;
            }
            // nobody reads SDL's event queue: state is polled with SDL_UpdateGamepads
            (api.SetGamepadEventsEnabled)(false);
            (api.SetJoystickEventsEnabled)(false);
        }
        Some(api)
    })
    .as_ref()
}

pub fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// Instance ids from an SDL list call, freeing the list.
pub fn ids(api: &Api, list: unsafe extern "C" fn(*mut c_int) -> *mut u32) -> Vec<u32> {
    let mut n: c_int = 0;
    let p = unsafe { list(&mut n) };
    if p.is_null() {
        return vec![];
    }
    let v = unsafe { std::slice::from_raw_parts(p, n.max(0) as usize) }.to_vec();
    unsafe { (api.free)(p as *mut c_void) };
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_dll_unpacks_to_a_pe_file() {
        use std::io::Read;
        let mut v = Vec::new();
        flate2::read::GzDecoder::new(super::DLL_GZ).read_to_end(&mut v).unwrap();
        assert_eq!(&v[..2], b"MZ");
        assert!(v.len() > 1_000_000);
    }
}
