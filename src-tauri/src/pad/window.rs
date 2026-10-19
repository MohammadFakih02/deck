//! A borderless top-level window showing a 32-bit image with per-pixel alpha
//! (UpdateLayeredWindow): smooth edges and soft shadows over the game. It never takes focus
//! (`WS_EX_NOACTIVATE`, `MA_NOACTIVATE`, `SWP_NOACTIVATE` everywhere), is click-through except in
//! Move / resize, and stays out of screen captures unless the user wants it in them.
//!
//! The windows live on the overlay thread; their window procedure only queues mouse events.

use super::raster::Img;
use std::cell::RefCell;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    Down,
    Right,
    Wheel(i16),
}

thread_local! {
    /// mouse events for the overlay window, read by the overlay's tick
    pub static EVENTS: RefCell<Vec<(HWND, Ev)>> = const { RefCell::new(Vec::new()) };
    /// the cursor to show over the overlay (Move / resize), or null
    pub static CURSOR: RefCell<HCURSOR> = const { RefCell::new(null_mut()) };
    /// what WM_INPUT messages go to (keyboard & mouse mode)
    pub static RAW: RefCell<Option<Box<dyn FnMut(isize)>>> = const { RefCell::new(None) };
}

const CLASS: &str = "Deck.PadOverlay";
const INPUT_CLASS: &str = "Deck.PadInput";
const OVERLAY_EX: u32 = WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_LBUTTONDOWN => EVENTS.with(|e| e.borrow_mut().push((hwnd, Ev::Down))),
        WM_RBUTTONUP => EVENTS.with(|e| e.borrow_mut().push((hwnd, Ev::Right))),
        WM_MOUSEWHEEL => EVENTS.with(|e| e.borrow_mut().push((hwnd, Ev::Wheel(((wp >> 16) & 0xFFFF) as u16 as i16)))),
        WM_SETCURSOR => {
            let c = CURSOR.with(|c| *c.borrow());
            if !c.is_null() {
                SetCursor(c);
                return 1;
            }
        }
        WM_INPUT => {
            RAW.with(|r| {
                if let Some(f) = r.borrow_mut().as_mut() {
                    f(lp);
                }
            });
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn register(name: &str) {
    let w = crate::util::wide(name);
    let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: unsafe { GetModuleHandleW(std::ptr::null()) }, lpszClassName: w.as_ptr(), ..unsafe { std::mem::zeroed() } };
    unsafe { RegisterClassW(&wc) };
}

/// The hidden window that receives raw keyboard & mouse input.
pub fn input_window() -> HWND {
    register(INPUT_CLASS);
    let (c, t) = (crate::util::wide(INPUT_CLASS), crate::util::wide("Deck controller overlay input"));
    unsafe { CreateWindowExW(0, c.as_ptr(), t.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null()) }
}

pub struct Layered {
    pub hwnd: HWND,
    screen: HDC,
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub size: (i32, i32),
    pub pos: (i32, i32),
    pub alpha: f32,
    pub shown: bool,
    click_through: bool,
    scratch: Vec<u8>,
}

impl Layered {
    pub fn new(title: &str) -> Option<Layered> {
        register(CLASS);
        let (c, t) = (crate::util::wide(CLASS), crate::util::wide(title));
        let ex = OVERLAY_EX | WS_EX_TOPMOST | WS_EX_TRANSPARENT;
        let hwnd = unsafe { CreateWindowExW(ex, c.as_ptr(), t.as_ptr(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null()) };
        if hwnd.is_null() {
            return None;
        }
        let screen = unsafe { GetDC(null_mut()) };
        let dc = unsafe { CreateCompatibleDC(screen) };
        Some(Layered { hwnd, screen, dc, bmp: null_mut(), old: null_mut(), bits: null_mut(), size: (0, 0), pos: (0, 0), alpha: 0.0, shown: false, click_through: true, scratch: Vec::new() })
    }

    fn resize(&mut self, w: i32, h: i32) -> bool {
        unsafe {
            if !self.bmp.is_null() {
                SelectObject(self.dc, self.old);
                DeleteObject(self.bmp);
                self.bmp = null_mut();
            }
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..std::mem::zeroed() };
            let mut bits: *mut std::ffi::c_void = null_mut();
            self.bmp = CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if self.bmp.is_null() {
                return false;
            }
            self.bits = bits as *mut u8;
            self.old = SelectObject(self.dc, self.bmp);
        }
        self.size = (w, h);
        true
    }

    pub fn set_image(&mut self, img: &Img) {
        let (w, h) = (img.w(), img.h());
        if (w, h) != self.size && !self.resize(w, h) {
            return;
        }
        img.bgra(&mut self.scratch);
        unsafe { std::ptr::copy_nonoverlapping(self.scratch.as_ptr(), self.bits, self.scratch.len().min((w * h * 4) as usize)) };
        self.update();
    }

    pub fn move_to(&mut self, x: i32, y: i32) {
        if (x, y) != self.pos {
            self.pos = (x, y);
            self.update();
        }
    }

    pub fn set_alpha(&mut self, a: f32) {
        self.alpha = a;
        self.update();
    }

    fn update(&mut self) {
        if self.bmp.is_null() {
            return;
        }
        let a = (self.alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        if a == 0 {
            // fully transparent: hide it. Any window over a fullscreen game, even an invisible
            // one, stops the game flipping straight to the screen.
            if self.shown {
                unsafe { ShowWindow(self.hwnd, SW_HIDE) };
                self.shown = false;
            }
            return;
        }
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: a, AlphaFormat: AC_SRC_ALPHA as u8 };
        let pt = POINT { x: self.pos.0, y: self.pos.1 };
        let sz = SIZE { cx: self.size.0, cy: self.size.1 };
        let src = POINT { x: 0, y: 0 };
        unsafe { UpdateLayeredWindow(self.hwnd, self.screen, &pt, &sz, self.dc, &src, 0, &blend, ULW_ALPHA) };
        if !self.shown {
            self.shown = true;
            let fg = unsafe { GetForegroundWindow() };
            unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            self.topmost();
            // anything that slipped through: hand focus back
            if !fg.is_null() && unsafe { GetForegroundWindow() } != fg {
                unsafe { SetForegroundWindow(fg) };
            }
        }
    }

    /// Back to the top of the topmost band without taking focus. A borderless game is topmost
    /// too and the most recently activated topmost window wins, so this repeats every 500 ms.
    pub fn topmost(&self) {
        unsafe { SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW) };
    }

    /// Re-applies the styles if something stripped them (cheap self-heal), and sets click-through.
    pub fn styles(&mut self, click_through: bool) {
        let ex = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
        let ok = ex & OVERLAY_EX == OVERLAY_EX && (ex & WS_EX_TRANSPARENT != 0) == click_through && ex & WS_EX_APPWINDOW == 0;
        if ok && click_through == self.click_through {
            return;
        }
        self.click_through = click_through;
        let mut ex = (ex | OVERLAY_EX) & !WS_EX_APPWINDOW;
        if click_through {
            ex |= WS_EX_TRANSPARENT;
        } else {
            ex &= !WS_EX_TRANSPARENT;
        }
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex as isize);
            SetWindowPos(self.hwnd, null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }

    /// Keep out of screenshots and recordings (Windows 10 2004 and later), or not.
    pub fn exclude_from_capture(&self, on: bool) {
        unsafe { SetWindowDisplayAffinity(self.hwnd, if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE }) };
    }
}

impl Drop for Layered {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            if !self.bmp.is_null() {
                SelectObject(self.dc, self.old);
                DeleteObject(self.bmp);
            }
            DeleteDC(self.dc);
            ReleaseDC(null_mut(), self.screen);
        }
    }
}
