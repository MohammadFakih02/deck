//! Screen colours for the "Screen" (ambient) and "Game" lighting modes: the desktop reduced to a strip of columns.
//!
//! Normally read with DXGI desktop duplication: the GPU shrinks the frame through a mip chain and only a
//! ~60x34 image is copied back, and a screen that has not changed costs nothing at all (no new frame).
//! A GDI read of the screen instead costs ~20 ms of CPU and a DWM readback every time, whatever its size -
//! 20 times a second while a game runs. GDI stays as the fallback (duplication unavailable: UAC prompt,
//! exclusive fullscreen mode switches, an HDR desktop format).
use crate::hw::fx::Rgb;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

pub const COLS: usize = 32;
const ROWS: i32 = 18;

/// Keeps the duplication session between samples (setting one up takes a few ms).
pub struct Grabber {
    dup: Option<Dup>,
    retry_at: Instant,
    last: Option<(Vec<Rgb>, Rgb)>,
}

impl Grabber {
    pub fn new() -> Self {
        Grabber { dup: None, retry_at: Instant::now(), last: None }
    }

    /// The colour of each screen column (left to right) and the overall average.
    pub fn sample(&mut self) -> Option<(Vec<Rgb>, Rgb)> {
        if self.dup.is_none() && Instant::now() >= self.retry_at {
            self.dup = Dup::new().ok();
            if self.dup.is_none() {
                self.retry_at = Instant::now() + Duration::from_secs(3);
            }
        }
        if let Some(d) = self.dup.as_mut() {
            match d.frame() {
                Ok(Some(px)) => {
                    self.last = Some(px);
                    return self.last.clone();
                }
                Ok(None) if self.last.is_some() => return self.last.clone(), // nothing new on screen
                Ok(None) => {}
                Err(_) => {
                    // access lost (mode change, secure desktop) or unsupported format: GDI for now, retry soon
                    self.dup = None;
                    self.retry_at = Instant::now() + Duration::from_secs(1);
                }
            }
        }
        self.last = gdi_sample();
        self.last.clone()
    }
}

struct Dup {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    /// created on the first frame, sized to it: (mip chain, its view, the level read back, staging copy of that level)
    tex: Option<(ID3D11Texture2D, ID3D11ShaderResourceView, u32, ID3D11Texture2D, u32, u32)>,
}

impl Dup {
    fn new() -> windows::core::Result<Self> {
        unsafe {
            let mut dev = None;
            let mut ctx = None;
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))?;
            let (dev, ctx): (ID3D11Device, ID3D11DeviceContext) = (dev.unwrap(), ctx.unwrap());
            let adapter = dev.cast::<IDXGIDevice>()?.GetAdapter()?;
            // the primary monitor (desktop origin 0,0), like the GDI path
            let mut i = 0;
            let output = loop {
                let o = adapter.EnumOutputs(i)?;
                let d = o.GetDesc()?;
                if d.DesktopCoordinates.left == 0 && d.DesktopCoordinates.top == 0 {
                    break o;
                }
                i += 1;
            };
            let dup = output.cast::<IDXGIOutput1>()?.DuplicateOutput(&dev)?;
            Ok(Dup { dev, ctx, dup, tex: None })
        }
    }

    /// Ok(None) = the screen has not changed since the last frame.
    fn frame(&mut self) -> windows::core::Result<Option<(Vec<Rgb>, Rgb)>> {
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            if let Err(e) = self.dup.AcquireNextFrame(0, &mut info, &mut res) {
                return if e.code() == DXGI_ERROR_WAIT_TIMEOUT { Ok(None) } else { Err(e) };
            }
            let got = (|| -> windows::core::Result<bool> {
                if info.LastPresentTime == 0 {
                    return Ok(false); // only the mouse pointer moved
                }
                let src: ID3D11Texture2D = res.as_ref().unwrap().cast()?;
                if self.tex.is_none() {
                    self.tex = Some(self.make_textures(&src)?);
                }
                let (mip, view, ..) = self.tex.as_ref().unwrap();
                self.ctx.CopySubresourceRegion(mip, 0, 0, 0, 0, &src, 0, None);
                self.ctx.GenerateMips(view);
                Ok(true)
            })();
            let _ = self.dup.ReleaseFrame();
            if !got? {
                return Ok(None);
            }
            let (mip, _, level, staging, w, h) = self.tex.as_ref().unwrap();
            self.ctx.CopySubresourceRegion(staging, 0, 0, 0, 0, mip, *level, None);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let px = std::slice::from_raw_parts(map.pData as *const u8, map.RowPitch as usize * *h as usize);
            let out = reduce(px, *w as usize, *h as usize, map.RowPitch as usize);
            self.ctx.Unmap(staging, 0);
            Ok(Some(out))
        }
    }

    fn make_textures(&self, src: &ID3D11Texture2D) -> windows::core::Result<(ID3D11Texture2D, ID3D11ShaderResourceView, u32, ID3D11Texture2D, u32, u32)> {
        unsafe {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            src.GetDesc(&mut d);
            if d.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                return Err(windows::core::Error::from(DXGI_ERROR_UNSUPPORTED)); // e.g. an HDR desktop: GDI handles it
            }
            // smallest mip level still at least twice the LED strip's resolution
            let mut level = 0;
            while (d.Width >> (level + 1)) >= COLS as u32 * 2 && (d.Height >> (level + 1)) >= ROWS as u32 * 2 {
                level += 1;
            }
            let mip_desc = D3D11_TEXTURE2D_DESC {
                Width: d.Width,
                Height: d.Height,
                MipLevels: level + 1,
                ArraySize: 1,
                Format: d.Format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
            };
            let mut mip = None;
            self.dev.CreateTexture2D(&mip_desc, None, Some(&mut mip))?;
            let mip = mip.unwrap();
            let mut view = None;
            self.dev.CreateShaderResourceView(&mip, None, Some(&mut view))?;
            let (w, h) = ((d.Width >> level).max(1), (d.Height >> level).max(1));
            let st_desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: d.Format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            self.dev.CreateTexture2D(&st_desc, None, Some(&mut staging))?;
            Ok((mip, view.unwrap(), level, staging.unwrap(), w, h))
        }
    }
}

/// GDI fallback: the desktop scaled down to COLS x ROWS.
fn gdi_sample() -> Option<(Vec<Rgb>, Rgb)> {
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
        Some(reduce(&px, COLS, ROWS as usize, COLS * 4))
    }
}

/// A BGRA image (any size >= COLS wide) to COLS column colours and the overall average.
fn reduce(px: &[u8], w: usize, h: usize, pitch: usize) -> (Vec<Rgb>, Rgb) {
    let mut cols = Vec::with_capacity(COLS);
    let (mut ar, mut ag, mut ab, mut an) = (0u64, 0u64, 0u64, 0u64);
    for c in 0..COLS {
        let (x0, x1) = (c * w / COLS, ((c + 1) * w / COLS).max(c * w / COLS + 1));
        let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
        for y in 0..h {
            for x in x0..x1.min(w) {
                let o = y * pitch + x * 4; // BGRA
                b += px[o] as u64;
                g += px[o + 1] as u64;
                r += px[o + 2] as u64;
                n += 1;
            }
        }
        ar += r;
        ag += g;
        ab += b;
        an += n;
        let n = n.max(1);
        cols.push(vivid(Rgb::new((r / n) as u8, (g / n) as u8, (b / n) as u8)));
    }
    let an = an.max(1);
    (cols, vivid(Rgb::new((ar / an) as u8, (ag / an) as u8, (ab / an) as u8)))
}

/// Screen averages come out greyish; push saturation up so LEDs show a clear colour.
fn vivid(c: Rgb) -> Rgb {
    let (r, g, b) = (c.r as f32, c.g as f32, c.b as f32);
    let avg = (r + g + b) / 3.0;
    let f = |v: f32| (avg + (v - avg) * 1.8).clamp(0.0, 255.0) as u8;
    Rgb::new(f(r), f(g), f(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_averages_columns() {
        // 64 x 2, left half pure red, right half pure blue (BGRA), with row padding
        let (w, h, pitch) = (64, 2, 64 * 4 + 16);
        let mut px = vec![0u8; pitch * h];
        for y in 0..h {
            for x in 0..w {
                let o = y * pitch + x * 4;
                if x < 32 { px[o + 2] = 255 } else { px[o] = 255 }
            }
        }
        let (cols, avg) = reduce(&px, w, h, pitch);
        assert_eq!(cols.len(), COLS);
        assert_eq!(cols[0], vivid(Rgb::new(255, 0, 0)));
        assert_eq!(cols[COLS - 1], vivid(Rgb::new(0, 0, 255)));
        assert_eq!(avg, vivid(Rgb::new(127, 0, 127)));
    }

    /// Live: duplication works on this PC and an unchanged screen costs nothing. `cargo test -- --ignored dxgi`
    #[test]
    #[ignore]
    fn dxgi_live() {
        let mut g = Grabber::new();
        assert!(g.sample().is_some()); // sets up the device and textures
        let mut busy = std::time::Duration::ZERO;
        for _ in 0..40 {
            let t = std::time::Instant::now();
            assert!(g.sample().is_some());
            busy += t.elapsed();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let t = std::time::Instant::now();
        let _ = gdi_sample();
        println!("dxgi {:?} per sample, gdi {:?}, duplication {}", busy / 40, t.elapsed(), if g.dup.is_some() { "on" } else { "OFF (GDI)" });
        assert!(g.dup.is_some());
    }
}
