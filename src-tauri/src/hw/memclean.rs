//! "Clear RAM" like MSI Center / RAMMap: trim every process's working set and purge the standby (cache) list.
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows_sys::Win32::Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
const MEMORY_EMPTY_WORKING_SETS: u32 = 2;
const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
const MEMORY_PURGE_STANDBY_LIST: u32 = 4;

#[repr(C)]
#[derive(Default)]
struct MemListInfo {
    zero: usize,
    free: usize,
    modified: usize,
    modified_no_write: usize,
    bad: usize,
    standby_by_priority: [usize; 8],
    repurposed_by_priority: [usize; 8],
    modified_pagefile: usize,
}

#[derive(Clone, Copy, Default)]
pub struct MemStat {
    pub total_gb: f64,
    pub used_gb: f64,    // in use by programs (total - available)
    pub standby_gb: f64, // cached, counted as "available" by Windows
    pub free_gb: f64,    // truly free (free + zeroed pages)
}

type NtQuery = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
type NtSet = unsafe extern "system" fn(u32, *const c_void, u32) -> i32;

fn ntdll(name: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe {
        let lib: Vec<u16> = "ntdll.dll".encode_utf16().chain(Some(0)).collect();
        let h = LoadLibraryW(lib.as_ptr());
        if h.is_null() {
            return None;
        }
        GetProcAddress(h, name.as_ptr())
    }
}

fn enable_privilege(name: &str) -> bool {
    unsafe {
        let mut tok: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok) == 0 {
            return false;
        }
        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        let mut ok = LookupPrivilegeValueW(std::ptr::null(), w.as_ptr(), &mut luid) != 0;
        if ok {
            let mut tp: TOKEN_PRIVILEGES = std::mem::zeroed();
            tp.PrivilegeCount = 1;
            tp.Privileges[0].Luid = luid;
            tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
            ok = AdjustTokenPrivileges(tok, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0;
        }
        CloseHandle(tok);
        ok
    }
}

/// Windows' memory numbers (physical and commit), the one place Deck asks for them.
pub fn status() -> Option<MEMORYSTATUSEX> {
    unsafe {
        let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut ms) != 0).then_some(ms)
    }
}

pub fn stat() -> MemStat {
    unsafe {
        let Some(ms) = status() else { return MemStat::default() };
        let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
        let mut s = MemStat { total_gb: gb(ms.ullTotalPhys), used_gb: gb(ms.ullTotalPhys - ms.ullAvailPhys), ..Default::default() };
        if let Some(f) = ntdll(b"NtQuerySystemInformation\0") {
            let q: NtQuery = std::mem::transmute(f);
            let mut info = MemListInfo::default();
            let mut ret = 0u32;
            if q(SYSTEM_MEMORY_LIST_INFORMATION, &mut info as *mut _ as *mut c_void, std::mem::size_of::<MemListInfo>() as u32, &mut ret) == 0 {
                let page = 4096u64;
                s.standby_gb = gb(info.standby_by_priority.iter().sum::<usize>() as u64 * page);
                s.free_gb = gb((info.free + info.zero) as u64 * page);
            }
        }
        s
    }
}

/// Runs the clean-up and returns a one-line result.
pub fn clear() -> String {
    let before = stat();
    // needs administrator; MoboCtl always runs elevated
    let p1 = enable_privilege("SeProfileSingleProcessPrivilege");
    let p2 = enable_privilege("SeIncreaseQuotaPrivilege");
    let Some(f) = ntdll(b"NtSetSystemInformation\0") else { return "Clear RAM: NtSetSystemInformation not available".into() };
    let set: NtSet = unsafe { std::mem::transmute(f) };
    let mut errs = vec![];
    for (cmd, what) in [(MEMORY_EMPTY_WORKING_SETS, "trim working sets"), (MEMORY_FLUSH_MODIFIED_LIST, "flush modified list"), (MEMORY_PURGE_STANDBY_LIST, "purge standby list")] {
        let r = unsafe { set(SYSTEM_MEMORY_LIST_INFORMATION, &cmd as *const u32 as *const c_void, 4) };
        if r != 0 {
            errs.push(format!("{} failed (0x{:08X})", what, r as u32));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let after = stat();
    let freed_use = (before.used_gb - after.used_gb).max(0.0);
    let freed_cache = (before.standby_gb - after.standby_gb).max(0.0);
    let mut s = format!("Cleared: {:.2} GB taken back from programs, {:.2} GB of cache dropped. Free now {:.1} GB.", freed_use, freed_cache, after.free_gb);
    if !errs.is_empty() {
        s += &format!("  ({}{})", errs.join(", "), if p1 && p2 { "" } else { "; privilege not granted" });
    }
    s
}
