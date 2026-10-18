//! The PC's screen on the phone (the phone remote's Screen view), and the phone's taps and typing
//! sent back as mouse and keyboard input. Works wherever the phone remote does: the home network,
//! or anywhere over Tailscale.
//!
//! Capture: DXGI desktop duplication of the chosen monitor (on whichever graphics card drives it,
//! so laptops with two GPUs work), shrunk on the GPU through a mip chain; a screen that hasn't
//! changed costs nothing and sends nothing. GDI is the fallback (HDR desktops, duplication not
//! available) and also takes the first picture of every session. Frames go out as JPEG, and only
//! while the phone has taken the previous ones, so a slow connection never builds up a queue.
//!
//! Off by default (`remote_screen`: off / view / control). Nothing here runs unless a phone has
//! the Screen view open.

use crate::h264;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows_sys::Win32::Foundation::{LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

// ------------------------------------------------------------------ monitors

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mon {
    pub handle: isize,
    pub left: i32,
    pub top: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

/// Every monitor, primary first, then left to right (physical pixels).
pub fn monitors() -> Vec<Mon> {
    unsafe extern "system" fn each(hm: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> windows_sys::core::BOOL {
        let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(hm, &mut mi) } != 0 {
            let r = mi.rcMonitor;
            let v = unsafe { &mut *(out as *mut Vec<Mon>) };
            v.push(Mon { handle: hm as isize, left: r.left, top: r.top, w: r.right - r.left, h: r.bottom - r.top, primary: mi.dwFlags & 1 != 0 });
        }
        1
    }
    let mut v: Vec<Mon> = Vec::new();
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut v as *mut _ as LPARAM) };
    v.sort_by_key(|m| (!m.primary, m.left, m.top));
    v
}

// ------------------------------------------------------------------ clock

/// Pictures and sound carry the time they were captured, in ms on Windows' performance counter
/// (the clock desktop duplication and WASAPI stamp their data with), so the phone can line them up.
fn qpc_ms(ticks: i64) -> f64 {
    static FREQ: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let f = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut f) };
        f.max(1) as f64
    });
    ticks as f64 * 1000.0 / f
}

fn now_ms() -> f64 {
    let mut t = 0i64;
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    qpc_ms(t)
}

/// A binary message for the phone: the capture time (f64 ms, little-endian), then the data.
fn stamped(ts: f64, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + data.len());
    v.extend_from_slice(&ts.to_le_bytes());
    v.extend_from_slice(data);
    v
}

// ------------------------------------------------------------------ quality

#[derive(Clone, Copy)]
struct Preset {
    /// widest picture sent (a wider screen is shrunk)
    max_w: u32,
    jpeg: u8,
}

fn preset(name: &str) -> Preset {
    match name {
        // "Full": the screen's own size (up to 2560 px), small text stays readable
        "sharp" => Preset { max_w: 2560, jpeg: 88 },
        "fast" => Preset { max_w: 960, jpeg: 60 },
        _ => Preset { max_w: 1280, jpeg: 75 },
    }
}

/// Shortest time between pictures for 30 or 60 a second (a little under, so a 60 Hz screen's
/// updates aren't skipped by timer jitter).
fn gap(fps: u32) -> Duration {
    Duration::from_micros(1_000_000 / fps.clamp(10, 60) as u64 - 2_000)
}

/// The picture size for a monitor: its own size, shrunk to fit `max_w` (even numbers for JPEG).
fn fit(w: i32, h: i32, max_w: u32) -> (u32, u32) {
    let (w, h) = (w.max(1) as u32, h.max(1) as u32);
    if w <= max_w {
        return (w, h);
    }
    let nh = (h as u64 * max_w as u64 / w as u64) as u32;
    (max_w & !1, nh.max(2) & !1)
}

// ------------------------------------------------------------------ capture

/// A BGRA picture, top row first, and when it was on screen (ms, see `qpc_ms`).
pub(crate) struct Shot {
    pub(crate) px: Vec<u8>,
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) ts: f64,
}

/// GDI: the monitor scaled straight to the picture size (HALFTONE keeps text readable).
pub(crate) fn gdi_shot(m: &Mon, w: u32, h: u32) -> Option<Shot> {
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let dc = CreateCompatibleDC(screen);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w as i32;
        bi.bmiHeader.biHeight = -(h as i32); // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        let mut out = None;
        if !bmp.is_null() && !bits.is_null() {
            let old = SelectObject(dc, bmp);
            SetStretchBltMode(dc, HALFTONE);
            let ok = if w as i32 == m.w && h as i32 == m.h {
                BitBlt(dc, 0, 0, w as i32, h as i32, screen, m.left, m.top, SRCCOPY | CAPTUREBLT)
            } else {
                StretchBlt(dc, 0, 0, w as i32, h as i32, screen, m.left, m.top, m.w, m.h, SRCCOPY | CAPTUREBLT)
            };
            if ok != 0 {
                out = Some(Shot { px: std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec(), w, h, ts: now_ms() });
            }
            SelectObject(dc, old);
            DeleteObject(bmp);
        }
        DeleteDC(dc);
        ReleaseDC(std::ptr::null_mut(), screen);
        out
    }
}

struct Dup {
    ctx: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    /// (full-size texture with mips (None when no shrinking), the mip level read, staging copy, its size)
    mip: Option<(ID3D11Texture2D, ID3D11ShaderResourceView)>,
    level: u32,
    staging: ID3D11Texture2D,
    w: u32,
    h: u32,
}

impl Dup {
    /// Duplication of monitor `m` on the graphics card that drives it.
    fn new(m: &Mon, max_w: u32) -> windows::core::Result<Self> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            let mut ai = 0;
            loop {
                let adapter = factory.EnumAdapters1(ai)?; // runs out: DXGI_ERROR_NOT_FOUND
                ai += 1;
                let mut oi = 0;
                while let Ok(output) = adapter.EnumOutputs(oi) {
                    oi += 1;
                    let d = output.GetDesc()?;
                    if d.Monitor.0 as isize != m.handle {
                        continue;
                    }
                    let mut dev = None;
                    let mut ctx = None;
                    D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))?;
                    let (dev, ctx): (ID3D11Device, ID3D11DeviceContext) = (dev.unwrap(), ctx.unwrap());
                    let dup = output.cast::<IDXGIOutput1>()?.DuplicateOutput(&dev)?;
                    let dd = dup.GetDesc();
                    if dd.ModeDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                        return Err(windows::core::Error::from(DXGI_ERROR_UNSUPPORTED)); // HDR desktop: GDI
                    }
                    let (fw, fh) = (dd.ModeDesc.Width, dd.ModeDesc.Height);
                    // halve on the GPU while the picture is still clearly wider than wanted
                    let mut level = 0;
                    while (fw >> level) > max_w * 3 / 2 && (fw >> (level + 1)) >= 320 {
                        level += 1;
                    }
                    let (w, h) = ((fw >> level).max(1), (fh >> level).max(1));
                    let tex = |w: u32, h: u32, mips: u32, usage: D3D11_USAGE, bind: u32, cpu: u32, misc: u32| D3D11_TEXTURE2D_DESC {
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
                    let mip = if level > 0 {
                        let mut t = None;
                        dev.CreateTexture2D(&tex(fw, fh, level + 1, D3D11_USAGE_DEFAULT, (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32, 0, D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32), None, Some(&mut t))?;
                        let t = t.unwrap();
                        let mut v = None;
                        dev.CreateShaderResourceView(&t, None, Some(&mut v))?;
                        Some((t, v.unwrap()))
                    } else {
                        None
                    };
                    let mut st = None;
                    dev.CreateTexture2D(&tex(w, h, 1, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ.0 as u32, 0), None, Some(&mut st))?;
                    return Ok(Dup { ctx, dup, mip, level, staging: st.unwrap(), w, h });
                }
            }
        }
    }

    /// Ok(None): nothing new on screen within `wait_ms` (or only the pointer moved).
    fn frame(&mut self, wait_ms: u32) -> windows::core::Result<Option<Shot>> {
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            if let Err(e) = self.dup.AcquireNextFrame(wait_ms, &mut info, &mut res) {
                return if e.code() == DXGI_ERROR_WAIT_TIMEOUT { Ok(None) } else { Err(e) };
            }
            let copied = (|| -> windows::core::Result<bool> {
                if info.LastPresentTime == 0 {
                    return Ok(false);
                }
                let src: ID3D11Texture2D = res.as_ref().unwrap().cast()?;
                match &self.mip {
                    Some((t, v)) => {
                        self.ctx.CopySubresourceRegion(t, 0, 0, 0, 0, &src, 0, None);
                        self.ctx.GenerateMips(v);
                        self.ctx.CopySubresourceRegion(&self.staging, 0, 0, 0, 0, t, self.level, None);
                    }
                    None => self.ctx.CopyResource(&self.staging, &src),
                }
                Ok(true)
            })();
            let _ = self.dup.ReleaseFrame();
            if !copied? {
                return Ok(None);
            }
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let pitch = map.RowPitch as usize;
            let row = self.w as usize * 4;
            let mut px = Vec::with_capacity(row * self.h as usize);
            let src = std::slice::from_raw_parts(map.pData as *const u8, pitch * self.h as usize);
            for y in 0..self.h as usize {
                px.extend_from_slice(&src[y * pitch..y * pitch + row]);
            }
            self.ctx.Unmap(&self.staging, 0);
            Ok(Some(Shot { px, w: self.w, h: self.h, ts: qpc_ms(info.LastPresentTime) }))
        }
    }
}

/// One monitor's pictures for one session.
struct Capturer {
    mon: Mon,
    p: Preset,
    dup: Option<Dup>,
    dup_retry: Instant,
    first: bool,
    last_hash: u64,
}

impl Capturer {
    fn new(mon: Mon, p: Preset) -> Self {
        Capturer { mon, p, dup: None, dup_retry: Instant::now(), first: true, last_hash: 0 }
    }

    /// The picture (at its final size) when the screen changed since the last one; the encoder
    /// thread turns it into a JPEG. Err: nothing can be read (the PC is locked, or an admin
    /// prompt is up: Windows keeps those screens from every app).
    fn grab(&mut self) -> Result<Option<Shot>, ()> {
        let (w, h) = fit(self.mon.w, self.mon.h, self.p.max_w);
        let mut from_gdi = true;
        let shot = if self.first {
            // the first picture: GDI always has one (duplication only reports changes)
            self.first = false;
            gdi_shot(&self.mon, w, h)
        } else {
            if self.dup.is_none() && Instant::now() >= self.dup_retry {
                self.dup = Dup::new(&self.mon, self.p.max_w).ok();
                if self.dup.is_none() {
                    self.dup_retry = Instant::now() + Duration::from_secs(3);
                }
            }
            // waits up to 15 ms for the screen's next update (wakes as soon as it comes)
            match self.dup.as_mut().map(|d| d.frame(15)) {
                Some(Ok(s)) => {
                    from_gdi = false;
                    s
                }
                Some(Err(_)) => {
                    self.dup = None;
                    self.dup_retry = Instant::now() + Duration::from_millis(500);
                    gdi_shot(&self.mon, w, h)
                }
                None => gdi_shot(&self.mon, w, h),
            }
        };
        let Some(mut shot) = shot else { return if self.dup.is_some() { Ok(None) } else { Err(()) } };
        // the GPU only halves: finish to the exact size here
        if shot.w > w {
            shot = shrink(&shot, w, h);
        }
        // GDI returns a picture every time: send it only when it changed (duplication only
        // hands over changed frames, so those skip the check)
        let hash = if from_gdi { hash(&shot.px) } else { 0 };
        if from_gdi && hash == self.last_hash {
            return Ok(None);
        }
        // a locked PC reads as an all-black screen through GDI
        if self.dup.is_none() && shot.px.as_chunks::<4>().0.iter().step_by(97).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0) {
            self.last_hash = 0;
            return Err(());
        }
        self.last_hash = hash;
        Ok(Some(shot))
    }
}

/// Shrinks a picture by less than half: each pixel averages the 2x2 source pixels around its spot.
fn shrink(s: &Shot, w: u32, h: u32) -> Shot {
    let (sw, sh, w, h) = (s.w as usize, s.h as usize, w as usize, h as usize);
    // byte offsets of the two source columns for each picture column, worked out once
    let cols: Vec<(usize, usize)> = (0..w).map(|x| {
        let fx = (x * 2 + 1) * sw / (w * 2);
        (fx * 4, (fx + 1).min(sw - 1) * 4)
    }).collect();
    let mut px = vec![0u8; w * h * 4];
    for (y, out) in px.chunks_exact_mut(w * 4).enumerate() {
        let fy = (y * 2 + 1) * sh / (h * 2);
        let r0 = &s.px[fy * sw * 4..(fy + 1) * sw * 4];
        let y1 = (fy + 1).min(sh - 1);
        let r1 = &s.px[y1 * sw * 4..(y1 + 1) * sw * 4];
        for (o, &(a, b)) in out.as_chunks_mut::<4>().0.iter_mut().zip(&cols) {
            for c in 0..3 {
                o[c] = ((r0[a + c] as u16 + r0[b + c] as u16 + r1[a + c] as u16 + r1[b + c] as u16 + 2) >> 2) as u8;
            }
        }
    }
    Shot { px, w: w as u32, h: h as u32, ts: s.ts }
}

fn hash(b: &[u8]) -> u64 {
    // FNV-1a over 8-byte words: plenty to notice a change
    let mut h: u64 = 0xcbf29ce484222325;
    for c in b.chunks(8) {
        let mut w = [0u8; 8];
        w[..c.len()].copy_from_slice(c);
        h = (h ^ u64::from_le_bytes(w)).wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
fn jpeg(s: &Shot, quality: u8) -> Option<Vec<u8>> {
    jpeg_with(s, quality, false)
}

/// `full_colour`: keep full colour detail (4:4:4) at high quality: halved colour (4:2:0) smears
/// small coloured text and thin lines, but takes about twice as long to encode, so only "Sharp"
/// (still text) uses it.
fn jpeg_with(s: &Shot, quality: u8, full_colour: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.px.len() / 24);
    let mut e = jpeg_encoder::Encoder::new(&mut out, quality);
    if full_colour && quality >= 80 {
        e.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
    }
    // small pictures (slow connections): tables fitted to the picture, a few % smaller for ~1 ms
    if s.w <= 640 {
        e.set_optimized_huffman_tables(true);
    }
    e.encode(&s.px, s.w as u16, s.h as u16, jpeg_encoder::ColorType::Bgra).ok()?;
    Some(out)
}

// ------------------------------------------------------------------ bitrate

/// Picture widths the bitrate limit steps through.
/// "Smooth" goes all the way down: 1 Mb/s at 60 fps leaves ~2 KB a picture, which only a tiny,
/// low-quality JPEG fits in (the phone stretches it: blurry, but every frame arrives).
const WIDTHS: [u32; 13] = [2560, 1920, 1600, 1280, 1024, 854, 640, 480, 360, 320, 256, 192, 160];
/// "Smooth" goes this low before giving up size (and only then frames)
const Q_MIN_SMOOTH: u8 = 4;
/// "Sharp" keeps the picture readable: full size and at least this quality, sending fewer pictures
/// instead (`sharp_fps`: ~5 a second at 8 Mb/s, 10 from 15 Mb/s, 2 on a slow line); only when even those don't
/// fit does it get narrower, never under `SHARP_MIN_W`
const Q_MIN_SHARP: u8 = 60;
const SHARP_MIN_W: u32 = 480;

/// Roughly how many bytes a JPEG takes per pixel at quality `q` (measured on desktop and game
/// screens: ~0.014 at q4, ~0.045 at q20, ~0.1 at q75), to start at a sensible size.
fn bytes_per_px(q: u8) -> f64 {
    0.012 + q as f64 * 0.0012
}

/// The widest picture whose JPEG at quality `q` fits `bytes` (16:9).
fn width_for(bytes: f64, q: u8, max_w: u32) -> u32 {
    let px = bytes / bytes_per_px(q);
    let w = (px * 16.0 / 9.0).sqrt() as u32;
    WIDTHS.iter().copied().find(|&x| x <= w.max(WIDTHS[WIDTHS.len() - 1]) && x <= max_w).unwrap_or(WIDTHS[WIDTHS.len() - 1])
}

/// What the phone's "shown" answers say about the connection: how long pictures take to arrive
/// and how many bytes got through. Auto bitrate reads it.
#[derive(Default)]
pub struct Feedback {
    sent: std::collections::VecDeque<(Instant, usize)>,
    delivered: usize,
    /// delay of recent pictures (ms, smoothed) and the lowest seen lately (the connection's own)
    lat: f64,
    base: f64,
    base_at: Option<Instant>,
    /// lowest delay since the last tune (0: none)
    win_min: f64,
}

impl Feedback {
    fn acked(&mut self) {
        let Some((t, n)) = self.sent.pop_front() else { return };
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.delivered += n;
        self.win_min = if self.win_min == 0.0 { ms } else { self.win_min.min(ms) };
        self.lat = if self.lat == 0.0 { ms } else { self.lat * 0.7 + ms * 0.3 };
        // the baseline: lowest delay, forgotten every 10 s so a slower route is learned
        if self.base == 0.0 || ms < self.base || self.base_at.is_none_or(|t| t.elapsed() > Duration::from_secs(10)) {
            self.base = if self.base_at.is_none_or(|t| t.elapsed() > Duration::from_secs(10)) { self.lat.min(ms) } else { ms };
            self.base_at = Some(Instant::now());
        }
    }
}

/// Keeps the stream under a bitrate limit: the JPEG quality follows the size of recent pictures;
/// when the quality bottoms out the picture gets narrower (and wider again when there's room);
/// and a byte bucket holds pictures back so the average never goes over. With a low limit it
/// aims for fewer, clearer pictures (~13 a second at 2 Mb/s) rather than many smeared ones.
struct Rate {
    kbps: u32,
    /// Auto: `kbps` follows the connection
    auto: bool,
    smooth: bool,
    tuned: Instant,
    max_w: u32,
    q_max: u8,
    q: u8,
    w: u32,
    bucket: f64,
    at: Instant,
    resized: Instant,
    /// pictures in a row that were several times too big
    over: u8,
    /// bytes set aside for pictures being encoded (the size of the last one, each), so the
    /// bucket doesn't look fuller than it is while they're on the encoder thread
    reserved: std::collections::VecDeque<f64>,
    last_n: f64,
    /// what the line carried when measured (kb/s, 0: not measured): Auto starts at 80 % of it, and
    /// on a slow line (under 3 Mb/s: DSL, weak mobile) never goes above that; a fast line's reading
    /// is only a start (a phone busy at that moment answers late and makes it look slower)
    line: u32,
    /// the bitrate set (0: Auto): H.264 treats it as the most to use and still backs off below it
    /// when the line is busy (other uploads share it)
    max: u32,
    /// H.264: congestion is judged by each half second's lowest delay (Wi-Fi power saving delays
    /// some pictures by 100-300 ms without the line being full; a full line delays them all)
    h264: bool,
}

impl Rate {
    /// `kbps` 0 = Auto (starts at 4 Mb/s and follows the connection from there).
    fn new(kbps: u32, p: Preset, smooth: bool) -> Rate {
        let auto = kbps == 0;
        let kbps = if auto { 4_000 } else { kbps };
        let mut r = Rate { kbps, auto, smooth, tuned: Instant::now(), max_w: p.max_w, q_max: p.jpeg, q: 0, w: p.max_w, bucket: 0.0, at: Instant::now(), resized: Instant::now(), over: 0, reserved: Default::default(), last_n: 0.0, line: 0, max: if auto { 0 } else { kbps }, h264: false };
        // start near where it will settle, instead of finding out from a few huge first pictures
        if smooth {
            r.q = p.jpeg.min(45);
            r.w = width_for(r.bytes_per_s() / 30.0, r.q, p.max_w);
        } else {
            r.q = p.jpeg.min(75);
            // the largest size whose pictures fit at the lowest sharp quality: native resolution
            // whenever the bitrate allows it, even at a few pictures a second
            r.w = width_for(r.bytes_per_s() / r.sharp_fps(), Q_MIN_SHARP, p.max_w).max(SHARP_MIN_W.min(p.max_w));
        }
        r
    }

    /// A measured line: Auto goes to 80 % of it (20 % kept free: other uploads, the measuring's own
    /// error); on a slow line it never climbs past that.
    fn set_line(&mut self, kbps: u32) {
        self.line = kbps.max(200);
        self.kbps = self.line * 4 / 5;
    }

    /// Pictures a second "Sharp" plans for: a few clear ones rather than many blurry ones.
    fn sharp_fps(&self) -> f64 {
        (self.kbps as f64 / 1500.0).clamp(2.0, 10.0)
    }

    /// Lowest quality before the picture gets narrower instead. Smooth lets tiny pictures go very
    /// low but shrinks a large one first: a 1280 px picture at quality 15 is worse than 854 px at 30.
    fn q_min(&self) -> u8 {
        if !self.smooth {
            Q_MIN_SHARP
        } else if self.w >= 854 {
            25
        } else if self.w >= 480 {
            12
        } else {
            Q_MIN_SMOOTH
        }
    }

    /// Auto: every half second, cut the bitrate when pictures queue up on the way (their delay
    /// climbs well above the connection's own) and raise it while they don't.
    fn tune(&mut self, fb: &parking_lot::Mutex<Feedback>) {
        if !(self.auto || self.h264) || self.tuned.elapsed() < Duration::from_millis(500) {
            return;
        }
        let secs = self.tuned.elapsed().as_secs_f64();
        self.tuned = Instant::now();
        let mut f = fb.lock();
        let got_kbps = f.delivered as f64 * 8.0 / 1000.0 / secs;
        f.delivered = 0;
        let (lat, base) = (if self.h264 && f.win_min > 0.0 { f.win_min } else { f.lat }, f.base.max(1.0));
        f.win_min = 0.0;
        // pictures still on their way for over a second: the link is saturated
        let stuck = f.sent.front().is_some_and(|(t, _)| t.elapsed() > Duration::from_millis(1000));
        drop(f);
        let k = self.kbps as f64;
        let new = if stuck || lat > base * 1.8 + 80.0 {
            // H.264 sends only what the picture needs (a still screen: little), so what got through
            // says nothing about the line: a quarter less (it fell from 80 to 6 Mb/s at one hiccup)
            if self.h264 {
                k * 0.75
            } else {
                (k * 0.75).min(got_kbps.max(150.0) * 0.9)
            }
        } else if lat < base * 1.3 + 40.0 && (got_kbps > k * 0.5 || self.held_back()) {
            // clearly fast (pictures arrive as quickly as the connection allows): climb faster
            k * if lat < base * 1.1 + 15.0 { 1.3 } else { 1.15 } + 100.0
        } else {
            k
        };
        // H.264 is planned for 20 Mb/s at most: no point climbing past it; a set bitrate is the top
        let top = if !self.auto {
            self.max as f64
        } else if self.line > 0 && self.line < 3000 {
            (self.line * 4 / 5) as f64
        } else if self.h264 {
            20_000.0
        } else {
            80_000.0
        };
        self.kbps = new.clamp(150f64.min(top), top) as u32;
    }

    fn bytes_per_s(&self) -> f64 {
        self.kbps as f64 * 1000.0 / 8.0
    }

    /// Pictures a second to aim for: the wanted rate. "Sharp" may send fewer: the byte bucket holds
    /// back what the budget can't carry at its quality.
    fn fps(&self, wanted: u32) -> u32 {
        wanted
    }

    /// Room for another picture now.
    fn may_send(&mut self) -> bool {
        let dt = self.at.elapsed().as_secs_f64();
        self.at = Instant::now();
        self.bucket = (self.bucket + dt * self.bytes_per_s()).min(self.bytes_per_s() * 0.25);
        self.bucket >= 0.0
    }

    /// A picture was handed to the encoder: set its likely size aside now.
    fn reserve(&mut self) {
        self.bucket -= self.last_n;
        self.reserved.push_back(self.last_n);
    }

    /// Quality or size are being held under what the Picture setting allows.
    fn held_back(&self) -> bool {
        self.q + 5 < self.q_max || self.w < self.max_w
    }

    /// A picture of `n` bytes went out. True when the picture width changed.
    fn sent(&mut self, n: usize, wanted_fps: u32) -> bool {
        // budget left unused (the screen changes less often than the frame rate allows) before
        // this picture: there's room for a better one
        // the real size replaces what was set aside for it
        let held = self.reserved.pop_front().unwrap_or(0.0);
        self.bucket += held;
        let unused = self.bucket >= self.bytes_per_s() * 0.2;
        self.bucket -= n as f64;
        self.last_n = n as f64;
        // each picture's share: smooth splits the budget over every frame; sharp only needs to
        // manage a few clear pictures a second
        let per_s = if self.smooth { self.fps(wanted_fps) as f64 } else { self.sharp_fps().min(wanted_fps as f64) };
        let target = self.bytes_per_s() / per_s;
        let r = n as f64 / target;
        if r > 1.15 && !unused {
            // far over: a big step (2x too big ≈ 8 lower)
            let step = ((r.ln() * 12.0).round() as u8).clamp(2, 20);
            self.q = self.q.saturating_sub(step).max(self.q_min());
        } else if r < 0.75 || unused {
            self.q = (self.q + 2).min(self.q_max);
        }
        // narrower soon after quality bottoms out, wider only after a calm while
        let since = self.resized.elapsed();
        let min_w = if self.smooth { 0 } else { SHARP_MIN_W };
        // straight to the width whose pictures fit (size goes with the area: width by √)
        let fitting = (self.w as f64 / r.max(1.0).sqrt() * 0.95) as u32;
        let smaller = WIDTHS
            .iter()
            .copied()
            .find(|&w| w < self.w && w <= fitting.max(1))
            .or_else(|| WIDTHS.iter().copied().find(|&w| w < self.w))
            // not below the floor: the narrowest allowed one instead
            .map(|w| if w < min_w { WIDTHS.iter().rev().copied().find(|&x| x < self.w && x >= min_w).unwrap_or(0) } else { w })
            .filter(|&w| w > 0);
        // wider: straight to the size the budget carries (Auto may have found a lot more room)
        let room = width_for(target, self.q_min().max(40), self.max_w);
        let larger = if room > self.w { Some(room) } else { WIDTHS.iter().rev().copied().find(|&w| w > self.w && w <= self.max_w) };
        // narrower when quality is at its floor, or straight away when pictures are several
        // times too big (quality alone would take many pictures to get there)
        self.over = if r > 3.0 { self.over.saturating_add(1) } else { 0 };
        let next = if (self.q <= self.q_min() + 2 && r > 1.2 || self.over >= 2) && !unused && since > Duration::from_millis(400) {
            smaller.map(|w| (w, (self.q + 12).min(self.q_max)))
        } else if since < Duration::from_millis(1500) {
            None
        } else if self.q >= self.q_max.saturating_sub(2) && (r < 0.55 || unused) {
            larger.map(|w| (w, self.q.saturating_sub(10).max(self.q_min())))
        } else {
            None
        };
        match next {
            Some((w, q)) => {
                (self.w, self.q, self.resized) = (w, q, Instant::now());
                true
            }
            None => false,
        }
    }
}

// ------------------------------------------------------------------ session

/// Pictures sent ahead of the phone's "shown" answers: covers the Wi-Fi round trip and the
/// phone's decoding, so the frame rate isn't set by the delay.
const IN_FLIGHT: i32 = 4;
/// H.264 pictures are small and many (and the phone answers each as it arrives): 4 on the way
/// held a phone on Wi-Fi with a ~100 ms round trip to ~40 a second, in bursts of 4.
const IN_FLIGHT_H264: i32 = 12;
/// Decoded in the phone page: answered on arrival while its decoder keeps up (8, answered once
/// decoded, held a phone with a ~200 ms Wi-Fi round trip to ~40 a second).
const IN_FLIGHT_SW: i32 = 12;

pub enum Out {
    Frame(Vec<u8>),
    Text(String),
}

enum Cmd {
    Monitor(usize),
    Quality(String),
    Fps(u32),
    Kbps(u32),
    Smooth(bool),
    /// the phone lost its place in the video: next picture is a key picture
    Key,
    /// the widest picture the phone's own player keeps up with (it drops pictures above it)
    MaxW(u32),
}

/// How pictures go to the phone: JPEG each, or H.264: decoded with WebCodecs where the phone's
/// browser has it (secure pages), else by the page's own decoder (WebAssembly, Constrained
/// Baseline), else through Media Source Extensions as fragmented MP4 (no WebAssembly).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Video {
    Jpeg,
    Wc,
    Sw,
    Mse,
}

impl Video {
    pub fn parse(s: &str) -> Video {
        match s {
            "h264w" => Video::Wc,
            "h264s" => Video::Sw,
            "h264m" => Video::Mse,
            _ => Video::Jpeg,
        }
    }
}

/// One phone's Screen view: a capture thread feeding `Out`s to the WebSocket.
pub struct Session {
    cmd: mpsc::Sender<Cmd>,
    /// pictures the phone is ready for (each "ack" adds one)
    credits: Arc<AtomicI32>,
    /// most pictures on the way (IN_FLIGHT, IN_FLIGHT_SW or IN_FLIGHT_H264)
    max: Arc<AtomicI32>,
    stop: Arc<AtomicBool>,
    /// the monitor being shown, for placing taps
    pub mon: Arc<parking_lot::Mutex<Option<Mon>>>,
    fb: Arc<parking_lot::Mutex<Feedback>>,
    /// when each line-measuring piece was answered
    pads: Arc<parking_lot::Mutex<Vec<(u32, Instant)>>>,
}

impl Session {
    pub fn pad_ack(&self, n: u32) {
        self.pads.lock().push((n, Instant::now()));
    }
    pub fn ack(&self) {
        self.fb.lock().acked();
        let max = self.max.load(Ordering::SeqCst);
        let c = self.credits.fetch_add(1, Ordering::SeqCst);
        if c >= max {
            self.credits.store(max, Ordering::SeqCst);
        }
    }
    pub fn monitor(&self, i: usize) {
        let _ = self.cmd.send(Cmd::Monitor(i));
    }
    pub fn quality(&self, q: &str) {
        let _ = self.cmd.send(Cmd::Quality(q.to_string()));
    }
    pub fn fps(&self, fps: u32) {
        let _ = self.cmd.send(Cmd::Fps(fps));
    }
    pub fn kbps(&self, kbps: u32) {
        let _ = self.cmd.send(Cmd::Kbps(kbps));
    }
    pub fn smooth(&self, on: bool) {
        let _ = self.cmd.send(Cmd::Smooth(on));
    }
    pub fn key(&self) {
        let _ = self.cmd.send(Cmd::Key);
    }
    pub fn max_w(&self, w: u32) {
        let _ = self.cmd.send(Cmd::MaxW(w));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn info(mons: &[Mon], i: usize, quality: &str, fps: u32, kbps: u32, smooth: bool) -> String {
    json!({
        "t": "info",
        "mon": i,
        "quality": quality,
        "fps": fps,
        "kbps": kbps,
        "priority": if smooth { "smooth" } else { "sharp" },
        "monitors": mons.iter().enumerate().map(|(n, m)| json!({ "i": n, "w": m.w, "h": m.h, "primary": m.primary })).collect::<Vec<_>>(),
    })
    .to_string()
}

pub fn start(quality: String, fps: u32, kbps: u32, smooth: bool, video: Video, away: bool, pad: bool, out: tokio::sync::mpsc::Sender<Out>) -> Session {
    let (tx, rx) = mpsc::channel();
    let n = match video {
        Video::Jpeg => IN_FLIGHT,
        Video::Sw => IN_FLIGHT_SW,
        _ => IN_FLIGHT_H264,
    };
    let credits = Arc::new(AtomicI32::new(n));
    let max = Arc::new(AtomicI32::new(n));
    let stop = Arc::new(AtomicBool::new(false));
    let mon = Arc::new(parking_lot::Mutex::new(None));
    let fb = Arc::new(parking_lot::Mutex::new(Feedback::default()));
    let pads = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (c2, x2, s2, m2, f2) = (credits.clone(), max.clone(), stop.clone(), mon.clone(), fb.clone());
    let p2 = pad.then(|| pads.clone());
    std::thread::Builder::new()
        .name("screencast".into())
        .spawn(move || run(quality, fps, kbps, smooth, (video, away, p2), out, rx, (c2, x2), s2, m2, f2))
        .ok();
    Session { cmd: tx, credits, max, stop, mon, fb, pads }
}

#[allow(clippy::too_many_arguments)]
fn run(mut quality: String, mut fps: u32, mut kbps: u32, mut smooth: bool, (mut video, away, pads): (Video, bool, Option<Arc<parking_lot::Mutex<Vec<(u32, Instant)>>>>), out: tokio::sync::mpsc::Sender<Out>, cmd: mpsc::Receiver<Cmd>, (credits, max_credits): (Arc<AtomicI32>, Arc<AtomicI32>), stop: Arc<AtomicBool>, cur_mon: Arc<parking_lot::Mutex<Option<Mon>>>, fb: Arc<parking_lot::Mutex<Feedback>>) {
    let mut mons = monitors();
    let mut idx = 0usize;
    let mut cap: Option<Capturer> = None;
    let mut last_frame = Instant::now() - Duration::from_secs(1);
    let mut last_cur = (i32::MIN, i32::MIN);
    let mut blocked = false;
    let mut rate = Rate::new(kbps, preset(&quality), smooth);
    rate.h264 = video != Video::Jpeg;
    // away (over Tailscale) the stream goes up the PC's own upload, often 1 Mb/s or less: Auto
    // starts there instead of at 4 Mb/s (which filled a DSL line's queue for seconds)
    if away && rate.auto {
        rate.kbps = 800;
    }
    // Auto measures the line first (the phone answers each piece as it arrives) and uses 80 % of it
    let mut scanned = Instant::now();
    if rate.auto {
        if let Some(p) = &pads {
            let _ = out.blocking_send(Out::Text(json!({ "t": "msg", "text": "Measuring your connection…" }).to_string()));
            if let Some(line) = scan_line(&out, p, 0) {
                rate.set_line(line);
            }
            let _ = out.blocking_send(Out::Text(json!({ "t": "msg", "text": "" }).to_string()));
            scanned = Instant::now();
        }
    }
    // H.264's share: what's left after the phone's sound (both go up the same line), 90 % of it
    let video_kbps = |k: u32| ((k.min(20_000).saturating_sub(SOUND_KBPS.load(Ordering::Relaxed))) * 9 / 10).max(100);
    let mut shown_rate = (0u32, 0u8, 0u32);
    let send = |o: Out| out.blocking_send(o).is_ok();
    if !send(Out::Text(info(&mons, idx, &quality, fps, kbps, smooth))) {
        return;
    }
    // where the time goes, every 2 s ({"t":"diag"}: the phone ignores it, test tools print it)
    let busy = Arc::new(AtomicI32::new(0));
    let mut diag = [0u32; 8]; // grabs, new, gdi, none, no credit, encoder busy, gap, bucket
    let mut grab_ms = 0f64;
    let mut diag_at = Instant::now();
    let (size_tx, size_rx) = mpsc::channel::<usize>();
    let mut enc_txs: Vec<mpsc::SyncSender<(Shot, u8, bool)>> = Vec::new();
    let mut h264_tx: Option<mpsc::SyncSender<H264In>> = None;
    let (fail_tx, fail_rx) = mpsc::channel::<String>();
    if video == Video::Jpeg {
        enc_txs = spawn_jpeg(&out, &fb, &busy, &credits, &size_tx);
    } else {
        h264_tx = Some(spawn_h264(video, &out, &fb, &busy, &credits, fail_tx));
    }
    let mut want_key = false;
    // H.264: the picture width follows the bitrate (narrower after 1 s of needing it, wider only
    // after 10 s, so the picture doesn't flip back and forth), and the frame rate where Sharp lowers it
    let (mut plan_w, mut plan_fps, mut plan_at, mut want_w) = (0u32, fps, Instant::now() - Duration::from_secs(1), (0u32, Instant::now()));
    let mut phone_max_w = u32::MAX;
    if !send(Out::Text(json!({ "t": "video", "v": if video == Video::Jpeg { "jpeg" } else { "pending" } }).to_string())) {
        return;
    }
    let mut next_enc = 0usize;
    while !stop.load(Ordering::SeqCst) {
        while let Ok(c) = cmd.try_recv() {
            match c {
                Cmd::Monitor(i) => idx = i,
                Cmd::Quality(q) => quality = q,
                Cmd::Fps(f) => fps = f,
                Cmd::Kbps(k) => kbps = k,
                Cmd::Smooth(s) => smooth = s,
                Cmd::Key => {
                    want_key = true;
                    continue;
                }
                Cmd::MaxW(w) => {
                    phone_max_w = w.max(256);
                    (plan_w, plan_at) = (0, Instant::now() - Duration::from_secs(1));
                    continue;
                }
            }
            // planned again right away (else the first pictures go out at the smallest size)
            (plan_w, plan_at) = (0, Instant::now() - Duration::from_secs(1));
            // Auto keeps what it learned about the connection
            let learned = rate.auto.then_some(rate.kbps);
            let line = rate.line;
            rate = Rate::new(kbps, preset(&quality), smooth);
            rate.h264 = video != Video::Jpeg;
            rate.line = line;
            if let (true, Some(k)) = (rate.auto, learned) {
                rate.kbps = k;
            } else if rate.auto && line > 0 {
                rate.kbps = line * 4 / 5;
            }
            mons = monitors();
            cap = None;
            last_cur = (i32::MIN, i32::MIN);
            if !send(Out::Text(info(&mons, idx.min(mons.len().saturating_sub(1)), &quality, fps, kbps, smooth))) {
                return;
            }
        }
        // an H.264 encoder that can't work: back to JPEG (the phone is told)
        if let Ok(why) = fail_rx.try_recv() {
            video = Video::Jpeg;
            h264_tx = None;
            max_credits.store(IN_FLIGHT, Ordering::SeqCst);
            credits.fetch_min(IN_FLIGHT, Ordering::SeqCst);
            enc_txs = spawn_jpeg(&out, &fb, &busy, &credits, &size_tx);
            rate = Rate::new(kbps, preset(&quality), smooth);
            cap = None;
            if !send(Out::Text(json!({ "t": "video", "v": "jpeg", "why": why }).to_string())) {
                return;
            }
        }
        // sizes of the pictures the encoder sent: the bitrate control follows them
        let mut resized = false;
        while let Ok(n) = size_rx.try_recv() {
            resized |= rate.sent(n, fps);
        }
        if resized {
            cap = None;
        }
        if video != Video::Jpeg && plan_at.elapsed() >= Duration::from_millis(500) {
            plan_at = Instant::now();
            if let Some(m) = mons.get(idx).or(mons.first()) {
                let (pw, pf) = h264::plan(video_kbps(rate.kbps), fps, smooth, preset(&quality).max_w.min(if video == Video::Wc { 2560 } else { 1280 }).min(phone_max_w), (m.w.max(2) as u32, m.h.max(2) as u32));
                plan_fps = pf;
                if plan_w == 0 {
                    plan_w = pw;
                    cap = None;
                } else if pw == plan_w {
                    want_w = (pw, Instant::now());
                } else {
                    if want_w.0 != pw {
                        want_w = (pw, Instant::now());
                    }
                    // (the browser's own player restarts its decoder at each change: it waits longer)
                    let hold = match (pw < plan_w, video == Video::Mse) {
                        (true, false) => Duration::from_secs(1),
                        (true, true) => Duration::from_secs(3),
                        (false, false) => Duration::from_secs(10),
                        (false, true) => Duration::from_secs(20),
                    };
                    if want_w.1.elapsed() >= hold {
                        plan_w = pw;
                        cap = None;
                    }
                }
            }
        }
        if cap.is_none() {
            if mons.is_empty() {
                mons = monitors();
            }
            let Some(m) = mons.get(idx).or(mons.first()).copied() else {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            };
            *cur_mon.lock() = Some(m);
            let max_w = if video == Video::Jpeg { rate.w } else { plan_w.max(256) };
            cap = Some(Capturer::new(m, Preset { max_w, jpeg: rate.q }));
        }
        let m = cap.as_ref().unwrap().mon;
        // what the limit settled on, for the phone's status line
        let (w, q, k) = (fit(m.w, m.h, if video == Video::Jpeg { rate.w } else { plan_w.max(256) }).0, rate.q, rate.kbps);
        if (w, q / 5, k / 100) != (shown_rate.0, shown_rate.1 / 5, shown_rate.2 / 100) {
            shown_rate = (w, q, k);
            if !send(Out::Text(json!({ "t": "rate", "w": w, "q": q, "fps": if video == Video::Jpeg { rate.fps(fps) } else { plan_fps }, "kbps": k, "auto": rate.auto, "line": rate.line, "codec": if video == Video::Jpeg { "jpeg" } else { "h264" } }).to_string())) {
                return;
            }
        }
        // the pointer, drawn by the phone (captures don't include it)
        let mut pt = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut pt) } != 0 && (pt.x, pt.y) != last_cur {
            last_cur = (pt.x, pt.y);
            let inside = pt.x >= m.left && pt.y >= m.top && pt.x < m.left + m.w && pt.y < m.top + m.h;
            let msg = if inside { json!({ "t": "cur", "x": (pt.x - m.left) as f64 / m.w as f64, "y": (pt.y - m.top) as f64 / m.h as f64 }) } else { json!({ "t": "cur", "x": -1 }) };
            if !send(Out::Text(msg.to_string())) {
                return;
            }
        }
        rate.tune(&fb);
        // measured again every 5 minutes (what else uses the line changes)
        if rate.auto && scanned.elapsed() > Duration::from_secs(300) {
            scanned = Instant::now();
            if let Some(line) = pads.as_ref().and_then(|p| scan_line(&out, p, 1000)) {
                rate.set_line(line);
            }
        }
        let aim = if video == Video::Jpeg { rate.fps(fps) } else { plan_fps };
        let bucket_ok = video != Video::Jpeg || rate.may_send();
        let encoder_free = busy.load(Ordering::SeqCst) < 2;
        // Pictures long on their way: the line holds a queue of them (a bitrate above what it
        // carries); no more until it drains. Counting pictures alone let 12 large ones sit ~0.6 s in
        // a home router, and the phone's sound (same router) waited behind them and ran dry.
        let backlog = {
            let f = fb.lock();
            f.sent.front().is_some_and(|(t, _)| t.elapsed() > Duration::from_millis((f.base as u64).min(300) + 100))
        };
        let has_credit = credits.load(Ordering::SeqCst) > 0 && !backlog;
        let gap_ok = last_frame.elapsed() >= gap(aim);
        if !has_credit {
            diag[4] += 1;
        } else if !encoder_free {
            diag[5] += 1;
        } else if !gap_ok {
            diag[6] += 1;
        }
        if encoder_free && has_credit && gap_ok && !bucket_ok {
            diag[7] += 1;
        }
        if diag_at.elapsed() >= Duration::from_secs(2) {
            let d = json!({ "t": "diag", "grabs": diag[0], "new": diag[1], "gdi": diag[2], "none": diag[3], "no_credit": diag[4], "enc_busy": diag[5], "gap": diag[6], "bucket": diag[7], "grab_ms": if diag[0] > 0 { grab_ms / diag[0] as f64 } else { 0.0 }, "gpu": cap.as_ref().is_some_and(|c| c.dup.is_some()), "lat": fb.lock().lat.round(), "base": fb.lock().base.round(), "credits": credits.load(Ordering::SeqCst) });
            if !send(Out::Text(d.to_string())) {
                return;
            }
            (diag, grab_ms, diag_at) = ([0; 8], 0.0, Instant::now());
        }
        if encoder_free && has_credit && gap_ok && bucket_ok {
            let t = Instant::now();
            let got = cap.as_mut().unwrap().grab();
            grab_ms += t.elapsed().as_secs_f64() * 1000.0;
            diag[0] += 1;
            match &got {
                Ok(Some(_)) => {
                    diag[1] += 1;
                    if cap.as_ref().is_some_and(|c| c.dup.is_none()) {
                        diag[2] += 1;
                    }
                }
                Ok(None) => diag[3] += 1,
                Err(()) => {}
            }
            match got {
                Ok(Some(shot)) => {
                    if blocked {
                        blocked = false;
                        if !send(Out::Text(json!({ "t": "msg", "text": "" }).to_string())) {
                            return;
                        }
                    }
                    credits.fetch_sub(1, Ordering::SeqCst);
                    // the gap counts from when this grab started, not when it finished (else
                    // every picture took the gap plus the ~6 ms grab: ~45 a second for 60)
                    last_frame = t;
                    busy.fetch_add(1, Ordering::SeqCst);
                    if let Some(tx) = &h264_tx {
                        let f = H264In { shot, key: std::mem::take(&mut want_key), kbps: video_kbps(rate.kbps), fps };
                        if tx.send(f).is_err() {
                            return;
                        }
                    } else {
                        rate.reserve();
                        if enc_txs[next_enc % 2].send((shot, rate.q, !smooth)).is_err() {
                            return;
                        }
                        next_enc += 1;
                    }
                }
                Ok(None) => {}
                Err(()) => {
                    if !blocked {
                        blocked = true;
                        if !send(Out::Text(json!({ "t": "msg", "text": "The PC is locked, or an admin prompt is open. Windows hides those screens; the picture comes back when it's gone." }).to_string())) {
                            return;
                        }
                    }
                    cap = None;
                    std::thread::sleep(Duration::from_millis(800));
                }
            }
        }
        // a picture never answered (the phone went to sleep mid-stream): stop waiting for it
        if fb.lock().sent.front().is_some_and(|(t, _)| t.elapsed() > Duration::from_secs(3)) {
            fb.lock().sent.pop_front();
            if credits.load(Ordering::SeqCst) < max_credits.load(Ordering::SeqCst) {
                credits.fetch_add(1, Ordering::SeqCst);
            }
        }
        // the DXGI wait paces the loop; GDI, a busy encoder, waiting on the phone or on the frame
        // gap need a short sleep
        if !encoder_free {
            std::thread::sleep(Duration::from_millis(1));
        } else if !has_credit || cap.as_ref().is_some_and(|c| c.dup.is_none()) {
            std::thread::sleep(Duration::from_millis(4));
        } else if last_frame.elapsed() < gap(aim) || !bucket_ok {
            std::thread::sleep((gap(aim).saturating_sub(last_frame.elapsed())).clamp(Duration::from_millis(1), Duration::from_millis(4)));
        }
    }
}

// ------------------------------------------------------------------ video encoders

/// Two threads JPEG-encode alternate pictures while the next one is captured (at 1920 px capture
/// takes ~5 ms and encoding 9-16, so one thread doing everything topped out around 35 a second). A
/// sender thread puts them back in order. At most two wait or encode at once; each picture's size
/// goes back to the bitrate control.
fn spawn_jpeg(out: &tokio::sync::mpsc::Sender<Out>, fb: &Arc<parking_lot::Mutex<Feedback>>, busy: &Arc<AtomicI32>, credits: &Arc<AtomicI32>, size_tx: &mpsc::Sender<usize>) -> Vec<mpsc::SyncSender<(Shot, u8, bool)>> {
    let mut enc_txs = Vec::new();
    let mut done_rxs = Vec::new();
    for i in 0..2 {
        let (tx, rx) = mpsc::sync_channel::<(Shot, u8, bool)>(1);
        let (dtx, drx) = mpsc::channel::<Option<Vec<u8>>>();
        std::thread::Builder::new()
            .name(format!("screencast-jpeg{i}"))
            .spawn(move || {
                for (shot, q, full) in rx {
                    let j = jpeg_with(&shot, q, full).map(|j| stamped(shot.ts, &j));
                    if dtx.send(j).is_err() {
                        break;
                    }
                }
            })
            .ok();
        enc_txs.push(tx);
        done_rxs.push(drx);
    }
    let (out, fb, busy, credits, size_tx) = (out.clone(), fb.clone(), busy.clone(), credits.clone(), size_tx.clone());
    std::thread::Builder::new()
        .name("screencast-send".into())
        .spawn(move || {
            // pictures went out to encoder 0, 1, 0, 1…: take them back in the same order
            for k in 0.. {
                let Ok(done) = done_rxs[k % 2].recv() else { break };
                match done {
                    Some(b) => {
                        let n = b.len();
                        fb.lock().sent.push_back((Instant::now(), n));
                        if out.blocking_send(Out::Frame(b)).is_err() {
                            break;
                        }
                        let _ = size_tx.send(n);
                    }
                    // not sent: the phone won't answer it, so its credit comes back now
                    None => {
                        credits.fetch_add(1, Ordering::SeqCst);
                    }
                }
                busy.fetch_sub(1, Ordering::SeqCst);
            }
        })
        .ok();
    enc_txs
}

/// How fast the way to the phone is (kb/s): two trains of 8 KB pieces sent back to back; the phone
/// answers each as it arrives, and the spread of the answers to the second train is how long the
/// slowest link took to carry it (the first gets TCP past its slow start). None when the phone
/// didn't answer in 4 s.
fn scan_line(out: &tokio::sync::mpsc::Sender<Out>, pads: &parking_lot::Mutex<Vec<(u32, Instant)>>, first: u32) -> Option<u32> {
    const SIZE: usize = 8000;
    let filler = "x".repeat(SIZE);
    let train = |from: u32, n: u32| -> Option<Vec<(u32, Instant)>> {
        pads.lock().clear();
        for i in from..from + n {
            out.blocking_send(Out::Text(json!({ "t": "pad", "n": i, "x": filler }).to_string())).ok()?;
        }
        let until = Instant::now() + Duration::from_secs(4);
        while pads.lock().len() < n as usize {
            if Instant::now() > until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut p = pads.lock().clone();
        p.sort_by_key(|x| x.0);
        Some(p)
    };
    train(first, 6)?;
    let p = train(first + 6, 8)?;
    let spread = p.last()?.1.duration_since(p.first()?.1).as_secs_f64().max(0.002);
    Some(((SIZE * 7 * 8) as f64 / spread / 1000.0).min(100_000.0) as u32)
}

/// One MSE fragment to the phone (kind 3/4), lasting `dur` (90 kHz).
fn frag(out: &tokio::sync::mpsc::Sender<Out>, fb: &Arc<parking_lot::Mutex<Feedback>>, mp4: &mut h264::Mp4, media_at: &mut u64, p: &h264::Packet, dur: u32) -> bool {
    let mut body = vec![if p.key { 4u8 } else { 3 }];
    body.extend(mp4.fragment(&p.data, p.key, *media_at, dur));
    *media_at += dur as u64;
    let msg = stamped(p.ts_ms, &body);
    fb.lock().sent.push_back((Instant::now(), msg.len()));
    out.blocking_send(Out::Frame(msg)).is_ok()
}

struct H264In {
    shot: Shot,
    key: bool,
    kbps: u32,
    /// the frame rate the stream is set up for
    fps: u32,
}

/// The H.264 encoder thread: picture in, packets out. Each message to the phone is: capture time
/// (8 bytes), a kind byte, then the data. Kind 0/1: delta/key picture (Annex-B, WebCodecs);
/// 2: MP4 header; 3/4: delta/key fragment (MSE). A {"t":"video"} text message before the first
/// picture of each stream (and after each size change) carries the codec string and size.
/// A hardware encoder that fails is replaced by the next one, then JPEG (`fail`).
fn spawn_h264(video: Video, out: &tokio::sync::mpsc::Sender<Out>, fb: &Arc<parking_lot::Mutex<Feedback>>, busy: &Arc<AtomicI32>, credits: &Arc<AtomicI32>, fail: mpsc::Sender<String>) -> mpsc::SyncSender<H264In> {
    let (tx, rx) = mpsc::sync_channel::<H264In>(1);
    let (out, fb, busy, credits) = (out.clone(), fb.clone(), busy.clone(), credits.clone());
    std::thread::Builder::new()
        .name("screencast-h264".into())
        .spawn(move || {
            let mse = video == Video::Mse;
            let cands = h264::candidates();
            let mut ci = 0usize;
            let mut last_err = String::new();
            let mut enc: Option<(h264::Encoder, u32, u32, u32)> = None;
            let mut mp4 = h264::Mp4::new();
            let mut announced = false;
            let mut media_at = 0u64;
            let mut recent: std::collections::VecDeque<Instant> = Default::default();
            // MSE: each picture lasts until the next one was captured (the video plays at the PC's
            // own pace), so the latest waits for its successor. A still screen sends no pictures:
            // the last one is sent again every 80 ms (a few hundred bytes each), because the
            // phone's player must never run out. Firefox drains its decoder when it runs out and
            // then decodes again from the last key picture, seconds back, showing old pictures.
            let mut pend: Option<h264::Packet> = None;
            let mut last_nv12: Option<Vec<u8>> = None;
            // times given to the encoder only go forward (a repeat is stamped "now", and a picture
            // captured just before it can arrive just after)
            let mut last_ts = f64::MIN;
            loop {
                let job = match rx.recv_timeout(Duration::from_millis(80)) {
                    Ok(f) => Some(f),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if !mse || enc.is_none() || last_nv12.is_none() {
                            continue;
                        }
                        if credits.fetch_sub(1, Ordering::SeqCst) <= 0 {
                            // no room on the way: the waiting picture goes as it is
                            credits.fetch_add(1, Ordering::SeqCst);
                            if let Some(prev) = pend.take() {
                                if !frag(&out, &fb, &mut mp4, &mut media_at, &prev, 7200) {
                                    return;
                                }
                            }
                            continue;
                        }
                        None
                    }
                    Err(_) => break,
                };
                // a real picture: its credit and its place in `busy` were taken by the capture loop;
                // a repeat took a credit only
                let real = job.is_some();
                let done = |n: i32| {
                    credits.fetch_add(n, Ordering::SeqCst);
                    if real {
                        busy.fetch_sub(1, Ordering::SeqCst);
                    }
                };
                let (f_kbps, f_fps, f_key, f_ts, nv12) = match job {
                    Some(f) => {
                        let (w, h) = (f.shot.w, f.shot.h);
                        if w % 2 == 1 || h % 2 == 1 {
                            let _ = fail.send("odd picture size".into());
                            return;
                        }
                        if enc.as_ref().is_none_or(|e| (e.1, e.2) != (w, h)) {
                            if let Some(prev) = pend.take() {
                                if !frag(&out, &fb, &mut mp4, &mut media_at, &prev, 3000) {
                                    return;
                                }
                            }
                            enc = None;
                            announced = false;
                            while enc.is_none() {
                                let Some(c) = cands.get(ci) else {
                                    done(1);
                                    let _ = fail.send(format!("no H.264 encoder worked ({last_err})"));
                                    return;
                                };
                                match h264::Encoder::open_as(c, w, h, f.fps, f.kbps, video == Video::Sw) {
                                    Ok(e) => enc = Some((e, w, h, f.fps)),
                                    Err(er) => {
                                        last_err = format!("{}: {er} at {w}x{h} {}k", c.name, f.kbps);
                                        ci += 1
                                    }
                                }
                            }
                        }
                        let nv12 = h264::bgra_to_nv12(&f.shot.px, w as usize, h as usize);
                        (f.kbps, f.fps, f.key, f.shot.ts, nv12)
                    }
                    None => (0, 0, false, now_ms(), last_nv12.take().unwrap()),
                };
                let (w, h) = { let e = enc.as_ref().unwrap(); (e.1, e.2) };
                let (e, _, _, cfg_fps) = enc.as_mut().unwrap();
                // a stream that gets fewer pictures than it's set up for (a still screen) is given
                // proportionally more bits per picture: hardware encoders spend per nominal frame
                // (repeats of a still picture don't count)
                if real {
                    let now = Instant::now();
                    recent.push_back(now);
                    while recent.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(2)) {
                        recent.pop_front();
                    }
                    let span = recent.front().map(|t| now.duration_since(*t).as_secs_f64()).unwrap_or(0.0).max(0.5);
                    let scale = if recent.len() >= 6 { (*cfg_fps as f64 / (recent.len() as f64 / span)).clamp(1.0, 4.0) } else { 1.0 };
                    e.set_bitrate(((f_kbps as f64 * scale / 100.0).round() as u32 * 100).max(100));
                }
                let _ = f_fps;
                let f_ts = f_ts.max(last_ts + 0.1);
                last_ts = f_ts;
                let res = e.encode(&nv12, f_ts, f_key);
                if mse {
                    last_nv12 = Some(nv12);
                }
                match res {
                    Ok(pkts) => {
                        if pkts.is_empty() {
                            done(1);
                            continue;
                        }
                        // a repeat that came out as more than one picture: the extra ones take credits too
                        if !real && pkts.len() > 1 {
                            credits.fetch_sub(pkts.len() as i32 - 1, Ordering::SeqCst);
                        }
                        let mut ok = true;
                        for p in pkts {
                            if !announced {
                                if !p.key {
                                    continue;
                                }
                                let Some((sps, pps)) = e.params().cloned().or_else(|| h264::param_sets(&p.data)) else { continue };
                                let codec = h264::codec_string(&sps);
                                if !out.blocking_send(Out::Text(json!({ "t": "video", "v": match video { Video::Mse => "mse", Video::Sw => "sw", _ => "wc" }, "codec": codec, "w": w, "h": h, "hw": cands.get(ci).is_some_and(|c| c.hardware) }).to_string())).is_ok() {
                                    return;
                                }
                                if mse {
                                    let mut init = vec![2u8];
                                    init.extend(mp4.init(&sps, &pps, w, h));
                                    if out.blocking_send(Out::Frame(stamped(p.ts_ms, &init))).is_err() {
                                        return;
                                    }
                                }
                                announced = true;
                            }
                            if mse {
                                let ts = p.ts_ms;
                                if let Some(prev) = pend.replace(p) {
                                    let dur = (((ts - prev.ts_ms) * 90.0) as i64).clamp(90, 45_000) as u32;
                                    ok &= frag(&out, &fb, &mut mp4, &mut media_at, &prev, dur);
                                }
                                continue;
                            }
                            let mut body = Vec::with_capacity(p.data.len() + 1);
                            body.push(p.key as u8);
                            body.extend_from_slice(&p.data);
                            let msg = stamped(p.ts_ms, &body);
                            fb.lock().sent.push_back((Instant::now(), msg.len()));
                            ok &= out.blocking_send(Out::Frame(msg)).is_ok();
                        }
                        if !ok {
                            return;
                        }
                        if real {
                            busy.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    Err(er) => {
                        // this encoder broke: the next one takes over (a new stream, key picture first)
                        last_err = format!("{}: {er} ({}x{}{})", cands.get(ci).map(|c| c.name.as_str()).unwrap_or("?"), w, h, if real { "" } else { ", repeat" });
                        let _ = out.blocking_send(Out::Text(json!({ "t": "note", "text": format!("H.264 encoder error, trying the next one: {last_err}") }).to_string()));
                        ci += 1;
                        enc = None;
                        done(1);
                        if ci >= cands.len() {
                            let _ = fail.send(format!("the H.264 encoder stopped working ({last_err})"));
                            return;
                        }
                    }
                }
            }
        })
        .ok();
    tx
}

// ------------------------------------------------------------------ sound

/// What the PC plays, for the phone's Screen view (only while the phone asks for it): WASAPI
/// loopback of the default output. Follows a change of the default output device.
/// Each packet: its capture time (8 bytes, like the pictures), then format (0 = 16-bit PCM,
/// 1 = IMA ADPCM), channels, sample rate (u32), [ADPCM: each channel's start state], samples.
/// Quality: "best" (PCM, the PC's rate, stereo: ~1.5 Mb/s), "good" (ADPCM stereo: ~384 kb/s),
/// "medium" (ADPCM 24 kHz stereo: ~200 kb/s), "low" (ADPCM 16 kHz mono: ~70 kb/s, for slow
/// connections).
/// What the phones' sound streams use (kb/s): the pictures leave room for it.
static SOUND_KBPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn start_sound(quality: &str, out: tokio::sync::mpsc::Sender<Out>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let q = match quality {
        "good" => SoundQ::Good,
        "medium" => SoundQ::Medium,
        "low" => SoundQ::Low,
        _ => SoundQ::Best,
    };
    let k = match q {
        SoundQ::Best => 1600,
        SoundQ::Good => 420,
        SoundQ::Medium => 210,
        SoundQ::Low => 80,
    };
    std::thread::Builder::new()
        .name("screencast-sound".into())
        .spawn(move || {
            SOUND_KBPS.fetch_add(k, Ordering::Relaxed);
            sound_run(q, out, s2);
            SOUND_KBPS.fetch_sub(k, Ordering::Relaxed);
        })
        .ok();
    stop
}

#[derive(Clone, Copy, PartialEq)]
enum SoundQ {
    Best,
    Medium,
    Good,
    Low,
}

/// IMA ADPCM: 4 bits a sample (a quarter of 16-bit PCM), simple enough to decode in a few lines
/// of JavaScript on the phone.
const ADPCM_STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const ADPCM_INDEX: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

#[derive(Clone, Copy, Default)]
struct Adpcm {
    pred: i32,
    idx: i32,
}

impl Adpcm {
    fn enc(&mut self, s: i16) -> u8 {
        let step = ADPCM_STEPS[self.idx as usize];
        let mut diff = s as i32 - self.pred;
        let mut code = 0u8;
        if diff < 0 {
            code = 8;
            diff = -diff;
        }
        let mut vp = step >> 3;
        if diff >= step {
            code |= 4;
            diff -= step;
            vp += step;
        }
        if diff >= step >> 1 {
            code |= 2;
            diff -= step >> 1;
            vp += step >> 1;
        }
        if diff >= step >> 2 {
            code |= 1;
            vp += step >> 2;
        }
        self.pred = (if code & 8 != 0 { self.pred - vp } else { self.pred + vp }).clamp(-32768, 32767);
        self.idx = (self.idx + ADPCM_INDEX[code as usize]).clamp(0, 88);
        code
    }
}

/// One sound packet (see `start_sound`) from interleaved samples.
fn sound_packet(ts: f64, q: SoundQ, rate: u32, ch: usize, samples: &[i16], st: &mut [Adpcm; 2]) -> Vec<u8> {
    let mut v = Vec::with_capacity(16 + samples.len() * 2);
    v.extend_from_slice(&ts.to_le_bytes());
    v.push(if q == SoundQ::Best { 0 } else { 1 });
    v.push(ch as u8);
    v.extend_from_slice(&rate.to_le_bytes());
    if q == SoundQ::Best {
        v.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        return v;
    }
    for s in st.iter().take(ch) {
        v.extend_from_slice(&(s.pred as i16).to_le_bytes());
        v.push(s.idx as u8);
        v.push(0);
    }
    if ch == 2 {
        // one byte per frame: left in the low half, right in the high half
        for f in samples.as_chunks::<2>().0 {
            v.push(st[0].enc(f[0]) | (st[1].enc(f[1]) << 4));
        }
    } else {
        for p in samples.chunks(2) {
            let a = st[0].enc(p[0]);
            let b = p.get(1).map(|&s| st[0].enc(s)).unwrap_or(0);
            v.push(a | (b << 4));
        }
    }
    v
}

struct SoundDev {
    client: windows::Win32::Media::Audio::IAudioClient,
    capture: windows::Win32::Media::Audio::IAudioCaptureClient,
    id: String,
    rate: u32,
    channels: usize,
    float: bool,
}

fn default_output_id() -> Option<String> {
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id = dev.GetId().ok()?;
        let s = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const _));
        s
    }
}

fn open_sound() -> Option<SoundDev> {
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id = dev.GetId().ok().and_then(|p| {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        })?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).ok()?;
        let fmt = client.GetMixFormat().ok()?;
        let (channels, bits, rate) = ((*fmt).nChannels.max(1) as usize, (*fmt).wBitsPerSample, (*fmt).nSamplesPerSec);
        let ok = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None).is_ok();
        CoTaskMemFree(Some(fmt as *const _));
        if !ok || !(bits == 32 || bits == 16) {
            return None;
        }
        let capture: IAudioCaptureClient = client.GetService().ok()?;
        client.Start().ok()?;
        Some(SoundDev { client, capture, id, rate, channels, float: bits == 32 })
    }
}

fn sound_run(q: SoundQ, out: tokio::sync::mpsc::Sender<Out>, stop: Arc<AtomicBool>) {
    use windows::Win32::Media::Audio::AUDCLNT_BUFFERFLAGS_SILENT;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut dev: Option<SoundDev> = None;
    let mut checked = Instant::now();
    let mut pcm: Vec<i16> = Vec::with_capacity(4096);
    // capture time of pcm's first sample (ms, see `qpc_ms`)
    let mut pcm_ts = 0f64;
    let mut st = [Adpcm::default(); 2];
    // "medium" (24 kHz stereo) and "low" (16 kHz mono): fewer samples a second, made by averaging
    // the PC's samples (a simple low-pass) at fractional steps
    let down = match q {
        SoundQ::Medium => Some((24_000u32, 2usize)),
        SoundQ::Low => Some((16_000, 1)),
        _ => None,
    };
    let mut acc: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
    let mut acc_pos = 0f64;
    let mut low: Vec<i16> = Vec::new();
    // WASAPI loopback sends nothing while the PC is silent, so the phone's queue would run dry
    // and count a "dropout" at every pause in the sound: the first 1.5 s of silence is sent as
    // silence instead (after that the phone knows it's a real pause)
    let mut last_data = Instant::now();
    let mut filled = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if dev.is_none() {
            dev = open_sound();
            match &dev {
                Some(d) => {
                    if out.blocking_send(Out::Text(json!({ "t": "fmt", "rate": d.rate, "ch": 2, "q": match q { SoundQ::Best => "best", SoundQ::Good => "good", SoundQ::Medium => "medium", SoundQ::Low => "low" } }).to_string())).is_err() {
                        return;
                    }
                }
                None => {
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }
            }
        }
        // the default output changed (headphones plugged in, picked in Windows): follow it
        if checked.elapsed() > Duration::from_secs(2) {
            checked = Instant::now();
            if default_output_id().is_some_and(|id| dev.as_ref().is_some_and(|d| d.id != id)) {
                dev = None;
                continue;
            }
        }
        let d = dev.as_ref().unwrap();
        let mut failed = false;
        let mut got = false;
        unsafe {
            loop {
                let n = match d.capture.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                if n == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let (mut frames, mut flags, mut qpc) = (0u32, 0u32, 0u64);
                if d.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc)).is_err() {
                    failed = true;
                    break;
                }
                got = true;
                let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null();
                if pcm.is_empty() && (down.is_none() || low.is_empty() && acc[0].is_empty()) {
                    // WASAPI's stamp is in 100 ns units of the same counter
                    pcm_ts = if qpc > 0 { qpc as f64 / 10_000.0 } else { now_ms() };
                }
                for f in 0..frames as usize {
                    for c in 0..2 {
                        // mono plays on both sides; surround keeps its front left and right
                        let ch = c.min(d.channels - 1);
                        let v = if silent {
                            0.0
                        } else if d.float {
                            *(data as *const f32).add(f * d.channels + ch)
                        } else {
                            *(data as *const i16).add(f * d.channels + ch) as f32 / 32768.0
                        };
                        pcm.push((v.clamp(-1.0, 1.0) * 32767.0) as i16);
                    }
                }
                let _ = d.capture.ReleaseBuffer(frames);
            }
        }
        if failed {
            dev = None;
            pcm.clear();
            continue;
        }
        if got {
            last_data = Instant::now();
            filled = Instant::now();
        } else if q != SoundQ::Low && last_data.elapsed() > Duration::from_millis(40) && last_data.elapsed() < Duration::from_millis(1500) {
            // real silence: what is missing since the last data or fill, as zeros
            let frames = (filled.elapsed().as_secs_f64() * d.rate as f64) as usize;
            if frames > 0 {
                pcm.extend(std::iter::repeat_n(0i16, frames * 2));
                filled = Instant::now();
            }
        }
        if let Some((to, chs)) = down {
            // to `to` samples a second (Low: one channel); packets of 20 ms
            let ratio = d.rate as f64 / to as f64;
            let start_ts = pcm_ts - acc[0].len() as f64 * 1000.0 / d.rate as f64;
            for f in pcm.as_chunks::<2>().0 {
                if chs == 1 {
                    acc[0].push((f[0] as f32 + f[1] as f32) * 0.5);
                } else {
                    acc[0].push(f[0] as f32);
                    acc[1].push(f[1] as f32);
                }
            }
            pcm.clear();
            while acc_pos + ratio < acc[0].len() as f64 {
                let (a, b) = (acc_pos as usize, ((acc_pos + ratio) as usize).max(acc_pos as usize + 1));
                for ch in acc.iter().take(chs) {
                    let avg = ch[a..b].iter().sum::<f32>() / (b - a) as f32;
                    low.push(avg.clamp(-32768.0, 32767.0) as i16);
                }
                acc_pos += ratio;
            }
            let used = acc_pos as usize;
            for ch in acc.iter_mut().take(chs) {
                ch.drain(..used);
            }
            acc_pos -= used as f64;
            let per = to as usize / 50 * chs;
            let mut ts = start_ts;
            while low.len() >= per {
                let chunk: Vec<i16> = low.drain(..per).collect();
                if out.blocking_send(Out::Frame(sound_packet(ts, q, to, chs, &chunk, &mut st))).is_err() {
                    return;
                }
                ts += 20.0;
            }
            pcm_ts = ts;
        } else {
            // 10 ms packets: small enough that the phone's buffer, not the packet size, sets the delay
            let packet = (d.rate as usize / 100) * 2;
            while pcm.len() >= packet {
                let chunk: Vec<i16> = pcm.drain(..packet).collect();
                if out.blocking_send(Out::Frame(sound_packet(pcm_ts, q, d.rate, 2, &chunk, &mut st))).is_err() {
                    return;
                }
                pcm_ts += (packet / 2) as f64 * 1000.0 / d.rate as f64;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(d) = dev {
        unsafe {
            let _ = d.client.Stop();
        }
    }
}

// ------------------------------------------------------------------ input

fn send_inputs(v: &[INPUT]) {
    if !v.is_empty() {
        unsafe { SendInput(v.len() as u32, v.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    }
}

fn mouse_input(flags: u32, dx: i32, dy: i32, data: u32) -> INPUT {
    INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

/// A point on the monitor (0..1 across and down) as SendInput's absolute coordinates, which span
/// the whole desktop (all monitors) as 0..65535.
fn abs_point(m: &Mon, x: f64, y: f64) -> (i32, i32) {
    let (vx, vy, vw, vh) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2), GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2)) };
    let px = m.left as f64 + x.clamp(0.0, 1.0) * (m.w - 1) as f64;
    let py = m.top as f64 + y.clamp(0.0, 1.0) * (m.h - 1) as f64;
    ((((px - vx as f64) * 65535.0) / (vw - 1) as f64).round() as i32, (((py - vy as f64) * 65535.0) / (vh - 1) as f64).round() as i32)
}

/// Key names the phone sends: (virtual key, extended key).
fn vk(name: &str) -> Option<(u16, bool)> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "enter" => (0x0D, false),
        "backspace" => (0x08, false),
        "tab" => (0x09, false),
        "esc" => (0x1B, false),
        "space" => (0x20, false),
        "delete" => (0x2E, true),
        "insert" => (0x2D, true),
        "home" => (0x24, true),
        "end" => (0x23, true),
        "pageup" => (0x21, true),
        "pagedown" => (0x22, true),
        "left" => (0x25, true),
        "up" => (0x26, true),
        "right" => (0x27, true),
        "down" => (0x28, true),
        "win" => (0x5B, true),
        "ctrl" => (0x11, false),
        "alt" => (0x12, false),
        "shift" => (0x10, false),
        "printscreen" => (0x2C, true),
        "volup" => (0xAF, true),
        "voldown" => (0xAE, true),
        "mute" => (0xAD, true),
        _ => {
            let b = n.as_bytes();
            if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
                (b[0].to_ascii_uppercase() as u16, false)
            } else {
                let f = n.strip_prefix('f').and_then(|f| f.parse::<u16>().ok()).filter(|f| (1..=12).contains(f))?;
                (0x70 + f - 1, false)
            }
        }
    })
}

fn key_event(vk_code: u16, ext: bool, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk_code as u32, MAPVK_VK_TO_VSC) } as u16;
    key_input(vk_code, scan, if ext { KEYEVENTF_EXTENDEDKEY } else { 0 } | if up { KEYEVENTF_KEYUP } else { 0 })
}

/// Applies one input message from the phone on monitor `m`:
/// mm (move), md / mu (button down / up; b 0 left, 1 right, 2 middle), wh (wheel, 120 per notch),
/// txt (type text), key (a key with optional mods: ctrl, alt, shift, win).
pub fn input(m: &Mon, v: &Value) {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
    let mv = || {
        let (ax, ay) = abs_point(m, f("x"), f("y"));
        mouse_input(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, ax, ay, 0)
    };
    match v["t"].as_str().unwrap_or("") {
        "mm" => send_inputs(&[mv()]),
        t @ ("md" | "mu") => {
            let down = t == "md";
            let flag = match v["b"].as_u64().unwrap_or(0) {
                1 => if down { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP },
                2 => if down { MOUSEEVENTF_MIDDLEDOWN } else { MOUSEEVENTF_MIDDLEUP },
                _ => if down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP },
            };
            send_inputs(&[mv(), mouse_input(flag, 0, 0, 0)]);
        }
        "wh" => {
            let mut ev = Vec::new();
            let (dy, dx) = (f("dy").round() as i32, f("dx").round() as i32);
            if dy != 0 {
                ev.push(mouse_input(MOUSEEVENTF_WHEEL, 0, 0, dy.clamp(-2400, 2400) as u32));
            }
            if dx != 0 {
                ev.push(mouse_input(MOUSEEVENTF_HWHEEL, 0, 0, dx.clamp(-2400, 2400) as u32));
            }
            send_inputs(&ev);
        }
        "txt" => {
            let mut ev = Vec::new();
            for ch in v["s"].as_str().unwrap_or("").chars().take(2000) {
                if ch == '\n' {
                    ev.push(key_event(0x0D, false, false));
                    ev.push(key_event(0x0D, false, true));
                    continue;
                }
                let mut buf = [0u16; 2];
                for u in ch.encode_utf16(&mut buf) {
                    ev.push(key_input(0, *u, KEYEVENTF_UNICODE));
                    ev.push(key_input(0, *u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
            send_inputs(&ev);
        }
        "key" => {
            let Some((k, ext)) = v["k"].as_str().and_then(vk) else { return };
            let mods: Vec<(u16, bool)> = v["mods"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().and_then(vk)).collect()).unwrap_or_default();
            let mut ev: Vec<INPUT> = mods.iter().map(|(m, e)| key_event(*m, *e, false)).collect();
            ev.push(key_event(k, ext, false));
            ev.push(key_event(k, ext, true));
            ev.extend(mods.iter().rev().map(|(m, e)| key_event(*m, *e, true)));
            send_inputs(&ev);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_small_screens_and_shrinks_wide_ones() {
        assert_eq!(fit(1280, 720, 1280), (1280, 720));
        assert_eq!(fit(1920, 1080, 1280), (1280, 720));
        assert_eq!(fit(3440, 1440, 1920), (1920, 802));
    }

    #[test]
    fn shrink_averages() {
        let s = Shot { px: [[10u8, 20, 30, 255], [30, 40, 50, 255]].repeat(8).concat(), w: 4, h: 4, ts: 0.0 };
        let o = shrink(&s, 2, 2);
        assert_eq!((o.w, o.h, o.px.len()), (2, 2, 16));
        assert_eq!(&o.px[..3], &[20, 30, 40]);
    }

    #[test]
    fn sharp_keeps_the_picture_and_drops_frames() {
        let p = preset("sharp");
        let mut r = Rate::new(8_000, p, false);
        // 8 Mb/s: plans ~5 clear pictures a second at full 1080p size
        assert!((r.sharp_fps() - 8_000.0 / 1500.0).abs() < 1e-9);
        assert!(r.q == 75 && r.w >= 1920, "{} {}", r.w, r.q);
        // pictures that fit (~180 KB = 5 a second): size and quality stay
        let (w, q) = (r.w, r.q);
        for _ in 0..10 {
            r.sent(180_000, 30);
        }
        assert_eq!((r.w, r.q), (w, q));
        // the budget is spent: the bucket holds the next picture back (fewer frames, not worse)
        r.bucket = 0.0;
        r.at = Instant::now();
        r.sent(95_000, 30);
        assert!(!r.may_send());
        // 1 Mb/s: plans 2 a second and starts small enough for them, never under 480
        let slow = Rate::new(1_000, p, false);
        assert_eq!(slow.sharp_fps(), 2.0);
        assert!(slow.w >= SHARP_MIN_W && slow.w < 1920, "{}", slow.w);
        // smooth keeps the frame rate, starts at a size its budget carries, and lets quality go lowest
        let s = Rate::new(2_000, p, true);
        assert_eq!((s.fps(60), s.q_min()), (60, Q_MIN_SMOOTH));
        assert!(s.w <= 640, "{}", s.w);
        // pictures far too big (5x), two in a row, shrink it straight away (one alone may be a scene cut)
        let mut big = Rate::new(8_000, p, true);
        big.resized = Instant::now() - Duration::from_secs(1);
        let w0 = big.w;
        // (or at once, when the first already took quality to its floor for that size)
        let first = big.sent(170_000, 30);
        assert!(first || big.sent(170_000, 30));
        assert!(big.w < w0);
    }

    #[test]
    fn smooth_shrinks_until_the_frame_rate_fits() {
        // 1 Mb/s at 60 fps: ~2 KB a picture. Pictures 8x too big jump several widths at once.
        let mut r = Rate::new(1_000, preset("balanced"), true);
        r.q = Q_MIN_SMOOTH;
        r.resized = Instant::now() - Duration::from_secs(1);
        r.bucket = 0.0;
        assert!(r.sent(16_000, 60));
        assert!(r.w <= 480, "{}", r.w);
        // sharp would stop at its floor and drop frames instead
        let mut sh = Rate::new(1_000, preset("balanced"), false);
        sh.w = SHARP_MIN_W;
        sh.q = Q_MIN_SHARP;
        sh.resized = Instant::now() - Duration::from_secs(5);
        assert!(!sh.sent(50_000, 60));
        assert_eq!(sh.w, SHARP_MIN_W);
    }

    /// How small the smallest pictures get (real screen): what 1 Mb/s at 30/60 fps can carry.
    #[test]
    #[ignore]
    fn tiny_picture_sizes() {
        let m = monitors()[0];
        for &w in &[480u32, 320, 256, 192, 160] {
            let (pw, ph) = fit(m.w, m.h, w);
            let shot = gdi_shot(&m, pw, ph).unwrap();
            for q in [4u8, 10, 20] {
                println!("{pw}x{ph} q{q}: {} B", jpeg(&shot, q).unwrap().len());
            }
        }
    }

    #[test]
    fn auto_bitrate_backs_off_and_recovers() {
        let fb = parking_lot::Mutex::new(Feedback::default());
        let mut r = Rate::new(0, preset("balanced"), true);
        assert!(r.auto && r.kbps == 4_000);
        // pictures arriving late (a queue on a slow line): cut toward what got through
        {
            let mut f = fb.lock();
            (f.lat, f.base, f.delivered) = (900.0, 60.0, 60_000);
        }
        r.tuned = Instant::now() - Duration::from_millis(600);
        r.tune(&fb);
        assert!(r.kbps < 1_000, "{}", r.kbps);
        // quick again and using what it has: climb
        let before = r.kbps;
        {
            let mut f = fb.lock();
            (f.lat, f.base, f.delivered) = (60.0, 60.0, 1_000_000);
        }
        r.tuned = Instant::now() - Duration::from_millis(600);
        r.tune(&fb);
        assert!(r.kbps > before);
    }

    #[test]
    fn adpcm_follows_the_signal() {
        // a 440 Hz tone survives encode → decode closely
        let mut enc = Adpcm::default();
        let mut dec = Adpcm::default();
        let mut worst = 0i32;
        for i in 0..4800 {
            let s = ((i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 12_000.0) as i16;
            let code = enc.enc(s);
            // the phone's decoder, in Rust
            let step = ADPCM_STEPS[dec.idx as usize];
            let mut vp = step >> 3;
            if code & 4 != 0 { vp += step; }
            if code & 2 != 0 { vp += step >> 1; }
            if code & 1 != 0 { vp += step >> 2; }
            dec.pred = (if code & 8 != 0 { dec.pred - vp } else { dec.pred + vp }).clamp(-32768, 32767);
            dec.idx = (dec.idx + ADPCM_INDEX[code as usize]).clamp(0, 88);
            if i > 200 {
                worst = worst.max((dec.pred - s as i32).abs());
            }
        }
        assert!(worst < 1500, "{worst}");
        // packet layout: time, format, channels, rate, states, samples
        let mut st = [Adpcm::default(); 2];
        let p = sound_packet(1.0, SoundQ::Low, 16_000, 1, &[0i16; 320], &mut st);
        assert_eq!(p.len(), 8 + 1 + 1 + 4 + 4 + 160);
        let p = sound_packet(1.0, SoundQ::Best, 48_000, 2, &[0i16; 960], &mut st);
        assert_eq!(p.len(), 8 + 1 + 1 + 4 + 1920);
    }

    #[test]
    fn key_names() {
        assert_eq!(vk("enter"), Some((0x0D, false)));
        assert_eq!(vk("C"), Some((0x43, false)));
        assert_eq!(vk("f5"), Some((0x74, false)));
        assert_eq!(vk("left"), Some((0x25, true)));
        assert_eq!(vk("f13"), None);
        assert_eq!(vk("nope"), None);
    }

    #[test]
    fn corners_map_to_the_monitor() {
        let m = Mon { handle: 0, left: 0, top: 0, w: 100, h: 100, primary: true };
        let (x0, y0) = abs_point(&m, 0.0, 0.0);
        let (x1, y1) = abs_point(&m, 1.0, 1.0);
        assert!(x0 <= x1 && y0 <= y1);
    }

    /// Where a frame's time goes: capture, shrink, JPEG; and how many new frames DXGI gives in 3 s.
    #[test]
    #[ignore]
    fn capture_timing() {
        let m = monitors()[0];
        let p = Preset { max_w: std::env::var("CAP_W").ok().and_then(|v| v.parse().ok()).unwrap_or(1280), jpeg: std::env::var("CAP_Q").ok().and_then(|v| v.parse().ok()).unwrap_or(75) };
        let (w, h) = fit(m.w, m.h, p.max_w);
        let mut d = Dup::new(&m, p.max_w).unwrap();
        let t0 = Instant::now();
        let (mut polls, mut new, mut cap_t, mut shrink_t, mut jpeg_t) = (0, 0, Duration::ZERO, Duration::ZERO, Duration::ZERO);
        while t0.elapsed() < Duration::from_secs(3) {
            polls += 1;
            let t = Instant::now();
            let Ok(Some(mut s)) = d.frame(15) else { continue };
            cap_t += t.elapsed();
            new += 1;
            let t = Instant::now();
            if s.w > w { s = shrink(&s, w, h); }
            shrink_t += t.elapsed();
            let t = Instant::now();
            let j = jpeg(&s, p.jpeg).unwrap();
            jpeg_t += t.elapsed();
            if new == 1 { println!("frame {}x{} -> {} KB", s.w, s.h, j.len() / 1024); }
        }
        let n = new.max(1) as u32;
        println!("polls {polls}, new frames {new} in 3 s; per frame: capture {:?}, shrink {:?}, jpeg {:?}", cap_t / n, shrink_t / n, jpeg_t / n);
    }

    /// Captures the primary monitor a few times and prints the picture sizes (needs a desktop).
    #[test]
    #[ignore]
    fn capture_live() {
        let m = monitors()[0];
        let mut c = Capturer::new(m, preset("balanced"));
        for i in 0..5 {
            let t = Instant::now();
            let r = c.grab();
            println!("{i}: {:?} in {:?} (dup: {})", r.as_ref().map(|o| o.as_ref().and_then(|sh| jpeg(sh, 75)).map(|j| j.len())), t.elapsed(), c.dup.is_some());
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
