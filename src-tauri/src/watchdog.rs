//! Hardware fail-safe for when Deck is ended from outside. Deck hands the fans back to the BIOS
//! and the GPU fan back to the driver when it quits, panics, crashes or Windows signs out; a process
//! that is terminated (Task Manager's "End process", `taskkill /f`, a fail-fast crash) runs none of
//! that, and a fixed fan duty would stay. So while Deck drives hardware, a second copy of Deck
//! (`--watchdog <pid>`) waits for it to end, and releases the hardware itself unless Deck said
//! goodbye first.

use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateEventW, GetExitCodeProcess, OpenEventW, OpenProcess, SetEvent, WaitForSingleObject, INFINITE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, SYNCHRONIZATION_SYNCHRONIZE};

/// Deck's "ending on my own terms" event (a handle, 0 = no watchdog).
static CLEAN: AtomicUsize = AtomicUsize::new(0);

fn event_name(pid: u32) -> Vec<u16> {
    format!("Local\\Deck-watchdog-{pid}").encode_utf16().chain([0]).collect()
}

/// Starts the watchdog for this process (once). Called when the hardware engine starts.
pub fn start() {
    if CLEAN.load(Ordering::SeqCst) != 0 {
        return;
    }
    let pid = std::process::id();
    // manual reset, not set: set once, by clean_exit
    let ev = unsafe { CreateEventW(std::ptr::null(), 1, 0, event_name(pid).as_ptr()) };
    if ev.is_null() {
        crate::log::warn(format!("watchdog: no event: {}", std::io::Error::last_os_error()));
        return;
    }
    if CLEAN.compare_exchange(0, ev as usize, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        unsafe { CloseHandle(ev) };
        return;
    }
    let started = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(["--watchdog", &pid.to_string()]).creation_flags(crate::util::CREATE_NO_WINDOW).spawn());
    match started {
        Ok(c) => crate::log::info(format!("watchdog {} started", c.id())),
        Err(e) => crate::log::warn(format!("watchdog didn't start: {e}")),
    }
}

/// Deck is ending on its own terms and has released the hardware: the watchdog stands down.
pub fn clean_exit() {
    let ev = CLEAN.load(Ordering::SeqCst);
    if ev != 0 {
        unsafe { SetEvent(ev as HANDLE) };
    }
}

/// Waits for `process` to end; true when it ended without `clean` being set first.
fn ended_unclean(process: HANDLE, clean: HANDLE) -> bool {
    unsafe { WaitForSingleObject(process, INFINITE) };
    let said_goodbye = unsafe { WaitForSingleObject(clean, 0) } == WAIT_OBJECT_0;
    !said_goodbye
}

/// How the watched Deck ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Deck said goodbye (the hardware is released).
    Clean,
    /// Ended from outside; the hardware needs releasing. `code` is the process's exit code.
    Unclean { code: Option<u32> },
    /// There was nothing to watch (no such process or event).
    NothingToWatch,
}

/// The exit code of an ended process.
fn exit_code(process: HANDLE) -> Option<u32> {
    let mut c = 0u32;
    (unsafe { GetExitCodeProcess(process, &mut c) } != 0).then_some(c)
}

/// Plain words for an exit code, for the log: a crash names its kind, a plain 1 or a Ctrl-C code
/// means "ended by something else" (Task Manager, `taskkill /f`, an installer).
pub fn describe_exit(code: Option<u32>) -> String {
    match code {
        None => "exit code unknown".into(),
        Some(0xC000_0005) => "crash: access violation (0xC0000005)".into(),
        Some(0xC000_0409) => "crash: fail-fast (0xC0000409)".into(),
        Some(0xC000_0374) => "crash: heap corruption (0xC0000374)".into(),
        Some(0xC000_00FD) => "crash: stack overflow (0xC00000FD)".into(),
        Some(0x4000_0015) => "crash: abort (0x40000015)".into(),
        Some(1) | Some(0xC000_013A) => "ended by another program (terminated)".into(),
        Some(0) => "exited with code 0, without saying goodbye".into(),
        Some(c) => format!("exit code {c:#X}"),
    }
}

/// The module (exe or DLL file name) that holds `addr`, for a crash report; "?" when unknown.
pub fn module_at(addr: *const std::ffi::c_void) -> String {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut module = std::ptr::null_mut();
    let mut buf = [0u16; 260];
    unsafe {
        if GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, addr as *const u16, &mut module) == 0 {
            return "?".into();
        }
        let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
        if n == 0 {
            return "?".into();
        }
        let path = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        path.rsplit('\\').next().unwrap_or("?").to_string()
    }
}

/// In the watchdog process: waits for Deck (`pid`) to end and says how.
pub fn wait_for(pid: u32) -> Outcome {
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    let clean = unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, event_name(pid).as_ptr()) };
    let outcome = if process.is_null() || clean.is_null() {
        Outcome::NothingToWatch
    } else if ended_unclean(process, clean) {
        Outcome::Unclean { code: exit_code(process) }
    } else {
        Outcome::Clean
    };
    unsafe {
        if !process.is_null() {
            CloseHandle(process);
        }
        if !clean.is_null() {
            CloseHandle(clean);
        }
    }
    outcome
}

/// In a helper process (`--start-after <pid>`): waits (at most 2 minutes) for `pid` to end. False
/// when it is still running after that.
pub fn wait_for_exit(pid: u32) -> bool {
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        // already gone (or never was)
        return true;
    }
    unsafe {
        let r = WaitForSingleObject(process, 120_000);
        CloseHandle(process);
        r == WAIT_OBJECT_0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;

    fn quick_process() -> std::process::Child {
        std::process::Command::new("cmd.exe").args(["/c", "exit", "0"]).creation_flags(crate::util::CREATE_NO_WINDOW).spawn().unwrap()
    }

    #[test]
    fn a_process_ended_from_outside_needs_releasing() {
        let ev = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        let child = quick_process();
        assert!(ended_unclean(child.as_raw_handle() as HANDLE, ev), "no goodbye: release");
        unsafe { CloseHandle(ev) };
    }

    #[test]
    fn a_clean_exit_stands_the_watchdog_down() {
        let ev = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        unsafe { SetEvent(ev) };
        let child = quick_process();
        assert!(!ended_unclean(child.as_raw_handle() as HANDLE, ev), "said goodbye: nothing to do");
        unsafe { CloseHandle(ev) };
    }

    #[test]
    fn nothing_to_watch_does_nothing() {
        // no such process (pids are multiples of 4): the watchdog has nothing to release for
        assert_eq!(wait_for(3), Outcome::NothingToWatch);
    }

    #[test]
    fn a_crash_address_names_its_module() {
        // an address inside this very program
        let here = a_crash_address_names_its_module as *const std::ffi::c_void;
        let name = module_at(here).to_lowercase();
        assert!(name.ends_with(".exe"), "{name}");
        assert_eq!(module_at(std::ptr::null()), "?");
    }

    #[test]
    fn exit_codes_are_named() {
        assert!(describe_exit(Some(0xC000_0005)).contains("access violation"));
        assert!(describe_exit(Some(1)).contains("another program"));
        assert!(describe_exit(None).contains("unknown"));
        assert_eq!(describe_exit(Some(7)), "exit code 0x7");
    }

    #[test]
    fn the_exit_code_of_an_ended_process_is_read() {
        let mut child = std::process::Command::new("cmd.exe").args(["/c", "exit", "7"]).creation_flags(crate::util::CREATE_NO_WINDOW).spawn().unwrap();
        child.wait().unwrap();
        // std keeps the handle open after wait: the code is still there to read
        assert_eq!(exit_code(child.as_raw_handle() as HANDLE), Some(7));
    }
}
