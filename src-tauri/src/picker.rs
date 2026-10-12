//! Screen tools that need you to point at something: colour picker, copy
//! text from the screen (Windows' built-in OCR) and a pixel ruler.
//!
//! The screen is frozen into memory when the tool starts (nothing is saved to
//! disk), shown in a fullscreen window, and released when the tool closes.

use std::cell::RefCell;
use windows::core::PCWSTR;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetCapture, ReleaseCapture, SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Color,
    Text,
    Measure,
}

#[derive(Debug)]
pub enum Outcome {
    Color(u8, u8, u8),
    Text(String),
    Measure(i32, i32),
    Cancelled,
    Failed(String),
}

struct State {
    mode: Mode,
    origin: (i32, i32),
    w: i32,
    h: i32,
    /// top-down BGRA of the frozen screen
    bits: Vec<u8>,
    bright: HDC,
    dark: HDC,
    bmps: Vec<HBITMAP>,
    drag_from: Option<(i32, i32)>,
    cur: (i32, i32),
    result: Option<Outcome>,
    font: HFONT,
    font_small: HFONT,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

unsafe fn dib(dc: HDC, w: i32, h: i32, px: &[u8]) -> (HDC, HBITMAP) {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut ptr = std::ptr::null_mut();
    let bmp = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut ptr, None, 0).unwrap_or_default();
    if !ptr.is_null() {
        std::ptr::copy_nonoverlapping(px.as_ptr(), ptr as *mut u8, px.len().min((w * h * 4) as usize));
    }
    let mdc = CreateCompatibleDC(Some(dc));
    SelectObject(mdc, HGDIOBJ(bmp.0));
    (mdc, bmp)
}

/// Captures the whole virtual screen as top-down BGRA.
unsafe fn grab() -> Option<(i32, i32, i32, i32, Vec<u8>)> {
    let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    if w <= 0 || h <= 0 {
        return None;
    }
    let sdc = GetDC(None);
    let mdc = CreateCompatibleDC(Some(sdc));
    let bmp = CreateCompatibleBitmap(sdc, w, h);
    let old = SelectObject(mdc, HGDIOBJ(bmp.0));
    let _ = BitBlt(mdc, 0, 0, w, h, Some(sdc), x, y, SRCCOPY | CAPTUREBLT);
    SelectObject(mdc, old);
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let lines = GetDIBits(mdc, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mdc);
    ReleaseDC(None, sdc);
    (lines > 0).then_some((x, y, w, h, px))
}

fn norm(a: (i32, i32), b: (i32, i32)) -> RECT {
    RECT { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0) + 1, bottom: a.1.max(b.1) + 1 }
}

unsafe fn text_at(dc: HDC, font: HFONT, s: &str, r: &mut RECT, flags: DRAW_TEXT_FORMAT) {
    let old = SelectObject(dc, HGDIOBJ(font.0));
    let mut w: Vec<u16> = s.encode_utf16().collect();
    DrawTextW(dc, &mut w, r, flags);
    SelectObject(dc, old);
}

unsafe fn pill(dc: HDC, font: HFONT, s: &str, cx: i32, top: i32) {
    let mut m = RECT::default();
    text_at(dc, font, s, &mut m, DT_CALCRECT | DT_SINGLELINE);
    let (tw, th) = (m.right - m.left, m.bottom - m.top);
    let r = RECT { left: cx - tw / 2 - 16, top, right: cx + tw / 2 + 16, bottom: top + th + 14 };
    let br = CreateSolidBrush(rgb(18, 20, 28));
    let pen = CreatePen(PS_SOLID, 1, rgb(70, 80, 110));
    let ob = SelectObject(dc, HGDIOBJ(br.0));
    let op = SelectObject(dc, HGDIOBJ(pen.0));
    let _ = RoundRect(dc, r.left, r.top, r.right, r.bottom, 14, 14);
    SelectObject(dc, ob);
    SelectObject(dc, op);
    let _ = DeleteObject(HGDIOBJ(br.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, rgb(235, 238, 245));
    let mut tr = r;
    text_at(dc, font, s, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

unsafe fn paint(hwnd: HWND, st: &State) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    // back buffer
    let buf = CreateCompatibleDC(Some(hdc));
    let bb = CreateCompatibleBitmap(hdc, st.w, st.h);
    let old = SelectObject(buf, HGDIOBJ(bb.0));
    let (cx, cy) = st.cur;
    match st.mode {
        Mode::Color => {
            let _ = BitBlt(buf, 0, 0, st.w, st.h, Some(st.bright), 0, 0, SRCCOPY);
            // loupe: 15x15 pixels around the cursor at 10x
            let n = 15;
            let z = 10;
            let size = n * z;
            let mut lx = cx + 24;
            let mut ly = cy + 24;
            if lx + size + 4 > st.w {
                lx = cx - 24 - size;
            }
            if ly + size + 50 > st.h {
                ly = cy - 24 - size - 46;
            }
            SetStretchBltMode(buf, COLORONCOLOR);
            let _ = StretchBlt(buf, lx, ly, size, size, Some(st.bright), cx - n / 2, cy - n / 2, n, n, SRCCOPY);
            let frame = CreateSolidBrush(rgb(255, 255, 255));
            let r = RECT { left: lx - 2, top: ly - 2, right: lx + size + 2, bottom: ly + size + 2 };
            FrameRect(buf, &r, frame);
            let _ = DeleteObject(HGDIOBJ(frame.0));
            // center pixel box
            let c = CreateSolidBrush(rgb(0, 0, 0));
            let pr = RECT { left: lx + (n / 2) * z - 1, top: ly + (n / 2) * z - 1, right: lx + (n / 2 + 1) * z + 1, bottom: ly + (n / 2 + 1) * z + 1 };
            FrameRect(buf, &pr, c);
            let _ = DeleteObject(HGDIOBJ(c.0));
            let (r8, g8, b8) = pixel(st, cx, cy);
            let hex = format!("#{r8:02X}{g8:02X}{b8:02X}   rgb({r8}, {g8}, {b8})");
            pill(buf, st.font_small, &hex, lx + size / 2, ly + size + 8);
            pill(buf, st.font, "Click to copy a colour  ·  Right-click or Esc to cancel", st.w / 2, 28);
        }
        Mode::Text | Mode::Measure => {
            let _ = BitBlt(buf, 0, 0, st.w, st.h, Some(st.dark), 0, 0, SRCCOPY);
            if let Some(from) = st.drag_from {
                let r = norm(from, st.cur);
                let _ = BitBlt(buf, r.left, r.top, r.right - r.left, r.bottom - r.top, Some(st.bright), r.left, r.top, SRCCOPY);
                let br = CreateSolidBrush(rgb(79, 140, 255));
                FrameRect(buf, &r, br);
                let r2 = RECT { left: r.left - 1, top: r.top - 1, right: r.right + 1, bottom: r.bottom + 1 };
                FrameRect(buf, &r2, br);
                let _ = DeleteObject(HGDIOBJ(br.0));
                let label = format!("{} × {}", r.right - r.left, r.bottom - r.top);
                let ly = if r.bottom + 44 < st.h { r.bottom + 8 } else { r.top - 40 };
                pill(buf, st.font_small, &label, (r.left + r.right) / 2, ly);
            } else {
                // crosshair
                let pen = CreatePen(PS_DOT, 1, rgb(160, 180, 220));
                let op = SelectObject(buf, HGDIOBJ(pen.0));
                SetBkMode(buf, TRANSPARENT);
                let _ = MoveToEx(buf, 0, cy, None);
                let _ = LineTo(buf, st.w, cy);
                let _ = MoveToEx(buf, cx, 0, None);
                let _ = LineTo(buf, cx, st.h);
                SelectObject(buf, op);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            let hint = if st.mode == Mode::Text {
                "Drag over the text you want to copy  ·  Right-click or Esc to cancel"
            } else {
                "Drag to measure  ·  Right-click or Esc to cancel"
            };
            pill(buf, st.font, hint, st.w / 2, 28);
        }
    }
    let _ = BitBlt(hdc, 0, 0, st.w, st.h, Some(buf), 0, 0, SRCCOPY);
    SelectObject(buf, old);
    let _ = DeleteObject(HGDIOBJ(bb.0));
    let _ = DeleteDC(buf);
    let _ = EndPaint(hwnd, &ps);
}

fn pixel(st: &State, x: i32, y: i32) -> (u8, u8, u8) {
    let x = x.clamp(0, st.w - 1);
    let y = y.clamp(0, st.h - 1);
    let i = ((y * st.w + x) * 4) as usize;
    (st.bits[i + 2], st.bits[i + 1], st.bits[i])
}

fn finish(hwnd: HWND, r: Outcome) {
    ST.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            if st.result.is_none() {
                st.result = Some(r);
            }
        }
    });
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let x = (l.0 & 0xffff) as i16 as i32;
    let y = ((l.0 >> 16) & 0xffff) as i16 as i32;
    match msg {
        WM_PAINT => {
            ST.with(|s| {
                if let Some(st) = s.borrow().as_ref() {
                    paint(hwnd, st);
                }
            });
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SETCURSOR => {
            if let Ok(c) = LoadCursorW(None, IDC_CROSS) {
                SetCursor(Some(c));
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            ST.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    st.cur = (x, y);
                }
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let mut done = None;
            ST.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    if st.mode == Mode::Color {
                        let (r, g, b) = pixel(st, x, y);
                        done = Some(Outcome::Color(r, g, b));
                    } else {
                        st.drag_from = Some((x, y));
                        st.cur = (x, y);
                    }
                }
            });
            if let Some(r) = done {
                finish(hwnd, r);
            } else {
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let mut sel = None;
            ST.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    if let Some(from) = st.drag_from.take() {
                        let r = norm(from, (x, y));
                        if r.right - r.left >= 4 && r.bottom - r.top >= 4 {
                            sel = Some((st.mode, r));
                        }
                    }
                }
            });
            match sel {
                Some((Mode::Measure, r)) => finish(hwnd, Outcome::Measure(r.right - r.left, r.bottom - r.top)),
                Some((_, r)) => {
                    let text = ST.with(|s| s.borrow().as_ref().map(|st| ocr(st, r)));
                    finish(hwnd, text.unwrap_or(Outcome::Cancelled));
                }
                None => {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            finish(hwnd, Outcome::Cancelled);
            LRESULT(0)
        }
        WM_KEYDOWN if w.0 == VK_ESCAPE.0 as usize => {
            finish(hwnd, Outcome::Cancelled);
            LRESULT(0)
        }
        WM_ACTIVATE if (w.0 & 0xffff) == 0 => {
            // lost focus (e.g. Alt+Tab): give up
            finish(hwnd, Outcome::Cancelled);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

/// Recognises the text in `r` with Windows' OCR (the user's languages).
fn ocr(st: &State, r: RECT) -> Outcome {
    // stay inside the captured screen
    let r = RECT { left: r.left.clamp(0, st.w), top: r.top.clamp(0, st.h), right: r.right.clamp(0, st.w), bottom: r.bottom.clamp(0, st.h) };
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w < 1 || h < 1 {
        return Outcome::Cancelled;
    }
    // Small text reads much better scaled up, but Windows' OCR refuses
    // images past a size limit (a wide strip across two screens, say).
    let max = OcrEngine::MaxImageDimension().map(|m| m as i32).unwrap_or(10_000);
    if w > max || h > max {
        return Outcome::Failed("That box is too big to read; drag a smaller one".into());
    }
    let mut scale = if h < 40 { 3 } else if h < 120 { 2 } else { 1 };
    while scale > 1 && (w * scale > max || h * scale > max) {
        scale -= 1;
    }
    let (ow, oh) = (w * scale, h * scale);
    let mut px = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh {
        for x in 0..ow {
            let sx = r.left + x / scale;
            let sy = r.top + y / scale;
            let si = ((sy * st.w + sx) * 4) as usize;
            let di = ((y * ow + x) * 4) as usize;
            px[di..di + 3].copy_from_slice(&st.bits[si..si + 3]);
            px[di + 3] = 255;
        }
    }
    let run = || -> windows::core::Result<String> {
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let writer = DataWriter::new()?;
        writer.WriteBytes(&px)?;
        let buffer = writer.DetachBuffer()?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, ow, oh)?;
        let result = engine.RecognizeAsync(&bmp)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines.join("\n"))
    };
    match run() {
        Ok(t) if !t.trim().is_empty() => Outcome::Text(t),
        Ok(_) => Outcome::Failed("No text found there".into()),
        Err(e) => Outcome::Failed(format!("Text recognition failed: {e}")),
    }
}

/// Runs a screen tool and blocks until it finishes. Call from a worker thread.
pub fn run(mode: Mode) -> Outcome {
    crate::util::com();
    unsafe {
        let Some((ox, oy, w, h, bits)) = grab() else { return Outcome::Failed("Couldn't read the screen".into()) };
        let sdc = GetDC(None);
        let (bright, b1) = dib(sdc, w, h, &bits);
        let darkpx: Vec<u8> = bits.iter().enumerate().map(|(i, v)| if i % 4 == 3 { 255 } else { (*v as u32 * 45 / 100) as u8 }).collect();
        let (dark, b2) = dib(sdc, w, h, &darkpx);
        drop(darkpx);
        ReleaseDC(None, sdc);
        let face = crate::util::wide("Segoe UI Semibold");
        let font = CreateFontW(-20, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, PCWSTR(face.as_ptr()));
        let font_small = CreateFontW(-17, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, PCWSTR(face.as_ptr()));
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        ST.with(|s| {
            *s.borrow_mut() = Some(State {
                mode,
                origin: (ox, oy),
                w,
                h,
                bits,
                bright,
                dark,
                bmps: vec![b1, b2],
                drag_from: None,
                cur: (p.x - ox, p.y - oy),
                result: None,
                font,
                font_small,
            })
        });
        let hinst: HINSTANCE = GetModuleHandleW(None).map(|m| m.into()).unwrap_or_default();
        let class = crate::util::wide("DeckScreenTool");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst, lpszClassName: PCWSTR(class.as_ptr()), ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            ox,
            oy,
            w,
            h,
            None,
            None,
            Some(hinst),
            None,
        );
        let Ok(hwnd) = hwnd else {
            cleanup();
            return Outcome::Failed("Couldn't open the screen tool".into());
        };
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        let r = ST.with(|s| s.borrow_mut().as_mut().and_then(|st| st.result.take()));
        cleanup();
        r.unwrap_or(Outcome::Cancelled)
    }
}

unsafe fn cleanup() {
    ST.with(|s| {
        if let Some(st) = s.borrow_mut().take() {
            let _ = DeleteDC(st.bright);
            let _ = DeleteDC(st.dark);
            for b in st.bmps {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
            let _ = DeleteObject(HGDIOBJ(st.font.0));
            let _ = DeleteObject(HGDIOBJ(st.font_small.0));
            let _ = st.origin;
        }
    });
}
