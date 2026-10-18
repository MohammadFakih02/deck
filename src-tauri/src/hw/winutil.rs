//! Notifications from the hardware threads (heat alerts) go to Deck's toasts.
use std::sync::OnceLock;

type Notify = Box<dyn Fn(&str, &str) + Send + Sync>;
static NOTIFY: OnceLock<Notify> = OnceLock::new();

/// Set once by Deck at start-up.
pub fn set_notify(f: Notify) {
    let _ = NOTIFY.set(f);
}

pub fn notify(title: &str, text: &str) {
    crate::log::info(format!("hardware: {title}: {text}"));
    if let Some(f) = NOTIFY.get() {
        f(title, text);
    }
}

// ---------------------------------------------------------------- session end (shutdown / restart / sign-out)
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

static SESSION_END: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

unsafe extern "system" fn session_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_ENDSESSION && wp != 0 {
        if let Some(f) = SESSION_END.get() {
            f();
        }
        return 0;
    }
    DefWindowProcW(h, msg, wp, lp)
}

/// Runs `f` when Windows ends the session (hidden top-level window: message-only windows
/// don't get WM_ENDSESSION), so the fans are handed back before the process is killed.
pub fn on_session_end(f: Box<dyn Fn() + Send + Sync>) {
    if SESSION_END.set(f).is_err() {
        return;
    }
    std::thread::spawn(|| unsafe {
        let cls: Vec<u16> = "DeckSessionWatch".encode_utf16().chain(Some(0)).collect();
        let hinst = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(session_proc);
        wc.hInstance = hinst;
        wc.lpszClassName = cls.as_ptr();
        RegisterClassW(&wc);
        let h = CreateWindowExW(0, cls.as_ptr(), cls.as_ptr(), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        if h.is_null() {
            return;
        }
        let mut m: MSG = std::mem::zeroed();
        while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    });
}
