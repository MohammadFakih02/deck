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
    /// a staging texture for `area`: (w, h, texture)
    area: Option<(u32, u32, ID3D11Texture2D)>,
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
        Ok(Capture { dev, ctx, rdev, pool, _session: session, size, shrink: Shrink { tex: None }, area: None })
    }

    /// A part of the last frame grabbed at the window's full resolution (`r`: x0, y0, x1, y1 as
    /// shares of the frame): the shrunk frame loses the detail small text needs (a boss's name).
    pub fn area(&mut self, r: (f32, f32, f32, f32)) -> Result<Frame> {
        let Some((w, h, mip, ..)) = self.shrink.tex.as_ref() else { return Err(windows::core::Error::new(E_FAIL, "no frame yet")) };
        let (w, h) = (*w, *h);
        let at = |v: f32, of: u32| ((v.clamp(0.0, 1.0) * of as f32) as u32).min(of);
        let (x0, y0, x1, y1) = (at(r.0, w), at(r.1, h), at(r.2, w), at(r.3, h));
        let (aw, ah) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if aw < 8 || ah < 4 {
            return Err(windows::core::Error::new(E_FAIL, "area too small"));
        }
        let mip = mip.clone();
        unsafe {
            if self.area.as_ref().is_none_or(|a| (a.0, a.1) != (aw, ah)) {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: aw,
                    Height: ah,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut t = None;
                self.dev.CreateTexture2D(&desc, None, Some(&mut t))?;
                self.area = Some((aw, ah, t.ok_or_else(|| windows::core::Error::new(E_FAIL, "no texture"))?));
            }
            let Some((_, _, staging)) = self.area.as_ref() else { return Err(windows::core::Error::new(E_FAIL, "no texture")) };
            // level 0 of the mip chain still holds the last frame as it came
            let bx = D3D11_BOX { left: x0, top: y0, front: 0, right: x1, bottom: y1, back: 1 };
            self.ctx.CopySubresourceRegion(staging, 0, 0, 0, 0, &mip, 0, Some(&bx));
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let (sw, sh, pitch) = (aw as usize, ah as usize, map.RowPitch as usize);
            let rows = std::slice::from_raw_parts(map.pData as *const u8, pitch * sh);
            let mut px = Vec::with_capacity(sw * sh * 4);
            for y in 0..sh {
                px.extend_from_slice(&rows[y * pitch..y * pitch + sw * 4]);
            }
            self.ctx.Unmap(staging, 0);
            Ok(Frame { w: sw, h: sh, px })
        }
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

impl Drop for Capture {
    /// The session and the frame pool hold the Direct3D device; left to reference counting they
    /// never let go of it, and every capture opened (each switch back to the game) kept its device,
    /// the driver's 16 threads and ~27 MB alive for good (2026-10-21: 645 driver threads, 1.2 GB).
    fn drop(&mut self) {
        let _ = self._session.Close();
        let _ = self.pool.Close();
        unsafe {
            self.ctx.ClearState();
            self.ctx.Flush();
        }
    }
}

/// A frame as the gate's 64x36 thumbnail: each cell's brightest colour. Plain grey makes a red
/// death banner ("YOU DIED", "KHAZAN HAS FALLEN") a dull mid grey; this keeps it bright on black.
pub fn thumb(f: &Frame) -> Vec<u8> {
    colour(f, super::detect::TW, super::detect::TH).as_chunks::<3>().0.iter().map(|c| c[0].max(c[1]).max(c[2])).collect()
}

/// A frame shrunk to `tw` x `th`, RGB, each cell the average of its pixels.
pub fn colour(f: &Frame, tw: usize, th: usize) -> Vec<u8> {
    let (w, h) = (f.w, f.h);
    let mut out = vec![0u8; tw * th * 3];
    if w == 0 || h == 0 {
        return out;
    }
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let (mut sum, mut n) = ([0u32; 3], 0u32);
            // every other pixel each way is plenty for an average
            for y in (y0..y1.min(h)).step_by(2) {
                for x in (x0..x1.min(w)).step_by(2) {
                    let p = &f.px[(y * w + x) * 4..];
                    sum[0] += p[2] as u32;
                    sum[1] += p[1] as u32;
                    sum[2] += p[0] as u32;
                    n += 1;
                }
            }
            for c in 0..3 {
                out[(ty * tw + tx) * 3 + c] = (sum[c] / n.max(1)) as u8;
            }
        }
    }
    out
}

/// The middle of a frame for the OCR, as grey from each pixel's brightest channel, stretched so the
/// darkest 2 % are black and the brightest 0.2 % white. OCR works on brightness, and a death banner
/// in dark red on a darkened screen ("KHAZAN HAS FALLEN", "YOU DIED") is faint in plain grey; this
/// makes it white on black. A flat, dark picture is not blown up into noise (the range has a floor).
pub fn middle_for_ocr(f: &Frame) -> (usize, usize, Vec<u8>) {
    let (w, h, mut px) = middle(f);
    stretch(&mut px);
    (w, h, px)
}

/// A part of a frame (`r`: x0, y0, x1, y1 in its pixels).
pub fn crop(f: &Frame, r: (usize, usize, usize, usize)) -> Frame {
    let (x0, x1, y0, y1) = (r.0.min(f.w), r.2.min(f.w), r.1.min(f.h), r.3.min(f.h));
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let mut px = Vec::with_capacity(w * h * 4);
    for y in y0..y1 {
        px.extend_from_slice(&f.px[(y * f.w + x0) * 4..(y * f.w + x1) * 4]);
    }
    Frame { w, h, px }
}

/// Small text for the OCR: stretched like `middle_for_ocr`, then made smoothly bigger (bilinear)
/// until it is at least `tall` pixels high; Windows' OCR misreads small, blocky letters.
pub fn text_for_ocr(f: Frame, tall: usize) -> (usize, usize, Vec<u8>) {
    let (w, h, mut px) = (f.w, f.h, f.px);
    stretch(&mut px);
    let grey: Vec<u8> = px.chunks(4).map(|p| p[0]).collect();
    enlarge(&grey, w, h, tall)
}

/// Small text made clean for the OCR: the line of text alone (rows without text cut away: the
/// scenery, a damage number under it), as black letters on white (Otsu's threshold; the letters
/// are whichever side has fewer pixels), then made bigger like `text_for_ocr`.
pub fn clean_text_for_ocr(f: Frame, tall: usize) -> (usize, usize, Vec<u8>) {
    let (w, h) = (f.w, f.h);
    if w == 0 || h == 0 {
        return (0, 0, vec![]);
    }
    let grey: Vec<u8> = f.px.chunks(4).map(|p| p[0].max(p[1]).max(p[2])).collect();
    let t = otsu(&grey);
    let bright = grey.iter().filter(|v| **v > t).count();
    let letters_bright = bright * 2 <= grey.len();
    let ink: Vec<bool> = grey.iter().map(|v| (*v > t) == letters_bright).collect();
    // the band of rows with the most ink (gaps of a row or two inside letters allowed)
    let per_row: Vec<usize> = (0..h).map(|y| ink[y * w..(y + 1) * w].iter().filter(|i| **i).count()).collect();
    let need = (w / 100).max(2);
    let (mut best, mut cur) = ((0usize, 0usize, 0usize), (0usize, 0usize, 0usize));
    let mut gap = 0;
    for (y, n) in per_row.iter().enumerate() {
        if *n >= need {
            if cur.2 == 0 {
                cur = (y, y + 1, 0);
            }
            cur.1 = y + 1;
            cur.2 += n;
            gap = 0;
        } else if cur.2 > 0 {
            gap += 1;
            if gap > 2 {
                if cur.2 > best.2 {
                    best = cur;
                }
                cur = (0, 0, 0);
            }
        }
    }
    if cur.2 > best.2 {
        best = cur;
    }
    let (y0, y1) = if best.2 == 0 { (0, h) } else { (best.0.saturating_sub(2), (best.1 + 2).min(h)) };
    let out: Vec<u8> = (y0..y1).flat_map(|y| ink[y * w..(y + 1) * w].iter().map(|i| if *i { 0 } else { 255 }).collect::<Vec<u8>>()).collect();
    enlarge(&out, w, y1 - y0, tall)
}

/// Small text made black on white by a threshold for each pixel (Sauvola's: the mean and spread of
/// the pixels around it, from running sums), after making it bigger like `text_for_ocr`. One
/// threshold for the whole name fails when part of it is over a bright sky and part over a dark
/// wall; this doesn't. The letters are the side the strong differences fall on: they are few and
/// stand far from what's around them.
pub fn local_text_for_ocr(f: Frame, tall: usize) -> (usize, usize, Vec<u8>) {
    if f.w == 0 || f.h == 0 {
        return (0, 0, vec![]);
    }
    let grey: Vec<u8> = f.px.chunks(4).map(|p| p[0].max(p[1]).max(p[2])).collect();
    let (w, h, big) = enlarge(&grey, f.w, f.h, tall);
    let g: Vec<u8> = big.chunks(4).map(|p| p[0]).collect();
    let (mean, sd) = local_stats(&g, w, h, (h / 3).max(4));
    let skew: f64 = g.iter().zip(&mean).map(|(v, m)| (*v as f64 - m).powi(3)).sum();
    let bright = skew > 0.0;
    let out = g.iter().zip(mean.iter().zip(&sd)).flat_map(|(v, (m, s))| {
        let (v, m) = if bright { (255.0 - *v as f64, 255.0 - m) } else { (*v as f64, *m) };
        let ink = v < m * (1.0 + SAUVOLA_K * (s / 128.0 - 1.0));
        let c = if ink { 0 } else { 255 };
        [c, c, c, 255]
    });
    (w, h, out.collect())
}

/// Sauvola's k: how far under the local mean a pixel must be to be a letter's
const SAUVOLA_K: f64 = 0.2;

/// Each pixel's mean and spread over the square `r` around it.
fn local_stats(g: &[u8], w: usize, h: usize, r: usize) -> (Vec<f64>, Vec<f64>) {
    let (iw, ih) = (w + 1, h + 1);
    let (mut sum, mut sq) = (vec![0u64; iw * ih], vec![0u64; iw * ih]);
    for y in 0..h {
        let (mut rs, mut rq) = (0u64, 0u64);
        for x in 0..w {
            let v = g[y * w + x] as u64;
            rs += v;
            rq += v * v;
            sum[(y + 1) * iw + x + 1] = sum[y * iw + x + 1] + rs;
            sq[(y + 1) * iw + x + 1] = sq[y * iw + x + 1] + rq;
        }
    }
    let box_of = |t: &[u64], x0: usize, y0: usize, x1: usize, y1: usize| t[y1 * iw + x1] + t[y0 * iw + x0] - t[y0 * iw + x1] - t[y1 * iw + x0];
    let (mut mean, mut sd) = (vec![0f64; w * h], vec![0f64; w * h]);
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let n = ((x1 - x0) * (y1 - y0)) as f64;
            let m = box_of(&sum, x0, y0, x1, y1) as f64 / n;
            mean[y * w + x] = m;
            sd[y * w + x] = (box_of(&sq, x0, y0, x1, y1) as f64 / n - m * m).max(0.0).sqrt();
        }
    }
    (mean, sd)
}

/// Otsu's threshold: the grey level that best splits the picture into two groups.
fn otsu(grey: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for v in grey {
        hist[*v as usize] += 1;
    }
    let total = grey.len() as f64;
    let sum: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * *c as f64).sum();
    let (mut wb, mut sb, mut best, mut at) = (0f64, 0f64, 0f64, 0u8);
    for (i, c) in hist.iter().enumerate() {
        wb += *c as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += i as f64 * *c as f64;
        let (mb, mf) = (sb / wb, (sum - sb) / wf);
        let between = wb * wf * (mb - mf) * (mb - mf);
        if between > best {
            best = between;
            at = i as u8;
        }
    }
    at
}

/// A grey picture made smoothly bigger (bilinear) until it is at least `tall` high (at most 4x),
/// as BGRA.
fn enlarge(grey: &[u8], w: usize, h: usize, tall: usize) -> (usize, usize, Vec<u8>) {
    let bgra = |g: &[u8]| g.iter().flat_map(|v| [*v, *v, *v, 255]).collect::<Vec<u8>>();
    if w == 0 || h == 0 || h >= tall {
        return (w, h, bgra(grey));
    }
    let k = (tall as f32 / h as f32).min(4.0);
    let (bw, bh) = ((w as f32 * k) as usize, (h as f32 * k) as usize);
    let at = |x: usize, y: usize| grey[y.min(h - 1) * w + x.min(w - 1)] as f32;
    let mut big = Vec::with_capacity(bw * bh * 4);
    for y in 0..bh {
        let sy = ((y as f32 + 0.5) / k - 0.5).max(0.0);
        let (y0, fy) = (sy as usize, sy.fract());
        for x in 0..bw {
            let sx = ((x as f32 + 0.5) / k - 0.5).max(0.0);
            let (x0, fx) = (sx as usize, sx.fract());
            let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
            let bot = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
            let v = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
            big.extend_from_slice(&[v, v, v, 255]);
        }
    }
    (bw, bh, big)
}

/// Grey from each pixel's brightest channel, the darkest 2 % black and the brightest 0.2 % white.
fn stretch(px: &mut [u8]) {
    let n = (px.len() / 4) as u64;
    if n == 0 {
        return;
    }
    let mut hist = [0u64; 256];
    for p in px.as_chunks::<4>().0 {
        hist[p[0].max(p[1]).max(p[2]) as usize] += 1;
    }
    let at = |permille: u64| {
        let mut acc = 0;
        for (i, c) in hist.iter().enumerate() {
            acc += c;
            if acc * 1000 >= n * permille {
                return i as u32;
            }
        }
        255
    };
    let lo = at(20);
    let hi = at(998).max(lo + 48);
    for p in px.as_chunks_mut::<4>().0 {
        let v = p[0].max(p[1]).max(p[2]) as u32;
        let s = (v.saturating_sub(lo) * 255 / (hi - lo)).min(255) as u8;
        p[0] = s;
        p[1] = s;
        p[2] = s;
        p[3] = 255;
    }
}

/// The middle of a frame (where the gate looked): (w, h, BGRA).
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
    fn letters_over_a_dark_and_a_bright_background_both_come_out_black() {
        // left half dark (20) with a letter stroke of 100; right half bright (150) with one of 230:
        // the left stroke is darker than the right background, so no one threshold splits both
        let (w, h) = (60, 24);
        let mut px = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let stroke = (6..18).contains(&y) && ((8..12).contains(&x) || (44..48).contains(&x));
                let v: u8 = match (x < 30, stroke) {
                    (true, false) => 20,
                    (true, true) => 100,
                    (false, false) => 150,
                    (false, true) => 230,
                };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let (ow, oh, out) = local_text_for_ocr(Frame { w, h, px }, 24);
        assert_eq!((ow, oh), (w, h));
        let at = |x: usize, y: usize| out[(y * ow + x) * 4];
        assert_eq!((at(9, 12), at(46, 12)), (0, 0), "both strokes are ink");
        assert_eq!((at(20, 2), at(52, 2)), (255, 255), "both backgrounds are paper");
    }

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

    #[test]
    fn small_text_is_cut_out_and_made_smoothly_bigger() {
        let (w, h) = (100, 50);
        let mut px = [0u8, 0, 0, 255].repeat(w * h);
        for y in 12..14 {
            for x in 22..28 {
                px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let c = crop(&Frame { w, h, px }, (20, 10, 30, 15));
        assert_eq!((c.w, c.h), (10, 5));
        let (bw, bh, big) = text_for_ocr(c, 20);
        assert_eq!((bw, bh, big.len()), (40, 20, 40 * 20 * 4), "4x, up to `tall`");
        let at = |x: usize, y: usize| big[(y * bw + x) * 4];
        assert_eq!(at(20, 10), 255, "the line, white");
        assert_eq!(at(20, 0), 0, "above it, black");
        assert!((1..255).contains(&at(20, 6)), "a soft edge, not a block: {}", at(20, 6));
        let (_, th, _) = text_for_ocr(crop(&Frame { w, h, px: [9u8, 9, 9, 255].repeat(w * h) }, (0, 0, 100, 50)), 20);
        assert_eq!(th, 50, "tall enough already: not made bigger");
    }

    /// A light name over a busy scene, a damage number under it: only the name's rows, black on white.
    #[test]
    fn a_name_is_cleaned_to_black_letters_on_white() {
        let (w, h) = (200, 60);
        // a scene: a soft gradient with some texture
        let mut px: Vec<u8> = (0..w * h).flat_map(|i| { let v = (40 + (i % w) / 8 + (i / w) % 7) as u8; [v, v, v, 255] }).collect();
        let mut put = |x0: usize, x1: usize, y0: usize, y1: usize, v: u8| {
            for y in y0..y1 {
                for x in x0..x1 {
                    px[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&[v, v, v]);
                }
            }
        };
        // "letters": light strokes on rows 10..22
        for x in (20..180).step_by(6) {
            put(x, x + 3, 10, 22, 235);
        }
        // a small damage number low down
        put(150, 156, 48, 52, 230);
        let (cw, ch, out) = clean_text_for_ocr(Frame { w, h, px }, 0);
        assert!((12..=18).contains(&ch), "only the name's rows (+ a margin): {ch}");
        assert_eq!(cw, w);
        let grey: Vec<u8> = out.chunks(4).map(|p| p[0]).collect();
        let dark = grey.iter().filter(|v| **v == 0).count();
        assert!(dark > 0 && dark * 2 < grey.len(), "letters black, the rest white");
        assert!(grey.iter().all(|v| *v == 0 || *v == 255), "two colours only");
    }

    #[test]
    fn dark_red_words_become_white_on_black_for_the_ocr() {
        let (w, h) = (640, 360);
        // near-black with a dark red band (BGRA): red 90 on 12 is faint as plain grey
        let mut px = [12u8, 12, 12, 255].repeat(w * h);
        for y in 170..190 {
            for x in 200..440 {
                px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[20, 18, 90, 255]);
            }
        }
        let (mw, _, m) = middle_for_ocr(&Frame { w, h, px });
        let at = |x: usize, y: usize| m[((y - 60) * mw + (x - 80)) * 4];
        assert_eq!(at(300, 180), 255, "the words: white");
        assert!(at(100, 100) < 20, "the screen around them: black");
        let (_, _, flat) = middle_for_ocr(&Frame { w, h, px: [12u8, 12, 12, 255].repeat(w * h) });
        assert!(flat.chunks(4).all(|p| p[0] < 10), "a plain dark picture stays dark, not noise");
    }
}
