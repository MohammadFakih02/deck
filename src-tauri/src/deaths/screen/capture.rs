//! Frames of one game window through Windows Graphics Capture (from `My-apps\capture-spike`):
//! the window's own picture, even when covered, without touching the game. Each frame is
//! shrunk on the GPU (a mip chain) and one level about 1000 px wide is read back, enough for OCR
//! of a death banner. Nothing is saved; frames live only in memory until the next one.
//!
//! May be black for exclusive fullscreen or a protected window (the gate notices: "blind").

use windows::core::{Interface, Result};
use windows::Foundation::TimeSpan;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// widest frame read back (the OCR's input)
const MAX_W: u32 = 1280;

/// BGRA, tightly packed.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

struct Shrink {
    /// (source w, h, mip texture, its view, level read back, staging, staging w, h)
    #[allow(clippy::type_complexity)]
    tex: Option<(u32, u32, ID3D11Texture2D, ID3D11ShaderResourceView, u32, ID3D11Texture2D, u32, u32)>,
}

pub struct Capture {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    rdev: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    _session: GraphicsCaptureSession,
    size: SizeInt32,
    shrink: Shrink,
}

impl Capture {
    /// Starts capturing `hwnd`, asking Windows for at most `rate` frames a second.
    pub fn new(hwnd: isize, rate: f64) -> Result<Capture> {
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(windows::core::Error::new(E_NOTIMPL, "Windows Graphics Capture isn't available"));
        }
        let (dev, ctx) = unsafe {
            let (mut dev, mut ctx) = (None, None);
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))?;
            (dev.ok_or_else(|| windows::core::Error::new(E_FAIL, "no Direct3D device"))?, ctx.ok_or_else(|| windows::core::Error::new(E_FAIL, "no Direct3D context"))?)
        };
        let rdev: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dev.cast::<IDXGIDevice>()?)? }.cast()?;
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(hwnd as *mut _))? };
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&rdev, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        // no yellow border, no cursor (Windows 11; older Windows ignores these)
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetMinUpdateInterval(TimeSpan { Duration: (10_000_000.0 / rate) as i64 });
        session.StartCapture()?;
        Ok(Capture { dev, ctx, rdev, pool, _session: session, size, shrink: Shrink { tex: None } })
    }

    /// The newest frame since the last call, or None when the window drew nothing new.
    pub fn grab(&mut self) -> Result<Option<Frame>> {
        let mut last: Option<Direct3D11CaptureFrame> = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            if let Some(old) = last.replace(f) {
                let _ = old.Close();
            }
        }
        let Some(f) = last else { return Ok(None) };
        let cs = f.ContentSize()?;
        let tex: ID3D11Texture2D = unsafe { f.Surface()?.cast::<IDirect3DDxgiInterfaceAccess>()?.GetInterface()? };
        let mut d = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut d) };
        let bx = D3D11_BOX { left: 0, top: 0, front: 0, right: (cs.Width.max(1) as u32).min(d.Width), bottom: (cs.Height.max(1) as u32).min(d.Height), back: 1 };
        let out = self.shrink(&tex, bx);
        let _ = f.Close();
        if (cs.Width, cs.Height) != (self.size.Width, self.size.Height) {
            self.size = cs;
            self.pool.Recreate(&self.rdev, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, cs)?;
        }
        out.map(Some)
    }

    fn shrink(&mut self, src: &ID3D11Texture2D, bx: D3D11_BOX) -> Result<Frame> {
        let (w, h) = (bx.right - bx.left, bx.bottom - bx.top);
        if w < 64 || h < 36 {
            return Err(windows::core::Error::new(E_FAIL, "window too small"));
        }
        unsafe {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            src.GetDesc(&mut d);
            if d.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                return Err(windows::core::Error::new(E_FAIL, "unexpected pixel format"));
            }
            if self.shrink.tex.as_ref().is_none_or(|t| (t.0, t.1) != (w, h)) {
                self.shrink.tex = Some(self.make(w, h)?);
            }
            let Some((_, _, mip, view, level, staging, sw, sh)) = self.shrink.tex.as_ref() else { return Err(windows::core::Error::new(E_FAIL, "no texture")) };
            self.ctx.CopySubresourceRegion(mip, 0, 0, 0, 0, src, 0, Some(&bx));
            if *level > 0 {
                self.ctx.GenerateMips(view);
            }
            self.ctx.CopySubresourceRegion(staging, 0, 0, 0, 0, mip, *level, None);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let (sw, sh, pitch) = (*sw as usize, *sh as usize, map.RowPitch as usize);
            let rows = std::slice::from_raw_parts(map.pData as *const u8, pitch * sh);
            let mut px = Vec::with_capacity(sw * sh * 4);
            for y in 0..sh {
                px.extend_from_slice(&rows[y * pitch..y * pitch + sw * 4]);
            }
            self.ctx.Unmap(staging, 0);
            Ok(Frame { w: sw, h: sh, px })
        }
    }

    #[allow(clippy::type_complexity)]
    fn make(&self, w: u32, h: u32) -> Result<(u32, u32, ID3D11Texture2D, ID3D11ShaderResourceView, u32, ID3D11Texture2D, u32, u32)> {
        let mut level = 0;
        while (w >> level) > MAX_W {
            level += 1;
        }
        let desc = |w, h, mips, usage, bind: u32, cpu: u32, misc: u32| D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: mips,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: usage,
            BindFlags: bind,
            CPUAccessFlags: cpu,
            MiscFlags: misc,
        };
        unsafe {
            let mut mip = None;
            let bind = (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32;
            self.dev.CreateTexture2D(&desc(w, h, level + 1, D3D11_USAGE_DEFAULT, bind, 0, D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32), None, Some(&mut mip))?;
            let mip = mip.ok_or_else(|| windows::core::Error::new(E_FAIL, "no texture"))?;
            let mut view = None;
            self.dev.CreateShaderResourceView(&mip, None, Some(&mut view))?;
            let (sw, sh) = ((w >> level).max(1), (h >> level).max(1));
            let mut staging = None;
            self.dev.CreateTexture2D(&desc(sw, sh, 1, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ.0 as u32, 0), None, Some(&mut staging))?;
            let view = view.ok_or_else(|| windows::core::Error::new(E_FAIL, "no view"))?;
            let staging = staging.ok_or_else(|| windows::core::Error::new(E_FAIL, "no staging texture"))?;
            Ok((w, h, mip, view, level, staging, sw, sh))
        }
    }
}

/// A frame as the gate's 64x36 grey thumbnail.
pub fn thumb(f: &Frame) -> Vec<u8> {
    grey(f, super::detect::TW, super::detect::TH)
}

/// A frame shrunk to `tw` x `th` grey, each cell the average of its pixels.
pub fn grey(f: &Frame, tw: usize, th: usize) -> Vec<u8> {
    let (w, h) = (f.w, f.h);
    let mut out = vec![0u8; tw * th];
    if w == 0 || h == 0 {
        return out;
    }
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let (mut sum, mut n) = (0u32, 0u32);
            // every other pixel each way is plenty for an average
            for y in (y0..y1.min(h)).step_by(2) {
                for x in (x0..x1.min(w)).step_by(2) {
                    let p = &f.px[(y * w + x) * 4..];
                    sum += (p[2] as u32 * 54 + p[1] as u32 * 183 + p[0] as u32 * 19) >> 8;
                    n += 1;
                }
            }
            out[ty * tw + tx] = (sum / n.max(1)) as u8;
        }
    }
    out
}

/// The middle of a frame (where the gate looked), for the OCR: (w, h, BGRA).
pub fn middle(f: &Frame) -> (usize, usize, Vec<u8>) {
    let (x0, x1, y0, y1) = (f.w / 8, f.w - f.w / 8, f.h / 6, f.h - f.h / 6);
    let w = x1.saturating_sub(x0);
    let mut px = Vec::with_capacity(w * (y1 - y0) * 4);
    for y in y0..y1 {
        px.extend_from_slice(&f.px[(y * f.w + x0) * 4..(y * f.w + x1) * 4]);
    }
    (w, y1 - y0, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_and_the_middle_come_from_the_right_pixels() {
        let (w, h) = (640, 360);
        let mut px = vec![0u8; w * h * 4];
        // a white block in the middle
        for y in 150..210 {
            for x in 200..440 {
                px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let f = Frame { w, h, px };
        let t = thumb(&f);
        assert_eq!(t[18 * 64 + 32], 255, "middle cell white");
        assert_eq!(t[0], 0, "corner black");
        let (mw, mh, m) = middle(&f);
        assert_eq!((mw, mh, m.len()), (480, 240, 480 * 240 * 4), "the middle 3/4 across, 2/3 down");
    }
}
