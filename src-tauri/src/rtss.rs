//! RivaTuner Statistics Server feed, replacing what MSI Afterburner gave RTSS:
//! - the on-screen display: Deck writes its text into one RTSS OSD slot (owner "Deck"), RTSS
//!   draws it in games and fills the framerate macros itself (no injection by Deck);
//! - `MAHMSharedMemory`, Afterburner's monitoring memory (v2.0 layout from the RTSS SDK), so
//!   RTSS OverlayEditor's "MSI Afterburner" data source and other readers keep working.
//!
//! Layouts: RTSS SDK `RTSSSharedMemory.h` (v2.x) and `MSIAB\MAHMSharedMemory.h`.

use crate::app::App;
use crate::osd::{self, FrameWindow};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_ALL_ACCESS, MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE};

pub use crate::osd::OsdStyle;

/// One second's readings.
#[derive(Default, Clone, Debug)]
pub struct Snap {
    pub gpu_temp: Option<f32>,
    pub gpu_usage: Option<f32>,
    pub gpu_fb: Option<f32>,
    pub gpu_mem_mb: Option<f32>,
    pub gpu_mem_total_mb: Option<f32>,
    pub gpu_clock: Option<f32>,
    pub gpu_memclock: Option<f32>,
    pub gpu_power_w: Option<f32>,
    pub gpu_power_pct: Option<f32>,
    pub gpu_fan: Option<f32>,
    pub gpu_volt_mv: Option<f32>,
    pub cpu_temp: Option<f32>,
    pub cpu_usage: Option<f32>,
    pub cpu_clock: Option<f32>,
    pub cpu_power: Option<f32>,
    pub cores: Vec<f32>,
    pub ram_mb: Option<f32>,
    pub ram_total_mb: Option<f32>,
    pub commit_mb: Option<f32>,
    pub proc_ram_mb: Option<f32>,
    pub fps: Option<f32>,
    pub frametime: Option<f32>,
    pub fps_avg: Option<f32>,
    pub fps_min: Option<f32>,
    pub fps_max: Option<f32>,
    pub fps_1low: Option<f32>,
    pub fps_01low: Option<f32>,
    /// what holds the GPU clock back right now ("" = nothing)
    pub gpu_limit: String,
    /// the game's graphics API and resolution as RivaTuner recorded them (the preview's API and Res)
    pub game_api: Option<&'static str>,
    pub game_res: Option<(u32, u32)>,
    pub cpu_volt: Option<f32>,
    /// MB/s
    pub net_down: Option<f32>,
    pub net_up: Option<f32>,
    pub core_reads: Vec<CoreRead>,
    /// Windows' own per-thread load and clock
    pub thread_reads: Vec<CoreRead>,
    /// this PC's fans (name, RPM) and board sensors (name, °C)
    pub fans: Vec<(String, f32)>,
    pub board_temps: Vec<(String, f32)>,
}

/// One CPU core's own readings (whatever this CPU gives out).
#[derive(Default, Clone, Debug)]
pub struct CoreRead {
    pub name: String,
    pub load: Option<f32>,
    pub clock: Option<f32>,
    pub temp: Option<f32>,
}

// --------------------------------------------------------------------- RTSS shared memory

const RTSS_SIG: u32 = u32::from_le_bytes(*b"SSTR"); // 'RTSS'
const OWNER: &[u8] = b"Deck";

struct View {
    h: HANDLE,
    p: *mut u8,
    /// The mapping's size: offsets from its header are checked against it (`array`)
    len: usize,
}

impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.p as *mut _ });
            CloseHandle(self.h);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn open_rtss() -> Option<View> {
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide("RTSSSharedMemoryV2").as_ptr());
        if h.is_null() {
            return None;
        }
        let v = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
        if v.Value.is_null() {
            CloseHandle(h);
            return None;
        }
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        let len = if VirtualQuery(v.Value, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 { 0 } else { mbi.RegionSize };
        let view = View { h, p: v.Value as *mut u8, len };
        (len >= HEADER && rd(view.p, 0) == RTSS_SIG && rd(view.p, 4) >= 0x0002_0000).then_some(view)
    }
}

unsafe fn rd(p: *const u8, off: usize) -> u32 {
    std::ptr::read_volatile(p.add(off) as *const u32)
}

/// The header fields Deck touches end here (dwLastForegroundAppProcessID at 68).
const HEADER: usize = 72;

/// One of RivaTuner's arrays (app entries described at 8, OSD slots at 20): (entry size, offset,
/// count), the count cut so every entry lies inside the `len`-byte mapping. Any program can create
/// the mapping before RivaTuner does, so the header is not trusted; it is read again each time,
/// since it can change after the mapping was opened.
unsafe fn array(p: *const u8, len: usize, at: usize, min_size: usize) -> (usize, usize, usize) {
    if len < HEADER {
        return (0, 0, 0);
    }
    let (size, off, n) = (rd(p, at) as usize, rd(p, at + 4) as usize, rd(p, at + 8) as usize);
    if size < min_size.max(1) || off < HEADER || off > len {
        return (size, off, 0);
    }
    (size, off, n.min((len - off) / size).min(256))
}

/// The smallest app entry Deck reads: up to dwFrameTime at 280.
const APP_ENTRY: usize = 284;
/// The smallest OSD slot Deck reads: szOSD and szOSDOwner.
const OSD_SLOT: usize = 512;

/// Finds Deck's slot (or claims a free one; slot 0 belongs to RTSS's own clients) and writes.
/// Empty `text` releases the slot.
fn write_osd(v: &View, text: &str) -> bool {
    write_osd_with(v, text, None)
}

/// The room the frametime graph gets, in characters (from the last text written): see
/// `osd::osd_text_width`. RivaTuner fills the graph itself from every frame.
static GRAPH_W: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Starts the framerate figures over (the reset key and the buttons).
pub fn reset_stats() {
    FRAMES.lock().0.clear();
}

/// The graph object as it should be now (`avg_fps`: the game's average, for the top on auto).
fn graph_now(st: &OsdStyle, avg_fps: Option<f32>) -> Vec<u8> {
    osd::graph_object(st, GRAPH_W.load(Ordering::Relaxed), avg_fps)
}

/// The OSD entry's object buffer (RTSS_SHARED_MEMORY_OSD_ENTRY.buffer, after szOSD, szOSDOwner and szOSDEx).
const OSD_OBJECTS: usize = 256 + 256 + 4096;

/// Where the overlay text goes in a slot, and how many bytes the field holds (with its closing
/// zero): RivaTuner 7.3.5 and newer (shared memory v2.20) read the 32 KB szOSDEx2 after the object
/// buffer, as RivaTuner's own Overlay Editor writes it; older ones the 4 KB szOSDEx.
fn text_field(version: u32, slot_size: usize) -> (usize, usize) {
    const EX2: usize = OSD_OBJECTS + 262_144;
    if version >= 0x0002_0014 && slot_size >= EX2 + 32_768 {
        (EX2, 32_768)
    } else {
        (512, 4096)
    }
}

/// `text` cut to whole lines that fit a `cap`-byte field with its closing zero: a cut inside a
/// colour tag would garble everything after it.
fn fit_lines(text: &[u8], cap: usize) -> &[u8] {
    let room = cap.saturating_sub(1);
    if text.len() <= room {
        return text;
    }
    &text[..text[..room + 1].iter().rposition(|&c| c == b'\n').unwrap_or(0)]
}

/// Set when the last overlay written didn't fit RivaTuner's text field and lost lines at its end.
static TRIMMED: AtomicBool = AtomicBool::new(false);

/// `graph`: an embedded object to put at the start of the slot's object buffer, for `<OBJ=00000000>`.
fn write_osd_with(v: &View, text: &str, graph: Option<Vec<u8>>) -> bool {
    unsafe {
        let p = v.p;
        let (size, off, n) = array(p, v.len, 20, OSD_SLOT);
        if size < 512 + 4096 || n < 2 {
            return false;
        }
        let entry = |i: usize| p.add(off + i * size);
        let owner = |e: *mut u8| std::slice::from_raw_parts(e.add(256), 256);
        let is_ours = |e: *mut u8| owner(e).starts_with(OWNER) && owner(e)[OWNER.len()] == 0;
        let mut slot = (1..n).find(|i| is_ours(entry(*i)));
        if slot.is_none() {
            if text.is_empty() {
                return true;
            }
            slot = (1..n).find(|i| owner(entry(*i))[0] == 0);
        }
        let Some(i) = slot else { return false };
        let e = entry(i);
        let busy = &*(p.add(36) as *const AtomicI32);
        // dwBusy bit 0 is the writers' lock (SDK sample); give up this round if it's held
        for _ in 0..50 {
            if busy.fetch_or(1, Ordering::SeqCst) & 1 == 0 {
                if let Some(g) = &graph {
                    // embedded objects need shared memory v2.12; older RivaTuners just skip the tag
                    if rd(p, 4) >= 0x0002_000C && size >= OSD_OBJECTS + g.len() {
                        std::ptr::copy_nonoverlapping(g.as_ptr(), e.add(OSD_OBJECTS), g.len());
                    }
                }
                let bytes = ansi(text);
                let (at, cap) = text_field(rd(p, 4), size);
                let fit = fit_lines(&bytes, cap);
                TRIMMED.store(fit.len() < bytes.len(), Ordering::Relaxed);
                // both fields are cleared, so the one not used now never shows an old overlay
                std::ptr::write_bytes(e.add(512), 0, 4096);
                if at != 512 {
                    std::ptr::write_bytes(e.add(at), 0, cap);
                }
                std::ptr::copy_nonoverlapping(fit.as_ptr(), e.add(at), fit.len());
                // the short field too, for old readers
                std::ptr::write_bytes(e, 0, 256);
                if text.is_empty() {
                    std::ptr::write_bytes(e.add(256), 0, 256);
                } else {
                    std::ptr::copy_nonoverlapping(OWNER.as_ptr(), e.add(256), OWNER.len());
                    *e.add(256 + OWNER.len()) = 0;
                }
                let frame = &*(p.add(32) as *const AtomicI32);
                frame.fetch_add(1, Ordering::SeqCst);
                busy.fetch_and(!1, Ordering::SeqCst);
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}

/// Framerate and frametime of the game as RTSS measured them (see `fps_at`).
fn fps(v: &View) -> Option<(f32, f32, String, u32)> {
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount() };
    let follow = FRAMES.lock().1.map(|(_, pid)| pid);
    unsafe { fps_at(v.p, v.len, now, follow) }
}

/// The graphics API and resolution RivaTuner recorded for process `pid`: the API from the low
/// word of the entry's dwFlags (APPFLAG_API_USAGE_MASK), the size from dwResolutionX/Y (shared
/// memory v2.20 and newer). None for what isn't known.
unsafe fn game_info_at(p: *const u8, len: usize, pid: u32) -> (Option<&'static str>, Option<(u32, u32)>) {
    const RES: usize = 9224;
    let (size, off, n) = array(p, len, 8, APP_ENTRY);
    let Some(e) = (0..n).map(|i| p.add(off + i * size)).find(|e| rd(*e, 0) == pid) else { return (None, None) };
    let api = match rd(e, 264) & 0xFFFF {
        1 => Some("OpenGL"),
        2 => Some("DirectDraw"),
        3 => Some("D3D8"),
        4 => Some("D3D9"),
        5 => Some("D3D9Ex"),
        6 => Some("D3D10"),
        7 => Some("D3D11"),
        8 | 9 => Some("D3D12"),
        10 => Some("Vulkan"),
        _ => None,
    };
    let res = (rd(p, 4) >= 0x0002_0014 && size >= RES + 8).then(|| (rd(e, RES), rd(e, RES + 4))).filter(|(w, h)| *w > 0 && *h > 0);
    (api, res)
}

/// RivaTuner measures the framerate over periods of about a second; a period that ended longer
/// ago than this is an app that stopped drawing (alt-tabbed, paused, closed), not a framerate.
const STALE_MS: u32 = 2500;

/// `fps` on RivaTuner's shared memory at `p`: (framerate, frametime ms, exe name, process id) of
/// the app the framerate figures follow (`follow`, so a line never mixes two programs), else
/// RivaTuner's foreground app, else the app drawing fastest. Only apps drawing now count: their
/// last period ended within `STALE_MS` of `now` (ms since boot, the clock RivaTuner stamps with).
unsafe fn fps_at(p: *const u8, len: usize, now: u32, follow: Option<u32>) -> Option<(f32, f32, String, u32)> {
    {
        let (size, off, n) = array(p, len, 8, APP_ENTRY);
        // dwLastForegroundAppProcessID sits after dwDesktopVideoCaptureFlags + Stat[5] + dwLastForegroundApp
        let fg_pid = rd(p, 40 + 4 + 20 + 4);
        let (mut fg, mut best): (Option<(f32, f32, String, u32)>, Option<(f32, f32, String, u32)>) = (None, None);
        for i in 0..n {
            let e = p.add(off + i * size);
            let pid = rd(e, 0);
            if pid == 0 {
                continue;
            }
            let (t0, t1, frames, ft) = (rd(e, 268), rd(e, 272), rd(e, 276), rd(e, 280));
            if t1 <= t0 || frames == 0 || now.wrapping_sub(t1) > STALE_MS {
                continue;
            }
            let rate = 1000.0 * frames as f32 / (t1 - t0) as f32;
            // szName[260], not trusted to end in a zero
            let raw = std::slice::from_raw_parts(e.add(4), 260);
            let name = String::from_utf8_lossy(&raw[..raw.iter().position(|&c| c == 0).unwrap_or(260)]).to_string();
            let name = name.rsplit('\\').next().unwrap_or("").to_string();
            let item = (rate, ft as f32 / 1000.0, name, pid);
            if Some(pid) == follow {
                return Some(item);
            }
            if pid == fg_pid {
                fg = Some(item);
            } else if best.as_ref().map(|b| b.0 < item.0).unwrap_or(true) {
                best = Some(item);
            }
        }
        fg.or(best)
    }
}

// --------------------------------------------------------------------- MAHM shared memory

const MAHM_HEADER: usize = 32;
const MAHM_ENTRY: usize = 5 * 260 + 6 * 4;
const MAHM_GPU: usize = 5 * 260 + 4;
const MAHM_SIG: u32 = u32::from_le_bytes(*b"MHAM"); // 'MAHM'

/// (name, unit, format, min, max, gpu-wide?, source id)
const SOURCES: &[(&str, &str, &str, f32, f32, bool, u32)] = &[
    ("GPU temperature", "°C", "%.0f", 0.0, 100.0, true, 0x00),
    ("GPU usage", "%", "%.0f", 0.0, 100.0, true, 0x30),
    ("Memory usage", "MB", "%.0f", 0.0, 16384.0, true, 0x31),
    ("Core clock", "MHz", "%.0f", 0.0, 3000.0, true, 0x20),
    ("Memory clock", "MHz", "%.0f", 0.0, 12000.0, true, 0x22),
    ("Power", "W", "%.0f", 0.0, 500.0, true, 0x61),
    ("Power percent", "%", "%.0f", 0.0, 150.0, true, 0x60),
    ("Fan speed", "%", "%.0f", 0.0, 100.0, true, 0x10),
    ("GPU voltage", "mV", "%.0f", 0.0, 1300.0, true, 0x40),
    ("CPU temperature", "°C", "%.0f", 0.0, 100.0, false, 0x80),
    ("CPU usage", "%", "%.0f", 0.0, 100.0, false, 0x90),
    ("CPU clock", "MHz", "%.0f", 0.0, 6000.0, false, 0xA0),
    ("CPU power", "W", "%.0f", 0.0, 300.0, false, 0x100),
    ("RAM usage", "MB", "%.0f", 0.0, 65536.0, false, 0x91),
    ("Commit charge", "MB", "%.0f", 0.0, 131072.0, false, 0x92),
    ("RAM usage \\ process", "MB", "%.0f", 0.0, 65536.0, false, 0x93),
    ("Framerate", "FPS", "%.0f", 0.0, 500.0, false, 0x50),
    ("Frametime", "ms", "%.1f", 0.0, 100.0, false, 0x51),
];

fn source_values(s: &Snap) -> Vec<Option<f32>> {
    vec![s.gpu_temp, s.gpu_usage, s.gpu_mem_mb, s.gpu_clock, s.gpu_memclock, s.gpu_power_w, s.gpu_power_pct, s.gpu_fan, s.gpu_volt_mv, s.cpu_temp, s.cpu_usage, s.cpu_clock, s.cpu_power, s.ram_mb, s.commit_mb, s.proc_ram_mb, s.fps, s.frametime]
}

pub fn mahm_size(cores: usize) -> usize {
    MAHM_HEADER + (SOURCES.len() + cores) * MAHM_ENTRY + MAHM_GPU
}

/// RTSS and MAHM text fields are ANSI (Windows-1252): "°" is 0xB0.
fn ansi(s: &str) -> Vec<u8> {
    // RivaTuner's text is ANSI: a dash for a missing value must not come out as '?'
    s.chars().map(|c| match c { '°' => 0xB0, '–' | '—' => b'-', c if c.is_ascii() => c as u8, _ => b'?' }).collect()
}

fn put_str(buf: &mut [u8], off: usize, s: &str) {
    let bytes = ansi(s);
    let n = bytes.len().min(259);
    buf[off..off + n].copy_from_slice(&bytes[..n]);
    buf[off + n] = 0;
}

/// Builds the whole MAHM block (written in one go into the mapping).
pub fn mahm_block(s: &Snap, osd: &[String], gpu: (&str, &str, &str, u32)) -> Vec<u8> {
    let n = SOURCES.len() + s.cores.len();
    let mut b = vec![0u8; mahm_size(s.cores.len())];
    let w32 = |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    w32(&mut b, 0, MAHM_SIG);
    w32(&mut b, 4, 0x0002_0000);
    w32(&mut b, 8, MAHM_HEADER as u32);
    w32(&mut b, 12, n as u32);
    w32(&mut b, 16, MAHM_ENTRY as u32);
    w32(&mut b, 20, crate::util::now_s() as u32);
    w32(&mut b, 24, 1);
    w32(&mut b, 28, MAHM_GPU as u32);
    let vals = source_values(s);
    let osd_item = |name: &str| crate::profiles::afterburner::osd_item(name).map(|i| osd.iter().any(|x| x == i)).unwrap_or(false);
    let mut entry = |i: usize, name: &str, unit: &str, fmt: &str, min: f32, max: f32, gpu_wide: bool, id: u32, v: Option<f32>| {
        let o = MAHM_HEADER + i * MAHM_ENTRY;
        put_str(&mut b, o, name);
        put_str(&mut b, o + 260, unit);
        put_str(&mut b, o + 520, name);
        put_str(&mut b, o + 780, unit);
        put_str(&mut b, o + 1040, fmt);
        let f = |b: &mut [u8], at: usize, x: f32| b[at..at + 4].copy_from_slice(&x.to_le_bytes());
        f(&mut b, o + 1300, v.filter(|v| v.is_finite()).unwrap_or(f32::MAX));
        f(&mut b, o + 1304, min);
        f(&mut b, o + 1308, max);
        w32(&mut b, o + 1312, if osd_item(name) { 1 } else { 0 });
        w32(&mut b, o + 1316, if gpu_wide { 0 } else { 0xFFFF_FFFF });
        w32(&mut b, o + 1320, id);
    };
    for (i, (name, unit, fmt, min, max, g, id)) in SOURCES.iter().enumerate() {
        entry(i, name, unit, fmt, *min, *max, *g, *id, vals[i]);
    }
    for (k, v) in s.cores.iter().enumerate() {
        entry(SOURCES.len() + k, &format!("CPU{} usage", k + 1), "%", "%.0f", 0.0, 100.0, false, 0x90, Some(*v));
    }
    let g = MAHM_HEADER + n * MAHM_ENTRY;
    put_str(&mut b, g, gpu.0);
    put_str(&mut b, g + 520, gpu.1);
    put_str(&mut b, g + 780, gpu.2);
    w32(&mut b, g + 1300, gpu.3);
    b
}

struct Mahm {
    h: HANDLE,
    p: *mut u8,
    size: usize,
}

unsafe impl Send for Mahm {}

static MAHM: Mutex<Option<Mahm>> = Mutex::new(None);

/// Writes the block, (re)creating the mapping when it must grow. Not while Afterburner owns it.
fn publish(block: &[u8]) {
    let mut m = MAHM.lock();
    // Afterburner started after Deck: it opens the same block, so step aside instead of two
    // programs writing it (its own data then reaches RTSS as before)
    let grow = m.as_ref().map(|x| x.size < block.len()).unwrap_or(false);
    if grow || (m.is_some() && crate::gpu::afterburner_running()) {
        if let Some(old) = m.take() {
            unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: old.p as *mut _ });
                CloseHandle(old.h);
            }
        }
    }
    // while Afterburner owns the mapping, look again only every 10 s
    static LAST_TRY: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    if m.is_none() {
        let mut lt = LAST_TRY.lock();
        if lt.map(|t| t.elapsed() < Duration::from_secs(10)).unwrap_or(false) {
            return;
        }
        *lt = Some(std::time::Instant::now());
    }
    if m.is_none() {
        let size = block.len() + 64 * MAHM_ENTRY; // room for more cores without remapping
        unsafe {
            let h = CreateFileMappingW(INVALID_HANDLE_VALUE, std::ptr::null(), PAGE_READWRITE, 0, size as u32, wide("MAHMSharedMemory").as_ptr());
            if h.is_null() {
                return;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS && crate::gpu::afterburner_running() {
                CloseHandle(h);
                return;
            }
            let v = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if v.Value.is_null() {
                CloseHandle(h);
                return;
            }
            *m = Some(Mahm { h, p: v.Value as *mut u8, size });
        }
    }
    if let Some(x) = m.as_ref() {
        unsafe {
            // signature last, so a reader never sees a half-written block as valid
            std::ptr::write_volatile(x.p as *mut u32, 0);
            std::ptr::copy_nonoverlapping(block.as_ptr().add(4), x.p.add(4), block.len() - 4);
            std::ptr::write_volatile(x.p as *mut u32, MAHM_SIG);
        }
    }
}

fn unpublish() {
    if let Some(x) = MAHM.lock().take() {
        unsafe {
            std::ptr::write_volatile(x.p as *mut u32, 0xDEAD);
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: x.p as *mut _ });
            CloseHandle(x.h);
        }
    }
}

// --------------------------------------------------------------------- the loop

static LAST: Mutex<Option<(Snap, Option<String>)>> = Mutex::new(None);
static APP: OnceLock<Arc<App>> = OnceLock::new();
/// when the Sensors page last asked how the overlay looks
static PREVIEW_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// the game's recent frametimes, and the RTSS app slot and process they came from (a game that
/// closes and another that takes its slot are two games)
static FRAMES: Mutex<(FrameWindow, Option<(u32, u32)>)> = Mutex::new((FrameWindow::new(), None));
static STOP: AtomicBool = AtomicBool::new(false);
/// held while the loop writes the slot and the monitoring block, and by `stop`
static WRITE: Mutex<()> = Mutex::new(());
static RUNNING: AtomicBool = AtomicBool::new(false);

/// RivaTuner has its own "Show On-Screen Display" switch that hides every overlay, Deck's
/// included. Afterburner's overlay key flips it, and Deck does the same: the switch follows
/// Deck's overlay on and off, so a switch left off (by Afterburner, or by hand) doesn't hide
/// Deck's. The switch RivaTuner shows and the 32-bit games use lives in its 32-bit hooks, which
/// 64-bit Deck can't load: the embedded 32-bit helper (`rtss32/`) sets it and tells RivaTuner.
/// The 64-bit hooks keep their own copy, set here directly.
fn set_rtss_osd_visible(visible: bool) {
    set_rtss_flag(1, visible); // RTSSHOOKSFLAG_OSD_VISIBLE in the RTSS SDK
}

/// RTSSHOOKSFLAG_LIMITER_DISABLED: RivaTuner's framerate limiter is off while it is set. This is
/// the switch Afterburner's limiter key flips; the limit itself stays in RivaTuner's profile.
const LIMITER_DISABLED: u32 = 4;

/// Sets (`on`) or clears one of RivaTuner's switch bits in both hooks: the 64-bit one first, in
/// Deck itself; the 32-bit one through a helper program, which a busy game's anti-cheat can make
/// slow to start, so 64-bit games (nearly all) don't wait for it.
fn set_rtss_flag(flag: u32, on: bool) {
    use windows::core::{HSTRING, PCSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    type SetFlags = unsafe extern "C" fn(u32, u32) -> u32;
    let Some(dir) = rtss_exe().and_then(|e| e.parent().map(|p| p.to_path_buf())) else { return };
    static SET64: OnceLock<Option<SetFlags>> = OnceLock::new();
    let set = SET64.get_or_init(|| unsafe {
        let m = LoadLibraryW(&HSTRING::from(dir.join("RTSSHooks64.dll").as_os_str())).ok()?;
        Some(std::mem::transmute::<RawProc, SetFlags>(GetProcAddress(m, PCSTR(c"SetFlags".as_ptr() as *const u8))?))
    });
    if let Some(set) = *set {
        unsafe { set(!flag, if on { flag } else { 0 }) };
        trace("64-bit switch");
    } else if crate::log::every("rtss hooks64", Duration::from_secs(3600)) {
        crate::log::warn("overlay: RivaTuner's RTSSHooks64.dll couldn't be loaded; its switches can't be set for 64-bit games");
    }
    if let Some(helper) = rtss32_helper() {
        use std::os::windows::process::CommandExt;
        let r = std::process::Command::new(helper)
            .arg(if on { "on" } else { "off" })
            .arg(&dir)
            .arg(flag.to_string())
            .creation_flags(crate::util::CREATE_NO_WINDOW)
            .status();
        trace("32-bit helper");
        match r {
            // 0: it was already as asked, 1: the helper changed it
            Ok(st) if matches!(st.code(), Some(0 | 1)) => {}
            Ok(st) => crate::log::info(format!("overlay: the 32-bit helper couldn't set RivaTuner's switch {flag} {} (exit {:?}); 32-bit games keep the old state", if on { "on" } else { "off" }, st.code())),
            Err(e) => crate::log::warn(format!("overlay: the 32-bit helper couldn't be started: {e}")),
        }
    }
}

/// What the limiter key set last (0 = not yet, 1 = off, 2 = on): the answer when RivaTuner's
/// switch can't be read.
static LIMITER_SET: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// RivaTuner's 64-bit hooks, loaded once: the switches (`GetFlags`/`SetFlags`) and the profile
/// calls the SDK's RTSSSharedMemorySample uses to change a profile in the running RivaTuner and
/// in hooked games (writing the profile file alone isn't seen until RivaTuner restarts).
/// What GetProcAddress hands back, before it is given its real signature.
type RawProc = unsafe extern "system" fn() -> isize;

struct Hooks64 {
    get_flags: unsafe extern "C" fn() -> u32,
    load_profile: unsafe extern "C" fn(*const u8) -> i32,
    set_property: unsafe extern "C" fn(*const u8, *const std::ffi::c_void, u32) -> i32,
    save_profile: unsafe extern "C" fn(*const u8) -> i32,
    update_profiles: unsafe extern "C" fn(),
}

fn hooks64() -> Option<&'static Hooks64> {
    use windows::core::{HSTRING, PCSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    // kept once loaded; until then tried again each time (RivaTuner may be installed later)
    static H: OnceLock<Hooks64> = OnceLock::new();
    if let Some(h) = H.get() {
        return Some(h);
    }
    let h = (|| unsafe {
        let dir = rtss_exe()?.parent()?.to_path_buf();
        let m = LoadLibraryW(&HSTRING::from(dir.join("RTSSHooks64.dll").as_os_str())).ok()?;
        let f = |name: &std::ffi::CStr| GetProcAddress(m, PCSTR(name.as_ptr() as *const u8));
        Some(Hooks64 {
            get_flags: std::mem::transmute::<RawProc, unsafe extern "C" fn() -> u32>(f(c"GetFlags")?),
            load_profile: std::mem::transmute::<RawProc, unsafe extern "C" fn(*const u8) -> i32>(f(c"LoadProfile")?),
            set_property: std::mem::transmute::<RawProc, unsafe extern "C" fn(*const u8, *const std::ffi::c_void, u32) -> i32>(f(c"SetProfileProperty")?),
            save_profile: std::mem::transmute::<RawProc, unsafe extern "C" fn(*const u8) -> i32>(f(c"SaveProfile")?),
            update_profiles: std::mem::transmute::<RawProc, unsafe extern "C" fn()>(f(c"UpdateProfiles")?),
        })
    })()?;
    let _ = H.set(h);
    H.get()
}

/// Whether RivaTuner's framerate limiter is on. Read from the 64-bit hooks (Deck sets both
/// copies, and 64-bit games use this one), else the 32-bit helper, else what the key set last.
/// None when none of them knows.
pub fn limiter_on() -> Option<bool> {
    use std::os::windows::process::CommandExt;
    if let Some(h) = hooks64() {
        return Some(unsafe { (h.get_flags)() } & LIMITER_DISABLED == 0);
    }
    let from_helper = (|| {
        let dir = rtss_exe()?.parent()?.to_path_buf();
        let code = std::process::Command::new(rtss32_helper()?).arg("status").arg(&dir).arg(LIMITER_DISABLED.to_string()).creation_flags(crate::util::CREATE_NO_WINDOW).status().ok()?.code()?;
        matches!(code, 10 | 11).then_some(code == 10)
    })();
    from_helper.or(match LIMITER_SET.load(Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    })
}

/// Tells RivaTuner's window to read its settings again (so it shows the new values).
fn refresh_rtss_window() {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
    const WM_RTSS_UPDATESETTINGS: u32 = 0x8000 + 100;
    unsafe {
        let wnd = FindWindowW(PCWSTR::null(), w!("RTSS")).or_else(|_| FindWindowW(PCWSTR::null(), w!("RivaTuner Statistics Server")));
        if let Ok(wnd) = wnd {
            let _ = PostMessageW(Some(wnd), WM_RTSS_UPDATESETTINGS, WPARAM(0), LPARAM(0));
        }
    }
}

/// The limit in RivaTuner's global profile (frames a second, 0 = none set).
pub fn limit_fps() -> u32 {
    let Some(f) = profiles_dir().map(|d| d.join("Global")) else { return 0 };
    let text = std::fs::read(&f).map(|b| from_ansi(&b)).unwrap_or_default();
    let mut in_sec = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_sec = l.eq_ignore_ascii_case("[Framerate]");
        } else if in_sec {
            if let Some(v) = l.strip_prefix("Limit=") {
                return v.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

/// Sets the limit in RivaTuner's global profile (a backup of the original is kept like the look's).
pub fn set_limit_fps(fps: u32) -> Result<(), String> {
    let dir = profiles_dir().ok_or("RivaTuner isn't installed.")?;
    let f = dir.join("Global");
    let bytes = std::fs::read(&f).map_err(|e| e.to_string())?;
    let text = from_ansi(&bytes);
    let keep = dir.join("Deck backup");
    let _ = std::fs::create_dir_all(&keep);
    if !keep.join("Global").exists() {
        let _ = std::fs::write(keep.join("Global"), &bytes);
    }
    let fps = fps.min(1000);
    let new = set_ini(&text, &[("Framerate", "Limit", fps.to_string()), ("Framerate", "LimitDenominator", "1".into())]);
    std::fs::write(&f, to_ansi(&new)).map_err(|e| e.to_string())?;
    // a running RivaTuner keeps its own copy of the profile (its window showed 0 while the file
    // said 5): load the file we just wrote, set the limit, save, and tell RivaTuner and the games
    if let Some(h) = hooks64() {
        unsafe {
            if (h.load_profile)(c"".as_ptr().cast()) != 0 {
                let v: u32 = fps;
                (h.set_property)(c"FramerateLimit".as_ptr().cast(), &v as *const u32 as *const std::ffi::c_void, 4);
                (h.save_profile)(c"".as_ptr().cast());
            }
            (h.update_profiles)();
        }
    }
    request_reload();
    refresh_rtss_window();
    Ok(())
}

/// The limiter key: RivaTuner's framerate limiter on or off, like Afterburner's key did.
pub fn toggle_limiter(app: &App) {
    if !rtss_running() {
        let _ = start_rtss();
    }
    let on = !limiter_on().unwrap_or(false);
    set_rtss_flag(LIMITER_DISABLED, !on);
    LIMITER_SET.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    refresh_rtss_window();
    let fps = limit_fps();
    let body = if fps == 0 { "No limit is set yet: set one on the Overlay page.".to_string() } else { format!("Games are held to {fps} FPS.") };
    app.toast(crate::app::Toast::simple("limiter", if on { "Framerate limit on" } else { "Framerate limit off" }, if on { &body } else { "Games run as fast as they can." }));
}

/// How RivaTuner's "Show On-Screen Display" switch is set: (the one its window shows and
/// 32-bit games use, the 64-bit hooks' copy). None when it can't be read.
pub fn osd_switch() -> (Option<bool>, Option<bool>) {
    use windows::core::{HSTRING, PCSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let Some(dir) = rtss_exe().and_then(|e| e.parent().map(|p| p.to_path_buf())) else { return (None, None) };
    let v32 = rtss32_helper().and_then(|h| {
        use std::os::windows::process::CommandExt;
        let code = std::process::Command::new(h).arg("status").arg(&dir).creation_flags(crate::util::CREATE_NO_WINDOW).status().ok()?.code()?;
        matches!(code, 10 | 11).then_some(code == 11)
    });
    let v64 = unsafe {
        LoadLibraryW(&HSTRING::from(dir.join("RTSSHooks64.dll").as_os_str()))
            .ok()
            .and_then(|m| GetProcAddress(m, PCSTR(c"GetFlags".as_ptr() as *const u8)))
            .map(|f| std::mem::transmute::<RawProc, unsafe extern "C" fn() -> u32>(f)() & 1 != 0)
    };
    (v32, v64)
}

/// The 32-bit helper, built by build.rs (empty when the i686 target wasn't installed).
static RTSS32: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rtss-osd32.exe"));

/// Where the helper may be written and started from: next to Deck, and without admin rights also
/// %LOCALAPPDATA%\Deck. An elevated Deck never uses that one: the user's programs could swap the
/// file between Deck's check and the start, and it would run with admin rights.
fn helper_dirs(elevated: bool) -> Vec<std::path::PathBuf> {
    let mut d: Vec<_> = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())).into_iter().collect();
    if !elevated {
        d.extend(crate::util::env_path("LOCALAPPDATA").map(|p| p.join("Deck")));
    }
    d
}

/// Writes the helper next to Deck (or, without admin rights, into %LOCALAPPDATA%\Deck) the first time it's needed.
fn rtss32_helper() -> Option<std::path::PathBuf> {
    if RTSS32.is_empty() {
        return None;
    }
    for d in helper_dirs(crate::elevation::is_admin()) {
        let p = d.join("rtss-osd32.exe");
        if std::fs::metadata(&p).map(|m| m.len() == RTSS32.len() as u64).unwrap_or(false) && std::fs::read(&p).map(|b| b == RTSS32).unwrap_or(false) {
            return Some(p);
        }
        if std::fs::create_dir_all(&d).is_ok() && std::fs::write(&p, RTSS32).is_ok() {
            return Some(p);
        }
    }
    None
}

/// RivaTuner's RTSS.exe: where its installer recorded it (the folder can be chosen when it is
/// installed; HKLM, so only an admin can change it), else the default folder.
pub fn rtss_exe() -> Option<std::path::PathBuf> {
    let recorded = [r"SOFTWARE\WOW6432Node\Unwinder\RTSS", r"SOFTWARE\Unwinder\RTSS"].iter().find_map(|k| hklm_string(k, "InstallPath")).map(std::path::PathBuf::from);
    let default = crate::util::program_files_x86().join("RivaTuner Statistics Server").join("RTSS.exe");
    recorded
        .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("RTSS.exe")))
        .into_iter()
        .chain(Some(default))
        .find(|p| p.is_file())
}

/// A text value under HKEY_LOCAL_MACHINE.
fn hklm_string(key: &str, value: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) }.ok().ok()?;
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    (!s.is_empty()).then_some(s)
}

pub fn rtss_running() -> bool {
    open_rtss().is_some()
}

/// Deck's own window (deck.exe and the WebView2 processes that draw it) gets a RivaTuner profile
/// with hooking off: no overlay belongs there, and a hooked window can freeze when RivaTuner
/// goes away. An existing profile only gets its EnableHooking line set; apply_look leaves these
/// files alone since they have no [OSD] section.
pub fn keep_out_of_hooking() {
    let Some(dir) = profiles_dir() else { return };
    let mut changed = false;
    for name in ["deck.exe.cfg", "msedgewebview2.exe.cfg"] {
        let f = dir.join(name);
        let old = std::fs::read(&f).ok().map(|b| from_ansi(&b));
        let new = match &old {
            Some(t) if t.lines().any(|l| l.trim().eq_ignore_ascii_case("[Hooking]")) => set_ini(t, &[("Hooking", "EnableHooking", "0".into())]),
            Some(t) => format!("{t}\r\n[Hooking]\r\nEnableHooking=0\r\n"),
            None => "[Hooking]\r\nEnableHooking=0\r\n".to_string(),
        };
        if old.as_deref() != Some(new.as_str()) && std::fs::write(&f, to_ansi(&new)).is_ok() {
            changed = true;
        }
    }
    if changed {
        request_reload();
    }
}

/// Starts RTSS (Deck runs as admin; RTSS needs it).
pub fn start_rtss() -> Result<(), String> {
    // by its window: the shared memory stays while Deck (or a game) still has it open
    if rtss_windows().0.is_some() {
        return Ok(());
    }
    let exe = rtss_exe().ok_or("RivaTuner Statistics Server is not installed.")?;
    keep_out_of_hooking();
    match std::process::Command::new(&exe).current_dir(exe.parent().unwrap()).spawn() {
        Ok(_) => {
            crate::log::info("overlay: RivaTuner started");
            Ok(())
        }
        Err(e) => {
            crate::log::warn(format!("overlay: RivaTuner couldn't be started: {e}"));
            Err(e.to_string())
        }
    }
}

/// RivaTuner's windows: its main window ("RivaTunerStatisticsServer", which also owns the tray
/// icon) and a helper window ("RTSS", of the same process) that shows up blank when made visible.
/// Found by their titles, not a process list: the tray switch looks every 5 s, also in games,
/// and starting `tasklist` that often cost a process each time.
fn rtss_windows() -> (Option<windows::Win32::Foundation::HWND>, Option<windows::Win32::Foundation::HWND>) {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId};
    // (title, process) of every window called one of the two names
    struct Found(Vec<(bool, u32, HWND)>);
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        let f = &mut *(l.0 as *mut Found);
        let mut buf = [0u16; 32];
        let n = GetWindowTextW(h, &mut buf) as usize;
        let main = match String::from_utf16_lossy(&buf[..n]).as_str() {
            "RivaTunerStatisticsServer" => true,
            "RTSS" => false,
            _ => return BOOL(1),
        };
        let mut p = 0;
        GetWindowThreadProcessId(h, Some(&mut p));
        f.0.push((main, p, h));
        BOOL(1)
    }
    let mut f = Found(vec![]);
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut f as *mut _ as isize));
    }
    let Some(&(_, pid, main)) = f.0.iter().find(|w| w.0) else { return (None, None) };
    (Some(main), f.0.iter().find(|w| !w.0 && w.1 == pid).map(|w| w.2))
}

/// Removes RivaTuner's tray icon (it keeps running; Deck's Overlay page opens it instead) or
/// puts it back. RivaTuner numbers its icon after its own window, and re-adds it when told the
/// taskbar was recreated, so no restart is needed either way. Deck repeats the removal while the
/// switch is on, since RivaTuner adds the icon again when it or Explorer restarts.
pub fn set_tray_icon(show: bool) {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIM_DELETE, NOTIFYICONDATAW};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, RegisterWindowMessageW};
    let (Some(owner), _) = rtss_windows() else { return };
    unsafe {
        if show {
            let _ = PostMessageW(Some(owner), RegisterWindowMessageW(w!("TaskbarCreated")), WPARAM(0), LPARAM(0));
        } else {
            let nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: owner, uID: owner.0 as usize as u32, ..Default::default() };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

/// Opens RivaTuner's own window. RivaTuner draws it properly only when it opens the window itself
/// (on start or from its tray icon); shown from outside it stays black. The tray icon can't be
/// clicked from outside either, so a running RivaTuner is restarted: its window opens, and with
/// the icon switch on, the icon is removed again at once. A game's overlay drops for a moment.
pub fn open_rtss_window() -> Result<(), String> {
    let exe = rtss_exe().ok_or("RivaTuner Statistics Server is not installed.")?;
    keep_out_of_hooking();
    // by its window, not its shared memory: Deck keeps that open, so it outlives RivaTuner.
    // Asked to close, not killed: a killed RivaTuner leaves its hooks in every app it drew in
    // (Deck's own window and browsers included), and those can freeze.
    if let (Some(main), _) = rtss_windows() {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let _ = unsafe { PostMessageW(Some(main), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        for _ in 0..25 {
            if rtss_windows().0.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if rtss_windows().0.is_some() {
            return Err("RivaTuner didn't close. Open it from its tray icon or the Start menu.".into());
        }
    }
    // with RivaTuner's own "Start minimized" on, a fresh start goes straight to the tray (whose
    // icon Deck may hide) and nothing opens: turn it off for this start, then put it back
    let config = exe.parent().unwrap().join("Profiles").join("Config");
    let minimized = std::fs::read(&config).ok().map(|b| from_ansi(&b)).filter(|t| t.lines().any(|l| l.trim() == "StartMinimized=1"));
    if let Some(t) = &minimized {
        let _ = std::fs::write(&config, to_ansi(&set_ini(t, &[("Settings", "StartMinimized", "0".into())])));
    }
    let started = std::process::Command::new(&exe).current_dir(exe.parent().unwrap()).spawn().map_err(|e| e.to_string());
    let mut shown = false;
    if started.is_ok() {
        // its window exists hidden at once; wait until RivaTuner shows it (it has read the setting)
        for _ in 0..50 {
            if rtss_windows().0.is_some_and(|w| unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(w) }.as_bool()) {
                shown = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if minimized.is_some() {
        // read again: RivaTuner may have written the file since
        if let Ok(b) = std::fs::read(&config) {
            let _ = std::fs::write(&config, to_ansi(&set_ini(&from_ansi(&b), &[("Settings", "StartMinimized", "1".into())])));
        }
    }
    started.map_err(|e| format!("RivaTuner didn't start again: {e}"))?;
    if !shown {
        return Err(if rtss_windows().0.is_some() {
            "RivaTuner is running again, but its window didn't open. Open it from its tray icon or the Start menu.".into()
        } else {
            "RivaTuner closed but didn't start again. Start it from the Start menu.".into()
        });
    }
    if APP.get().is_some_and(|a| a.settings.lock().rtss_tray_hidden) {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(300));
            set_tray_icon(false);
        }
    }
    Ok(())
}

/// Installs RivaTuner from its official source with winget, which downloads Guru3D's own
/// installer and checks its hash (RivaTuner's licence doesn't allow shipping it inside Deck),
/// then starts it.
pub fn install_rtss() -> Result<(), String> {
    if rtss_exe().is_some() {
        return start_rtss();
    }
    let out = crate::util::run_hidden(
        "winget",
        &["install", "--id", "Guru3D.RTSS", "--exact", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"],
    )
    .map_err(|_| "Windows' package manager (winget) isn't on this PC. Get RivaTuner from guru3d.com.".to_string())?;
    if rtss_exe().is_none() {
        let text = String::from_utf8_lossy(&out.stdout);
        let last = text.lines().map(str::trim).rfind(|l| l.chars().any(char::is_alphanumeric)).unwrap_or("no details");
        return Err(format!("RivaTuner didn't install ({last}). Get it from guru3d.com."));
    }
    start_rtss()
}

/// Overlay on/off (the hotkey and the Sensors page switch).
pub fn toggle(app: &App) {
    trace("worker");
    let on = {
        let mut s = app.settings.lock();
        s.osd_on = !s.osd_on;
        s.osd_on
    };
    app.save_settings();
    if on && !rtss_running() {
        let _ = start_rtss();
    }
    trace("RivaTuner checked");
    refresh_now();
    // RivaTuner's own switch at once: the loop would flip it only after its next sensor read,
    // which a busy game (Helldivers 2) can hold up for a long while
    follow_switch(app, rtss_running());
    if let Some(at) = trace_end(on) {
        if on {
            watch_drawn(at);
        }
    }
}

/// After the overlay key turned it on: how long until the game in front took the new overlay.
/// The game's hook copies RivaTuner's overlay frame number into its own entry when it does, so
/// a late overlay shows as Deck's (the key line), or RivaTuner's and the game's (this line).
fn watch_drawn(at: Instant) {
    const APP_OSD_FRAME: usize = 332; // dwOSDFrame in RTSS_SHARED_MEMORY_APP_ENTRY
    let _ = std::thread::Builder::new().name("osd-drawn".into()).spawn(move || {
        let Some(v) = open_rtss() else { return };
        let ms = || at.elapsed().as_millis();
        // the global frame number after Deck's write, and RivaTuner's foreground app
        let (want, pid) = unsafe { (rd(v.p, 32), rd(v.p, 68)) };
        let entry = || unsafe {
            let (size, off, n) = array(v.p, v.len, 8, APP_OSD_FRAME + 4);
            (0..n).map(|i| v.p.add(off + i * size)).find(|e| pid != 0 && rd(*e, 0) == pid).map(|e| (rd(e, APP_OSD_FRAME), rd(e, 280)))
        };
        loop {
            let Some((got, frame_us)) = entry() else {
                crate::log::info(format!("overlay key: RivaTuner has no game in front to draw in ({} ms after the press)", ms()));
                return;
            };
            if got.wrapping_sub(want) as i32 >= 0 {
                crate::log::info(format!("overlay key: the game took the overlay {} ms after the press (a frame takes it {:.1} ms)", ms(), frame_us as f32 / 1000.0));
                return;
            }
            if ms() > 5000 {
                crate::log::info(format!("overlay key: the game hadn't taken the overlay 5 s after the press (its overlay frame {got}, RivaTuner's {want})"));
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
}

/// What Deck last did to RivaTuner's own switch: (the overlay was on, the switch is on in a
/// running RivaTuner). Shared by the loop and the overlay key, so it is flipped once.
static SWITCH: Mutex<(Option<bool>, bool)> = Mutex::new((None, false));

/// RivaTuner's own switch follows Deck's overlay, like Afterburner's key: on when it comes on (or
/// RivaTuner (re)starts while it is on), off when it is switched off. Deck starting with its
/// overlay off leaves the switch alone. The setting is read under the lock, so two quick presses
/// can't leave the switch at the older one.
fn follow_switch(app: &App, running: bool) {
    let mut g = SWITCH.lock();
    let on = app.settings.lock().osd_on;
    let (was_on, shown) = *g;
    if running {
        if on && !shown {
            set_rtss_osd_visible(true);
        } else if !on && was_on == Some(true) {
            set_rtss_osd_visible(false);
        }
    }
    *g = (Some(on), on && running);
}

/// The overlay key's last press, timed step by step until RivaTuner's switch followed it;
/// written to the log as one line (finding why it shows late in some games).
struct Trace {
    at: Instant,
    how: String,
    steps: Vec<(&'static str, Instant)>,
}
static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

/// The overlay key went down at `at`; `how` says how the key thread learned of it.
pub fn trace_key(at: Instant, how: String) {
    *TRACE.lock() = Some(Trace { at, how, steps: vec![] });
}

fn trace(step: &'static str) {
    if let Some(t) = TRACE.lock().as_mut() {
        t.steps.push((step, Instant::now()));
    }
}

/// Logs the press's line; the time of the press, when there was one.
fn trace_end(on: bool) -> Option<Instant> {
    let t = TRACE.lock().take()?;
    let ms = |i: Instant| i.saturating_duration_since(t.at).as_millis();
    let steps: Vec<String> = t.steps.iter().map(|(n, i)| format!("{n} {}", ms(*i))).collect();
    crate::log::info(format!("overlay key: {} in {} ms ({}); ms after the press: {}", if on { "on" } else { "off" }, t.at.elapsed().as_millis(), t.how, steps.join(", ")));
    Some(t.at)
}

/// Set when something changed (the overlay switched on, items or style edited): the loop stops
/// sleeping and writes straight away.
static WAKE: AtomicBool = AtomicBool::new(false);
/// The loop's thread, woken by `refresh_now` and `stop` instead of checking `WAKE` every 25 ms.
static LOOP: std::sync::OnceLock<std::thread::Thread> = std::sync::OnceLock::new();

fn wake_loop() {
    if let Some(t) = LOOP.get() {
        t.unpark();
    }
}

/// Shows a change at once: writes the overlay from the readings the loop already has (it keeps
/// them fresh even while the overlay is off) and wakes the loop for new ones.
pub fn refresh_now() {
    WAKE.store(true, Ordering::SeqCst);
    wake_loop();
    let Some(app) = APP.get() else { return };
    let (on, items, style) = {
        let s = app.settings.lock();
        (s.osd_on, s.osd_items.clone(), s.osd_render_style())
    };
    let Some((snap, name)) = LAST.lock().clone() else {
        trace("no readings yet");
        return;
    };
    let Some(v) = open_rtss() else {
        trace("RivaTuner not open");
        return;
    };
    let _w = WRITE.lock();
    trace("write lock");
    if STOP.load(Ordering::SeqCst) {
        return;
    }
    if on {
        let (text, width) = osd::osd_text_width(&items, &style, &snap, name.as_deref().unwrap_or(""));
        GRAPH_W.store(width, Ordering::Relaxed);
        write_osd_with(&v, &text, style.graph.then(|| graph_now(&style, snap.fps_avg)));
    } else {
        write_osd(&v, "");
    }
    trace("text written");
}

/// The Sensors page's view: what is installed, the catalogue of items, and the overlay as lines.
pub fn status() -> Value {
    *PREVIEW_AT.lock() = Some(Instant::now()); // the page is open: keep the frame statistics coming
    let last = LAST.lock().clone();
    let (items, style) = APP
        .get()
        .map(|a| {
            let s = a.settings.lock();
            (s.osd_items.clone(), s.osd_render_style())
        })
        .unwrap_or_default();
    let (snap, name) = last.clone().unwrap_or_default();
    json!({
        "installed": rtss_exe().is_some(),
        "running": rtss_running(),
        "items": osd::catalogue(&snap).iter().map(|(id, label, short, cat, stat, have)| json!({"id": id, "label": label, "short": short, "cat": cat, "stat": stat, "have": have})).collect::<Vec<_>>(),
        "cats": osd::CATS.iter().map(|(k, l)| json!([k, l])).collect::<Vec<_>>(),
        "lines": osd::preview(&items, &style, &snap, name.as_deref().unwrap_or("game.exe")),
        "defaults": osd::DEFAULT_ITEMS,
        "fps": snap.fps,
        "frametime": snap.frametime,
        "app": name,
        "profiles": profiles_dir().map(|d| d.join("Global").exists()).unwrap_or(false),
        "frames": FRAMES.lock().0.len(),
        // the overlay is longer than RivaTuner's text field: its last lines are left out
        "trimmed": TRIMMED.load(Ordering::Relaxed),
        // the Sensors page's frametime graph: the last 600 frames, and the figures over the whole window
        "ft_recent": FRAMES.lock().0.recent(600),
        "stats": {
            "avg": snap.fps_avg, "min": snap.fps_min, "max": snap.fps_max, "low1": snap.fps_1low, "low01": snap.fps_01low,
            "secs": APP.get().map(|a| a.settings.lock().osd_style.stats_secs).unwrap_or(60),
        },
    })
}

/// Each core's name ("P-core 0") and its logical processors, read once.
fn core_masks() -> &'static [(String, u64)] {
    static MASKS: std::sync::OnceLock<Vec<(String, u64)>> = std::sync::OnceLock::new();
    MASKS.get_or_init(crate::hw::sysmon::cores)
}

/// Gathers the readings from Deck's GPU engine, the hardware engine and Windows.
fn snapshot(app: &App, sys: &mut sysinfo::System, clocks: &crate::hw::sensors::ThreadClocks, rt: Option<&View>) -> (Snap, Option<String>) {
    let mut s = Snap::default();
    // the clocks MSI Afterburner shows (ratio x bus), so both overlays agree; empty without the driver
    let mut ratio: Vec<Option<f32>> = vec![];
    if let Some(g) = app.gpu_engine() {
        let l = g.nvml.live();
        s.gpu_temp = l.temp_c.map(|v| v as f32);
        s.gpu_usage = l.util.map(|v| v as f32);
        s.gpu_fb = l.mem_util.map(|v| v as f32);
        s.gpu_mem_mb = l.mem_used_mb.map(|v| v as f32);
        s.gpu_mem_total_mb = l.mem_total_mb.map(|v| v as f32);
        s.gpu_clock = l.core_mhz.map(|v| v as f32);
        s.gpu_memclock = l.mem_mhz.map(|v| v as f32);
        s.gpu_power_w = l.power_w;
        if let (Some(w), Ok((_, _, _, def))) = (l.power_w, g.nvml.power_limit()) {
            s.gpu_power_pct = Some(w * 1000.0 / def.max(1) as f32 * 100.0);
        }
        s.gpu_fan = l.fan_pct.first().map(|v| *v as f32);
        s.gpu_limit = l.limit.clone();
        s.gpu_volt_mv = g.api.as_ref().and_then(|a| a.voltage_mv());
    }
    if let Some(h) = app.hw_engine() {
        if let Ok(l) = h.live.lock() {
            s.cpu_temp = l.cpu_t;
            s.cpu_usage = l.cpu_l;
            s.cpu_clock = l.cpu_mhz.map(|v| v as f32);
            ratio = l.thread_ratio_mhz.clone();
            if let Some(m) = ratio.iter().flatten().copied().reduce(f32::max) {
                s.cpu_clock = Some(m);
            }
        }
        if let (Ok(l), Ok(i)) = (h.live.lock(), h.hw.lock()) {
            for (k, name) in i.fan_names.iter().enumerate() {
                if let Some(rpm) = l.fan_rpm.get(k).filter(|r| **r > 0) {
                    s.fans.push((name.clone(), *rpm as f32));
                }
            }
            for (k, name) in i.temp_names.iter().enumerate() {
                if let Some(Some(t)) = l.board_t.get(k) {
                    s.board_temps.push((name.clone(), *t));
                }
            }
        }
        if let Ok(t) = h.table.lock() {
            let mut cores: Vec<CoreRead> = vec![];
            for r in &t.rows {
                if !r.group.starts_with("CPU") {
                    continue;
                }
                let v = r.cur.map(|v| v as f32);
                match r.name.as_str() {
                    "Package power" => s.cpu_power = v,
                    "Core voltage (VID)" => s.cpu_volt = v,
                    n => {
                        let (label, kind) = match (n.strip_suffix(" temperature"), n.strip_suffix(" clock"), n.strip_suffix(" load")) {
                            (Some(l), _, _) => (l, 0),
                            (_, Some(l), _) => (l, 1),
                            (_, _, Some(l)) => (l, 2),
                            _ => continue,
                        };
                        if matches!(label, "Package" | "Core max" | "Total") || label.is_empty() {
                            continue;
                        }
                        let at = match cores.iter().position(|c| c.name == label) {
                            Some(i) => i,
                            None => {
                                cores.push(CoreRead { name: label.to_string(), ..Default::default() });
                                cores.len() - 1
                            }
                        };
                        match kind {
                            0 => cores[at].temp = v,
                            1 => cores[at].clock = v,
                            _ => cores[at].load = v,
                        }
                    }
                }
            }
            if !ratio.is_empty() {
                for c in &mut cores {
                    if let Some((_, m)) = core_masks().iter().find(|(l, _)| *l == c.name) {
                        if let Some(v) = crate::hw::sensors::core_mhz(*m, &ratio) {
                            c.clock = Some(v);
                        }
                    }
                }
            }
            s.core_reads = cores;
        }
    }
    sys.refresh_cpu_usage();
    s.cores = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    if s.cpu_usage.is_none() {
        s.cpu_usage = Some(sys.global_cpu_usage());
    }
    // a core's load is its busiest thread's: the average of a P-core's two threads hid a game's main
    // thread at 100 % (shown as 50 %), the sign of a CPU-bound game
    let loads: Vec<Option<f32>> = s.cores.iter().map(|v| Some(*v)).collect();
    for c in &mut s.core_reads {
        if let Some((_, m)) = core_masks().iter().find(|(l, _)| *l == c.name) {
            if let Some(v) = crate::hw::sensors::core_mhz(*m, &loads) {
                c.load = Some(v);
            }
        }
    }
    // not sysinfo's frequency(): on Windows that is each core's nominal clock, never the live one
    let mut mhz = clocks.read();
    if ratio.len() >= s.cores.len() && !ratio.is_empty() {
        mhz = ratio;
    }
    s.thread_reads = sys
            .cpus()
            .iter()
            .enumerate()
            .map(|(i, c)| CoreRead { name: format!("CPU{}", i + 1), load: Some(c.cpu_usage()), clock: mhz.get(i).copied().flatten().filter(|f| *f > 0.0), temp: None })
            .collect();
    if s.core_reads.is_empty() {
        // no per-core sensors on this CPU: the threads stand in
        s.core_reads = s.thread_reads.clone();
    }
    {
        if let Some(ms) = crate::hw::memclean::status() {
            let mb = |b: u64| (b / (1 << 20)) as f32;
            s.ram_mb = Some(mb(ms.ullTotalPhys - ms.ullAvailPhys));
            s.ram_total_mb = Some(mb(ms.ullTotalPhys));
            s.commit_mb = Some(mb(ms.ullTotalPageFile - ms.ullAvailPageFile));
        }
    }
    let mut app_name = None;
    let mut game = None;
    if let Some((rate, ft, name, pid)) = rt.and_then(fps) {
        s.fps = Some(rate);
        s.frametime = Some(ft);
        app_name = Some(name);
        game = Some(pid);
        if let Some(v) = rt {
            (s.game_api, s.game_res) = unsafe { game_info_at(v.p, v.len, pid) };
        }
    }
    {
        let st = FRAMES.lock().0.stats();
        (s.fps_avg, s.fps_min, s.fps_max, s.fps_1low, s.fps_01low) = (st.avg, st.min, st.max, st.low1, st.low01);
    }
    // the game RivaTuner draws in, not whatever window is in front (a screenshot tool, Discord);
    // a game whose anti-cheat hides its memory shows nothing rather than 0
    if let Some(pid) = game {
        let p = sysinfo::Pid::from_u32(pid);
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), false);
        s.proc_ram_mb = sys.process(p).map(|p| (p.memory() / (1 << 20)) as f32).filter(|mb| *mb > 0.0);
    }
    (s, app_name)
}

// --------------------------------------------------------------------- frametimes from RivaTuner's ring

/// Where an RTSS app entry (shared memory v2.5 and newer) keeps its last 1024 frametimes (µs,
/// `dwStatFrameTimeBuf`) and the next slot it writes (`dwStatFrameTimeBufPos`). RivaTuner fills
/// it for every game it draws in, so nothing has to connect before the game starts.
const FT_BUF: usize = 924;
const FT_LEN: usize = 1024;
const FT_POS: usize = FT_BUF + FT_LEN * 4;

/// The frametimes (ms) written since the ring was at `last`; it is now at `pos`.
fn ring_since(buf: &[u32], last: u32, pos: u32) -> Vec<f32> {
    let n = buf.len() as u32;
    if n == 0 || pos >= n || last >= n {
        return vec![];
    }
    let k = (pos + n - last) % n;
    (0..k).map(|i| buf[((last + i) % n) as usize] as f32 / 1000.0).collect()
}

/// Whose frames count: RivaTuner's foreground game while it draws, else whichever app drew last.
fn pick_app(fg: u32, drew: &std::collections::HashMap<u32, Instant>, now: Instant) -> Option<u32> {
    let fresh = |t: &Instant| now.duration_since(*t) < Duration::from_secs(2);
    if drew.get(&fg).map(fresh).unwrap_or(false) {
        return Some(fg);
    }
    drew.iter().filter(|(_, t)| fresh(t)).max_by_key(|(_, t)| **t).map(|(i, _)| *i)
}

/// Statistics are kept while the overlay is on or the Sensors page is open.
fn stats_wanted() -> bool {
    let Some(app) = APP.get() else { return false };
    app.settings.lock().osd_on || PREVIEW_AT.lock().map(|t| t.elapsed() < Duration::from_secs(10)).unwrap_or(false)
}

/// How long ago process `pid` started, when Windows lets Deck ask.
fn process_age(pid: u32) -> Option<Duration> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u, mut now) = (z, z, z, z, z);
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0;
        CloseHandle(h);
        GetSystemTimeAsFileTime(&mut now);
        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        ok.then(|| Duration::from_nanos(ft(now).saturating_sub(ft(c)).saturating_mul(100)))
    }
}

/// Copies new frametimes out of RivaTuner's rings, twenty times a second, while something wants them.
fn frame_loop() {
    use std::collections::HashMap;
    // app slot → (process, ring position last seen); app slot → when its ring last moved
    let mut seen: HashMap<u32, (u32, u32)> = HashMap::new();
    let mut drew: HashMap<u32, Instant> = HashMap::new();
    // process → (when RivaTuner first listed it, how old it was then, its name) until its first frame is logged
    let mut launch: HashMap<u32, Option<(Instant, Duration, String)>> = HashMap::new();
    let mut view: Option<(View, Instant)> = None;
    while !STOP.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
        let now = Instant::now();
        let window = Duration::from_secs(APP.get().map(|a| a.settings.lock().osd_style.stats_secs).unwrap_or(60).clamp(10, 600) as u64);
        if !stats_wanted() {
            view = None;
            seen.clear();
            drew.clear();
            FRAMES.lock().0.prune(now, Duration::ZERO);
            continue;
        }
        // reopened now and then so a restarted RivaTuner is picked up
        if view.as_ref().map(|v| v.1.elapsed() > Duration::from_secs(5)).unwrap_or(true) {
            view = open_rtss().map(|v| (v, now));
        }
        let Some((v, _)) = view.as_ref() else {
            FRAMES.lock().0.prune(now, window);
            continue;
        };
        let mut fresh: HashMap<u32, Vec<f32>> = HashMap::new();
        let fg;
        unsafe {
            let p = v.p;
            let (size, off, n) = array(p, v.len, 8, APP_ENTRY);
            if rd(p, 4) < 0x0002_0005 || size < FT_POS + 4 {
                continue; // too old a RivaTuner for the ring
            }
            fg = rd(p, 64);
            launch.retain(|pid, _| (0..n).any(|i| rd(p.add(off + i * size), 0) == *pid));
            for i in 0..n {
                let e = p.add(off + i * size);
                let (key, pid) = (i as u32, rd(e, 0));
                if pid == 0 {
                    seen.remove(&key);
                    drew.remove(&key);
                    continue;
                }
                let pos = rd(e, FT_POS);
                // a game started in the last two minutes: log how long it took to show the overlay
                launch.entry(pid).or_insert_with(|| {
                    let raw = std::slice::from_raw_parts(e.add(4), 260);
                    let name = String::from_utf8_lossy(&raw[..raw.iter().position(|&c| c == 0).unwrap_or(260)]).rsplit('\\').next().unwrap_or("").to_string();
                    process_age(pid).filter(|a| *a < Duration::from_secs(120)).map(|a| (now, a, name))
                });
                match seen.insert(key, (pid, pos)) {
                    // the same game moved on: what it wrote since is new; on first sight the ring is old history
                    Some((was, last)) if was == pid && last != pos => {
                        let buf: Vec<u32> = (0..FT_LEN).map(|k| rd(e, FT_BUF + k * 4)).collect();
                        fresh.insert(key, ring_since(&buf, last, pos));
                        drew.insert(key, now);
                        if let Some((hooked, age, name)) = launch.get_mut(&pid).and_then(Option::take) {
                            crate::log::info(format!(
                                "overlay: RivaTuner hooked {name} {:.1} s after it started; its first frame came {:.1} s after that",
                                age.as_secs_f32(),
                                now.duration_since(hooked).as_secs_f32()
                            ));
                        }
                    }
                    Some((was, _)) if was != pid => {
                        drew.remove(&key);
                    }
                    _ => {}
                }
            }
        }
        if let Some(app) = pick_app(fg, &drew, now) {
            let mut f = FRAMES.lock();
            let who = seen.get(&app).map(|(pid, _)| (app, *pid));
            if f.1 != who {
                f.0.clear();
                f.1 = who;
            }
            for ms in fresh.remove(&app).unwrap_or_default() {
                f.0.push(ms, now, window);
            }
        }
        FRAMES.lock().0.prune(now, window);
    }
}

// --------------------------------------------------------------------- RivaTuner's own look

fn profiles_dir() -> Option<std::path::PathBuf> {
    rtss_exe().and_then(|e| e.parent().map(|p| p.join("Profiles")))
}

/// RTSS profiles are ANSI text; keeping every byte as one char makes the round trip lossless.
fn from_ansi(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn to_ansi(s: &str) -> Vec<u8> {
    s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
}

/// Sets `key` inside `[section]` of INI text, leaving everything else as it was.
pub fn set_ini(text: &str, edits: &[(&str, &str, String)]) -> String {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    for (sec, key, val) in edits {
        let head = format!("[{sec}]");
        let Some(i) = lines.iter().position(|l| l.trim().eq_ignore_ascii_case(&head)) else { continue };
        let end = lines[i + 1..].iter().position(|l| l.trim_start().starts_with('[')).map(|k| i + 1 + k).unwrap_or(lines.len());
        let line = format!("{key}={val}");
        let at = lines[i + 1..end].iter().position(|l| l.split_once('=').map(|(k, _)| k.trim().eq_ignore_ascii_case(key)).unwrap_or(false));
        match at {
            Some(k) => lines[i + 1 + k] = line,
            None => lines.insert(i + 1, line),
        }
    }
    let mut out = lines.join(nl);
    if text.ends_with('\n') {
        out.push_str(nl);
    }
    out
}

/// What the look settings mean in RTSS's profile files.
pub fn look_edits(st: &OsdStyle) -> Vec<(&'static str, &'static str, String)> {
    let mx = st.margin_x.clamp(1, 800) as i32;
    let my = st.margin_y.clamp(1, 800) as i32;
    let x = if st.corner.ends_with('r') { -mx } else { mx };
    let y = if st.corner.starts_with('b') { -my } else { my };
    let b = |v: bool| (v as u8).to_string();
    vec![
        // "On-Screen Display support": off would hide Deck's overlay in every game
        ("OSD", "EnableOSD", "1".to_string()),
        ("OSD", "PositionX", x.to_string()),
        ("OSD", "PositionY", y.to_string()),
        ("OSD", "ZoomRatio", st.zoom.clamp(1, 8).to_string()),
        ("OSD", "EnableBgnd", b(st.shadow)),
        ("OSD", "EnableFill", b(st.box_on)),
        ("OSD", "FillColor", format!("{:02X}000000", (st.box_opacity.min(100) * 255 / 100) as u8)),
        // Deck draws the graph as an object in its own text; RivaTuner's own one would be a second copy
        ("OSD", "EnableFrametimeHistory", b(false)),
        ("OSD", "FrametimeHistoryWidth", (-(st.graph_w.clamp(8, 128) as i32)).to_string()),
        ("OSD", "FrametimeHistoryHeight", (-(st.graph_h.clamp(1, 24) as i32)).to_string()),
        ("OSD", "FrametimeHistoryStyle", st.graph_style.min(3).to_string()),
        ("OSD", "MaximumFrametime", st.graph_max.min(1000).to_string()),
        ("OSD", "EnableFrameColorBar", b(st.color_bar)),
        // RivaTuner's own statistics line repeats Deck's framerate figures in another style
        ("OSD", "EnableStat", "0".to_string()),
        ("OSD", "ScaleToFit", b(st.fit)),
        ("OSD", "IntegerFramerate", b(st.int_fps)),
        ("Font", "Face", st.font.clone()),
        ("Font", "Weight", if st.bold { "700" } else { "400" }.to_string()),
    ]
}

/// Writes the look into RivaTuner's global profile (and every game profile it has made), keeps a
/// copy of each original in `Profiles\Deck backup`, and tells running games to reload.
pub fn apply_look(st: &OsdStyle) -> Result<usize, String> {
    let dir = profiles_dir().ok_or("RivaTuner isn't installed.")?;
    let edits = look_edits(st);
    let mut files = vec![dir.join("Global")];
    if st.all_games {
        for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().map(|x| x.eq_ignore_ascii_case("cfg")).unwrap_or(false) && p.file_name().map(|n| n != "Global").unwrap_or(false) {
                files.push(p);
            }
        }
    }
    let mut changed = 0;
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        let text = from_ansi(&bytes);
        if !text.contains("[OSD]") {
            continue;
        }
        let new = set_ini(&text, &edits);
        if new == text {
            continue;
        }
        let keep = dir.join("Deck backup");
        if let Some(name) = f.file_name() {
            let _ = std::fs::create_dir_all(&keep);
            if !keep.join(name).exists() {
                let _ = std::fs::write(keep.join(name), &bytes);
            }
        }
        std::fs::write(&f, to_ansi(&new)).map_err(|e| format!("{}: {e}", f.display()))?;
        changed += 1;
    }
    if changed > 0 {
        request_reload();
    }
    Ok(changed)
}

/// Puts RivaTuner's profile files back the way they were before Deck first changed them.
pub fn restore_look() -> Result<usize, String> {
    let dir = profiles_dir().ok_or("RivaTuner isn't installed.")?;
    let keep = dir.join("Deck backup");
    let mut n = 0;
    for e in std::fs::read_dir(&keep).map_err(|_| "Deck hasn't changed RivaTuner's profiles.".to_string())?.flatten() {
        if e.path().is_file() && std::fs::copy(e.path(), dir.join(e.file_name())).is_ok() {
            n += 1;
        }
    }
    request_reload();
    Ok(n)
}

/// APPFLAG_PROFILE_UPDATE_REQUESTED on every running 3D application.
fn request_reload() {
    let Some(v) = open_rtss() else { return };
    unsafe {
        let (size, off, n) = array(v.p, v.len, 8, APP_ENTRY);
        for i in 0..n {
            let e = v.p.add(off + i * size);
            if rd(e, 0) != 0 {
                (&*(e.add(264) as *const AtomicI32)).fetch_or(0x1000_0000, Ordering::SeqCst);
            }
        }
    }
}

// --------------------------------------------------------------------- the loop

pub fn start(app: Arc<App>) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = APP.set(app.clone());
    if app.settings.lock().rtss_autostart {
        let _ = start_rtss();
    }
    std::thread::Builder::new().name("rtss-frames".into()).spawn(frame_loop).ok();
    std::thread::Builder::new()
        .name("rtss".into())
        .spawn(move || {
            let _ = LOOP.set(std::thread::current());
            let mut sys = sysinfo::System::new();
            let mut net = crate::util::NetCounter::new();
            let mut net_at = Instant::now();
            let clocks = crate::hw::sensors::ThreadClocks::open();
            // a slot left by a Deck that crashed would keep showing frozen numbers
            if let Some(v) = open_rtss() {
                write_osd(&v, "");
            }
            let mut wrote = false;
            let mut tray_at = Instant::now() - Duration::from_secs(60);
            while !STOP.load(Ordering::SeqCst) {
                let (on, items, ms, mahm, style) = {
                    let s = app.settings.lock();
                    (s.osd_on, s.osd_items.clone(), s.osd_ms.clamp(250, 2000), s.mahm_on, s.osd_render_style())
                };
                let rt = open_rtss();
                let read_at = Instant::now();
                let (mut snap, name) = snapshot(&app, &mut sys, &clocks, rt.as_ref());
                if crate::log::debug_on() && crate::log::every("rtss snapshot time", Duration::from_secs(600)) {
                    crate::log::debug(format!("overlay: one reading of the sensors took {} ms", read_at.elapsed().as_millis()));
                }
                {
                    let dt = net_at.elapsed().as_secs_f32().max(0.2);
                    net_at = Instant::now();
                    let (rx, tx) = net.sample();
                    snap.net_down = Some(rx as f32 / dt / 1048576.0);
                    snap.net_up = Some(tx as f32 / dt / 1048576.0);
                }
                // stop() clears the slot under this lock, so nothing is written after it
                let _w = WRITE.lock();
                if STOP.load(Ordering::SeqCst) {
                    break;
                }
                follow_switch(&app, rt.is_some());
                if let Some(v) = &rt {
                    if on {
                        let (text, width) = osd::osd_text_width(&items, &style, &snap, name.as_deref().unwrap_or(""));
                        GRAPH_W.store(width, Ordering::Relaxed);
                        wrote = write_osd_with(v, &text, style.graph.then(|| graph_now(&style, snap.fps_avg)));
                    } else if wrote {
                        write_osd(v, "");
                        wrote = false;
                    }
                }
                if mahm {
                    let g = app.gpu.lock().clone();
                    let (pci, name_s, drv, mem) = g.map(|g| (g.caps.pci.clone(), g.caps.name.clone(), g.caps.driver.clone(), (snap.gpu_mem_total_mb.unwrap_or(0.0) * 1024.0) as u32)).unwrap_or_default();
                    publish(&mahm_block(&snap, &items, (&pci, &name_s, &drv, mem)));
                } else {
                    unpublish();
                }
                drop(_w);
                if tray_at.elapsed() >= Duration::from_secs(5) && rt.is_some() && app.settings.lock().rtss_tray_hidden {
                    tray_at = Instant::now();
                    set_tray_icon(false);
                }
                *LAST.lock() = Some((snap, name));
                drop(rt);
                // sleep, but wake at once when something changed (`wake_loop`)
                let until = Instant::now() + Duration::from_millis(ms as u64);
                while !WAKE.swap(false, Ordering::SeqCst) && !STOP.load(Ordering::SeqCst) {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    std::thread::park_timeout(left);
                }
            }
        })
        .ok();
}

/// On quit: frees Deck's OSD slot and marks the monitoring memory dead.
pub fn stop() {
    let _w = WRITE.lock();
    STOP.store(true, Ordering::SeqCst);
    wake_loop();
    if let Some(v) = open_rtss() {
        write_osd(&v, "");
    }
    unpublish();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any program can create RivaTuner's shared memory first, so its header is not trusted: every
    /// entry Deck reads or writes lies inside the mapping, whatever the header claims.
    #[test]
    fn a_lying_rivatuner_header_stays_inside_the_mapping() {
        let mut m = vec![0u8; 2000];
        let put = |m: &mut Vec<u8>, at: usize, v: u32| m[at..at + 4].copy_from_slice(&v.to_le_bytes());
        // 100 entries of 300 bytes from offset 100: only 6 fit in 2000 bytes
        put(&mut m, 8, 300);
        put(&mut m, 12, 100);
        put(&mut m, 16, 100);
        assert_eq!(unsafe { array(m.as_ptr(), m.len(), 8, 284) }, (300, 100, 6));
        // an offset past the end, an entry too small to hold the fields read, or a mapping smaller than the header: nothing
        put(&mut m, 12, 5000);
        assert_eq!(unsafe { array(m.as_ptr(), m.len(), 8, 284) }.2, 0);
        put(&mut m, 12, 100);
        put(&mut m, 8, 8);
        assert_eq!(unsafe { array(m.as_ptr(), m.len(), 8, 284) }.2, 0);
        assert_eq!(unsafe { array(m.as_ptr(), 40, 8, 284) }.2, 0);
        // the framerate reader uses it: a header pointing far outside reads nothing
        put(&mut m, 8, 300);
        put(&mut m, 12, u32::MAX - 10);
        put(&mut m, 16, 255);
        assert_eq!(unsafe { fps_at(m.as_ptr(), m.len(), 0, None) }, None);
    }

    /// (DllCharacteristics, extended DLL characteristics) of a PE file.
    fn pe_flags(b: &[u8]) -> (u16, u32) {
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let pe = u32_at(0x3C) as usize;
        let (sections, opt_size, opt) = (u16_at(pe + 6) as usize, u16_at(pe + 20) as usize, pe + 24);
        let dirs = opt + if u16_at(opt) == 0x20B { 112 } else { 96 };
        let (dbg_rva, dbg_size) = (u32_at(dirs + 6 * 8) as usize, u32_at(dirs + 6 * 8 + 4) as usize);
        let to_off = |rva: usize| {
            (0..sections).map(|i| opt + opt_size + i * 40).find_map(|s| {
                let (va, len, raw) = (u32_at(s + 12) as usize, (u32_at(s + 8).max(u32_at(s + 16))) as usize, u32_at(s + 20) as usize);
                (va..va + len).contains(&rva).then(|| rva - va + raw)
            })
        };
        let mut ex = 0;
        if let Some(d) = to_off(dbg_rva) {
            for e in (d..d + dbg_size).step_by(28) {
                if u32_at(e + 12) == 20 {
                    ex = u32_at(u32_at(e + 24) as usize);
                }
            }
        }
        (u16_at(opt + 70), ex)
    }

    /// Exploit mitigations on both programs Deck runs with admin rights: ASLR, DEP and Control Flow
    /// Guard; Deck itself also CET shadow stacks (.cargo/config.toml, build.rs).
    #[test]
    fn deck_and_its_helper_are_built_with_exploit_mitigations() {
        const DYNAMIC_BASE: u16 = 0x40;
        const NX: u16 = 0x100;
        const GUARD_CF: u16 = 0x4000;
        const CET_COMPAT: u32 = 0x1;
        let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let (dll, ex) = pe_flags(&me);
        assert_eq!(dll & (DYNAMIC_BASE | NX | GUARD_CF), DYNAMIC_BASE | NX | GUARD_CF, "Deck: {dll:#x}");
        assert_eq!(ex & CET_COMPAT, CET_COMPAT, "Deck: CETCOMPAT missing ({ex:#x})");
        if !RTSS32.is_empty() {
            let (dll, _) = pe_flags(RTSS32);
            assert_eq!(dll & (DYNAMIC_BASE | NX | GUARD_CF), DYNAMIC_BASE | NX | GUARD_CF, "helper: {dll:#x}");
        }
    }

    /// An elevated Deck starts the helper only from its own (admin-only) folder.
    #[test]
    fn an_elevated_deck_never_runs_the_helper_from_a_user_folder() {
        let local = crate::util::env_path("LOCALAPPDATA").unwrap();
        assert!(!helper_dirs(true).iter().any(|d| d.starts_with(&local)));
        assert!(helper_dirs(false).iter().any(|d| d.starts_with(&local)));
    }

    /// The helper runs on PCs without the Visual C++ runtime (Deck itself links it in), so it must
    /// not import VCRUNTIME140.dll. Empty when the i686 target isn't installed (nothing to check).
    #[test]
    fn the_32_bit_helper_needs_no_visual_cpp_runtime() {
        let lower: Vec<u8> = RTSS32.iter().map(u8::to_ascii_lowercase).collect();
        assert!(!lower.windows(16).any(|w| w == b"vcruntime140.dll"), "rtss-osd32.exe imports VCRUNTIME140.dll");
    }

    #[test]
    fn layout_sizes() {
        assert_eq!(MAHM_ENTRY, 1324);
        assert_eq!(MAHM_GPU, 1304);
        let s = Snap { gpu_temp: Some(65.0), cores: vec![10.0, 20.0], ..Default::default() };
        let b = mahm_block(&s, &["gpu_temp".into()], ("VEN_10DE", "", "RTX 3070", 8 << 20));
        assert_eq!(u32::from_le_bytes(b[0..4].try_into().unwrap()), 0x4D41484D, "'MAHM' as a C multi-char constant");
        assert_eq!(u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize, SOURCES.len() + 2);
        assert_eq!(b.len(), 32 + (SOURCES.len() + 2) * 1324 + 1304);
        // first entry: GPU temperature 65, flagged for the OSD
        assert_eq!(&b[32..47], b"GPU temperature");
        assert_eq!(f32::from_le_bytes(b[32 + 1300..32 + 1304].try_into().unwrap()), 65.0);
        assert_eq!(u32::from_le_bytes(b[32 + 1312..32 + 1316].try_into().unwrap()), 1);
        // a missing value is FLT_MAX
        assert_eq!(f32::from_le_bytes(b[32 + 1324 + 1300..32 + 1324 + 1304].try_into().unwrap()), f32::MAX);
        // the per-core entries are named like Afterburner's
        let o = 32 + SOURCES.len() * 1324;
        assert_eq!(&b[o..o + 10], b"CPU1 usage");
    }

    #[test]
    #[ignore]
    fn rtss_tray_icon() {
        println!("{:?}", super::rtss_windows());
        super::set_tray_icon(false);
        std::thread::sleep(std::time::Duration::from_secs(4));
        super::set_tray_icon(true);
    }

    #[test]
    fn degree_sign_is_one_ansi_byte() {
        assert!(ansi("65°C").contains(&0xB0) && ansi("65°C").len() == 4);
        assert_eq!(ansi("– FPS"), b"- FPS", "a missing value");
    }

    #[test]
    fn profile_editing_keeps_the_rest() {
        let src = "[OSD]\r\nEnableOSD=1\r\nPositionX=-1\r\nZoomRatio=2\r\n[Font]\r\nFace=Unispace\r\n[Info]\r\nTimestamp=1\r\n";
        let st = OsdStyle { corner: "bl".into(), margin_x: 20, margin_y: 20, zoom: 3, font: "Consolas".into(), ..Default::default() };
        let out = set_ini(src, &look_edits(&st));
        assert!(out.contains("PositionX=20\r\n") && out.contains("PositionY=-20\r\n") && out.contains("ZoomRatio=3\r\n"), "{out}");
        assert!(out.contains("Face=Consolas") && out.contains("EnableOSD=1") && out.contains("Timestamp=1\r\n"));
        assert!(out.contains("Weight=400"), "added when missing");
        assert!(out.ends_with("\r\n"));
        assert_eq!(set_ini(&out, &look_edits(&st)), out, "a second run changes nothing");
        assert_eq!(set_ini("[Other]\nA=1\n", &look_edits(&st)), "[Other]\nA=1\n", "sections that aren't there are not invented");
    }

    /// Read-only look at what RivaTuner records per app (needs RTSS running).
    #[test]
    #[ignore]
    fn dump_rtss_apps() {
        let v = open_rtss().expect("RTSS shared memory");
        unsafe {
            let p = v.p;
            let (size, off, n) = (rd(p, 8) as usize, rd(p, 12) as usize, rd(p, 16) as usize);
            println!("version {:#x} entry {size} n {n} fg {} fgpid {}", rd(p, 4), rd(p, 64), rd(p, 68));
            println!("mapping {} bytes; within it: apps {:?}, OSD slots {:?}; fps {:?}", v.len, array(p, v.len, 8, APP_ENTRY), array(p, v.len, 20, OSD_SLOT), fps(&v));
            let fe = p.add(off + rd(p, 64) as usize * size);
            for _ in 0..6 {
                println!("count {} pos {} frames {} t1 {}", rd(fe, 920), rd(fe, 5020), rd(fe, 276), rd(fe, 272));
                std::thread::sleep(Duration::from_millis(40));
            }
            for i in 0..n.min(256) {
                let e = p.add(off + i * size);
                if rd(e, 0) == 0 {
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(e.add(4) as *const i8).to_string_lossy().to_string();
                let buf: Vec<u32> = (0..1024).map(|k| rd(e, 924 + k * 4)).collect();
                let nz = buf.iter().filter(|x| **x != 0).count();
                println!(
                    "#{i} pid {} {name} frames {} ft {} statflags {:#x} statmin/avg/max {} {} {} ftcount {} bufpos {} bufrate {} nonzero {nz} sample {:?} low1 {} low01 {} lowpos {} {}",
                    rd(e, 0), rd(e, 276), rd(e, 280), rd(e, 284), rd(e, 304), rd(e, 308), rd(e, 312), rd(e, 920), rd(e, 5020), rd(e, 5024), &buf[..8],
                    rd(e, 9176), rd(e, 9180), rd(e, 9184), rd(e, 9188)
                );
            }
        }
    }

    /// RivaTuner 7.3.5+ (shared memory v2.20) has a 32 KB text field after the object buffer, the
    /// one its own Overlay Editor writes; older ones only the 4 KB one.
    #[test]
    fn long_overlays_go_to_rivatuners_big_text_field() {
        assert_eq!(text_field(0x0002_0015, 299_520), (OSD_OBJECTS + 262_144, 32_768));
        assert_eq!(text_field(0x0002_0013, 299_520), (512, 4096), "too old a RivaTuner");
        assert_eq!(text_field(0x0002_0015, 266_752), (512, 4096), "a slot too small for the big field");
    }

    /// Text over the field's size is cut at the end of a whole line, never inside a colour tag
    /// (which garbled everything after it).
    #[test]
    fn a_too_long_overlay_loses_whole_lines() {
        let t = b"<C0=FF>GPU 50\nCPU 40\nRAM 8";
        assert_eq!(fit_lines(t, 64), &t[..], "it fits");
        assert_eq!(fit_lines(t, 22), b"<C0=FF>GPU 50\nCPU 40", "room for 21 bytes and the closing zero");
        assert_eq!(fit_lines(t, 10), b"", "not even one line fits");
    }

    #[test]
    fn ring_gives_only_the_frames_written_since() {
        let mut buf = vec![0u32; 8];
        for (i, v) in [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000].iter().enumerate() {
            buf[i] = *v;
        }
        assert_eq!(ring_since(&buf, 2, 5), vec![3.0, 4.0, 5.0]);
        assert_eq!(ring_since(&buf, 6, 1), vec![7.0, 8.0, 1.0], "wraps around the end");
        assert!(ring_since(&buf, 3, 3).is_empty(), "nothing new");
        assert!(ring_since(&buf, 3, 9).is_empty(), "a position past the end is nonsense");
        assert_eq!(FT_POS, 5020, "dwStatFrameTimeBufPos in RTSS_SHARED_MEMORY_APP_ENTRY");
    }

    /// A fake RivaTuner block: header (entry size, offset, count, foreground app) and app entries.
    fn rtss_block(fg: u32, apps: &[(u32, &str, u32)]) -> Vec<u8> {
        let (size, off) = (300usize, 256usize);
        let mut b = vec![0u8; off + size * apps.len()];
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 8, size as u32);
        put(&mut b, 12, off as u32);
        put(&mut b, 16, apps.len() as u32);
        put(&mut b, 68, fg);
        for (i, (pid, name, frames)) in apps.iter().enumerate() {
            let e = off + i * size;
            put(&mut b, e, *pid);
            b[e + 4..e + 4 + name.len()].copy_from_slice(name.as_bytes());
            put(&mut b, e + 268, 1000);
            put(&mut b, e + 272, 2000);
            put(&mut b, e + 276, *frames);
            put(&mut b, e + 280, 16600);
        }
        b
    }

    #[test]
    fn the_game_ram_belongs_to_the_game_rivatuner_draws_in() {
        let b = rtss_block(900, &[(500, "C:\\Games\\League of Legends.exe", 60), (600, "C:\\Other\\menu.exe", 30)]);
        let (rate, ft, name, pid) = unsafe { fps_at(b.as_ptr(), b.len(), 2500, None) }.expect("a game is drawing");
        assert_eq!((rate, ft, name.as_str(), pid), (60.0, 16.6, "League of Legends.exe", 500), "a screenshot tool in front (pid 900) is not the game");
        let b = rtss_block(600, &[(500, "a.exe", 60), (600, "b.exe", 30)]);
        assert_eq!(unsafe { fps_at(b.as_ptr(), b.len(), 2500, None) }.map(|x| x.3), Some(600), "RivaTuner's own foreground app wins");
        assert_eq!(unsafe { let b = rtss_block(1, &[]); fps_at(b.as_ptr(), b.len(), 2500, None) }, None);
    }

    /// The framerate, frametime and game RAM come from the game the lows and the average count, so
    /// one line never mixes two programs (a launcher drawing at 300 FPS beside the game).
    #[test]
    fn the_framerate_comes_from_the_game_the_figures_follow() {
        let b = rtss_block(500, &[(500, "launcher.exe", 300), (600, "game.exe", 60)]);
        assert_eq!(unsafe { fps_at(b.as_ptr(), b.len(), 2500, Some(600)) }.map(|x| x.3), Some(600));
        assert_eq!(unsafe { fps_at(b.as_ptr(), b.len(), 2500, Some(42)) }.map(|x| x.3), Some(500), "a followed game that left: RivaTuner's foreground app");
    }

    /// RivaTuner keeps an app's last framerate after it stops drawing (alt-tabbed, paused, a
    /// closed window it still lists): a period that ended over 2.5 s ago is no framerate at all.
    #[test]
    fn a_game_that_stopped_drawing_has_no_framerate() {
        let b = rtss_block(500, &[(500, "game.exe", 60)]);
        assert!(unsafe { fps_at(b.as_ptr(), b.len(), 4400, None) }.is_some(), "the period ended at 2000");
        assert_eq!(unsafe { fps_at(b.as_ptr(), b.len(), 4600, None) }, None);
        assert_eq!(unsafe { fps_at(b.as_ptr(), b.len(), 4600, Some(500)) }, None, "following it doesn't keep it alive");
    }

    /// The preview's API and resolution are the game's own (a D3D11 game at 1920x1080 showed
    /// "D3D12" and the desktop's size), read from RivaTuner's entry for it; nothing when unknown.
    #[test]
    fn the_api_and_resolution_are_the_games_own() {
        // an entry big enough for the v2.20 resolution fields (dwResolutionX at 9224)
        let (size, off) = (9300usize, 256usize);
        let mut b = vec![0u8; off + size];
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 4, 0x0002_0015);
        put(&mut b, 8, size as u32);
        put(&mut b, 12, off as u32);
        put(&mut b, 16, 1);
        put(&mut b, off, 500);
        put(&mut b, off + 264, 0x0001_0007); // x64, D3D11
        put(&mut b, off + 9224, 1920);
        put(&mut b, off + 9228, 1080);
        assert_eq!(unsafe { game_info_at(b.as_ptr(), b.len(), 500) }, (Some("D3D11"), Some((1920, 1080))));
        assert_eq!(unsafe { game_info_at(b.as_ptr(), b.len(), 501) }, (None, None), "another process");
        put(&mut b, off + 264, 0x0001_000A);
        put(&mut b, 4, 0x0002_0013);
        assert_eq!(unsafe { game_info_at(b.as_ptr(), b.len(), 500) }, (Some("Vulkan"), None), "a RivaTuner too old for the resolution");
        put(&mut b, off + 264, 0x0001_0000);
        assert_eq!(unsafe { game_info_at(b.as_ptr(), b.len(), 500) }.0, None, "no API recorded yet");
    }

    #[test]
    fn frames_follow_the_game_that_is_drawing() {
        use std::collections::HashMap;
        let now = Instant::now();
        let old = now - Duration::from_secs(5);
        let mut drew = HashMap::new();
        assert_eq!(pick_app(0, &drew, now), None);
        drew.insert(2, now);
        drew.insert(3, now - Duration::from_millis(500));
        assert_eq!(pick_app(3, &drew, now), Some(3), "RivaTuner's foreground game wins while it draws");
        assert_eq!(pick_app(7, &drew, now), Some(2), "otherwise the one that drew last");
        drew.insert(3, old);
        assert_eq!(pick_app(3, &drew, now), Some(2), "a game that stopped drawing doesn't hold on");
    }

    /// Reads two seconds of the drawing app's frames the way `frame_loop` does (needs a game in RTSS).
    #[test]
    #[ignore]
    fn live_ring_stats() {
        let v = open_rtss().expect("RTSS shared memory");
        unsafe {
            let p = v.p;
            let (size, off, n) = (rd(p, 8) as usize, rd(p, 12) as usize, rd(p, 16) as usize);
            let pos0: Vec<u32> = (0..n).map(|i| rd(p.add(off + i * size), FT_POS)).collect();
            let mut w = FrameWindow::new();
            let mut last = pos0.clone();
            let t0 = Instant::now();
            let mut app = None;
            while t0.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(50));
                for i in 0..n {
                    let e = p.add(off + i * size);
                    let pos = rd(e, FT_POS);
                    if rd(e, 0) != 0 && pos != last[i] && app.map(|a| a == i).unwrap_or(true) {
                        app = Some(i);
                        let buf: Vec<u32> = (0..FT_LEN).map(|k| rd(e, FT_BUF + k * 4)).collect();
                        for ms in ring_since(&buf, last[i], pos) {
                            w.push(ms, Instant::now(), Duration::from_secs(60));
                        }
                    }
                    last[i] = pos;
                }
            }
            let name = app.map(|i| std::ffi::CStr::from_ptr(p.add(off + i * size + 4) as *const i8).to_string_lossy().to_string());
            println!("app {name:?} frames {} stats {:?}", w.len(), w.stats());
        }
    }

    /// Read-only: how long after the probe starts Deck's slot shows up (switch the overlay on meanwhile).
    #[test]
    #[ignore]
    fn time_slot_appears() {
        let v = open_rtss().expect("RTSS shared memory");
        let t0 = Instant::now();
        let mut was = None;
        while t0.elapsed() < Duration::from_secs(8) {
            let ours = unsafe {
                let p = v.p;
                let (size, off, n) = (rd(p, 20) as usize, rd(p, 24) as usize, rd(p, 28) as usize);
                (1..n).any(|i| std::slice::from_raw_parts(p.add(off + i * size + 256), 5) == b"Deck ")
            };
            if was != Some(ours) {
                println!("{:>5} ms (clock {}) Deck's slot {}", t0.elapsed().as_millis(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 100_000, if ours { "there" } else { "empty" });
                was = Some(ours);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Read-only: what Deck's OSD slot holds (needs Deck and RTSS running).
    #[test]
    #[ignore]
    fn dump_deck_slot() {
        let v = open_rtss().expect("RTSS shared memory");
        unsafe {
            let p = v.p;
            let (size, off, n) = (rd(p, 20) as usize, rd(p, 24) as usize, rd(p, 28) as usize);
            for i in 0..n {
                let e = p.add(off + i * size);
                let owner = std::ffi::CStr::from_ptr(e.add(256) as *const i8).to_string_lossy().to_string();
                if owner.is_empty() {
                    continue;
                }
                let text = std::ffi::CStr::from_ptr(e.add(text_field(rd(p, 4), size).0) as *const i8).to_string_lossy().to_string();
                let obj: Vec<u8> = (0..36).map(|k| *e.add(OSD_OBJECTS + k)).collect();
                println!("slot {i} owner {owner:?} size {size} text tail {:?}
object {:?}", &text[text.len().saturating_sub(60)..], obj);
            }
        }
    }

    #[test]
    fn rtss_signature() {
        assert_eq!(&RTSS_SIG.to_le_bytes(), b"SSTR");
        assert_eq!(MAHM_SIG, 0x4D41484D);
        assert_eq!(RTSS_SIG, 0x52545353);
    }
}
