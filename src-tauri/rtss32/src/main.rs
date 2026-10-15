//! `rtss-osd32 on|off <RivaTuner folder>`: turns RivaTuner's "Show On-Screen Display" switch on
//! or off, the way Afterburner's overlay key does.
//!
//! The switch lives in RivaTuner's 32-bit hooks (RTSSHooks.dll). A 64-bit program can't load that
//! DLL, and the 64-bit hooks keep their own copy that RivaTuner's window never reads, so Deck runs
//! this tiny 32-bit helper. Same calls as the SDK's HotkeyHandler plugin: SetFlags, then
//! WM_RTSS_UPDATESETTINGS so RivaTuner picks the change up.
//!
//! `rtss-osd32 status <folder>` only reads it. A third argument picks another switch by its flag
//! bit: 4 is RTSSHOOKSFLAG_LIMITER_DISABLED (the framerate limiter is off while it is set).
//!
//! Exit code: 0 already as asked, 1 changed, 10 / 11 status off / on, 2 bad arguments,
//! 3 RTSSHooks.dll missing or too old.
#![windows_subsystem = "windows"]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::exit;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> isize;
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn PostMessageW(wnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
}

/// RTSSHOOKSFLAG_OSD_VISIBLE in the SDK.
const OSD_VISIBLE: u32 = 1;
/// WM_APP + 100: makes RivaTuner re-read its settings and update its window.
const WM_RTSS_UPDATESETTINGS: u32 = 0x8000 + 100;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let (on, status) = match args.next().as_deref().and_then(OsStr::to_str) {
        Some("on") => (true, false),
        Some("off") => (false, false),
        Some("status") => (false, true),
        _ => exit(2),
    };
    let Some(dir) = args.next().map(PathBuf::from) else { exit(2) };
    let flag: u32 = match args.next() {
        Some(f) => f.to_str().and_then(|s| s.parse().ok()).filter(|f| [1, 4].contains(f)).unwrap_or_else(|| exit(2)),
        None => OSD_VISIBLE,
    };
    unsafe {
        let m = LoadLibraryW(wide(dir.join("RTSSHooks.dll").as_os_str()).as_ptr());
        if m == 0 {
            exit(3);
        }
        let (get, set) = (GetProcAddress(m, c"GetFlags".as_ptr().cast()), GetProcAddress(m, c"SetFlags".as_ptr().cast()));
        if get == 0 || set == 0 {
            exit(3);
        }
        let get: extern "C" fn() -> u32 = std::mem::transmute(get);
        let set: extern "C" fn(u32, u32) -> u32 = std::mem::transmute(set);
        if status {
            exit(10 + (get() & flag != 0) as i32);
        }
        if (get() & flag != 0) == on {
            exit(0);
        }
        set(!flag, if on { flag } else { 0 });
        let mut wnd = FindWindowW(std::ptr::null(), wide(OsStr::new("RTSS")).as_ptr());
        if wnd == 0 {
            wnd = FindWindowW(std::ptr::null(), wide(OsStr::new("RivaTuner Statistics Server")).as_ptr());
        }
        if wnd != 0 {
            PostMessageW(wnd, WM_RTSS_UPDATESETTINGS, 0, 0);
        }
    }
    exit(1);
}
