//! Screen colours for the "Screen" (ambient) lighting mode: the desktop scaled down to a strip of columns with GDI.
use crate::hw::fx::Rgb;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

pub const COLS: usize = 32;
const ROWS: i32 = 18;

/// Returns the colour of each screen column (left to right) and the overall average.
pub fn sample() -> Option<(Vec<Rgb>, Rgb)> {
    unsafe {
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, COLS as i32, ROWS);
        let old = SelectObject(mem, bmp as _);
        SetStretchBltMode(mem, HALFTONE);
        let ok = StretchBlt(mem, 0, 0, COLS as i32, ROWS, screen, 0, 0, sw, sh, SRCCOPY) != 0;
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = COLS as i32;
        bi.bmiHeader.biHeight = -ROWS; // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        let mut px = vec![0u8; COLS * ROWS as usize * 4];
        let got = ok && GetDIBits(mem, bmp, 0, ROWS as u32, px.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS) != 0;
        SelectObject(mem, old);
        DeleteObject(bmp as _);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        if !got {
            return None;
        }
        let mut cols = Vec::with_capacity(COLS);
        let (mut ar, mut ag, mut ab) = (0u32, 0u32, 0u32);
        for x in 0..COLS {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for y in 0..ROWS as usize {
                let o = (y * COLS + x) * 4; // BGRA
                b += px[o] as u32;
                g += px[o + 1] as u32;
                r += px[o + 2] as u32;
            }
            let n = ROWS as u32;
            ar += r;
            ag += g;
            ab += b;
            cols.push(vivid(Rgb::new((r / n) as u8, (g / n) as u8, (b / n) as u8)));
        }
        let n = (ROWS as usize * COLS) as u32;
        Some((cols, vivid(Rgb::new((ar / n) as u8, (ag / n) as u8, (ab / n) as u8))))
    }
}

/// Screen averages come out greyish; push saturation up so LEDs show a clear colour.
fn vivid(c: Rgb) -> Rgb {
    let (r, g, b) = (c.r as f32, c.g as f32, c.b as f32);
    let avg = (r + g + b) / 3.0;
    let f = |v: f32| (avg + (v - avg) * 1.8).clamp(0.0, 255.0) as u8;
    Rgb::new(f(r), f(g), f(b))
}
