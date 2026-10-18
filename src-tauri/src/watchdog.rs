//! Hardware fail-safe for when Deck is ended from outside. Deck hands the fans back to the BIOS
//! and the GPU fan back to the driver when it quits, panics, crashes or Windows signs out; a process
//! that is terminated (Task Manager's "End process", `taskkill /f`, a fail-fast crash) runs none of
//! that, and a fixed fan duty would stay. So while Deck drives hardware, a second copy of Deck
//! (`--watchdog <pid>`) waits for it to end, and releases the hardware itself unless Deck said
//! goodbye first.

use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateEventW, OpenEventW, OpenProcess, SetEvent, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE, SYNCHRONIZATION_SYNCHRONIZE};

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

/// In the watchdog process: waits for Deck (`pid`) to end. True when it was ended from outside
/// and the hardware needs releasing.
pub fn wait_for(pid: u32) -> bool {
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    let clean = unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, event_name(pid).as_ptr()) };
    let unclean = !process.is_null() && !clean.is_null() && ended_unclean(process, clean);
    unsafe {
        if !process.is_null() {
            CloseHandle(process);
        }
        if !clean.is_null() {
            CloseHandle(clean);
        }
    }
    unclean
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
        assert!(!wait_for(3));
    }
}
