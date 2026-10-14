//! RivaTuner Statistics Server feed, replacing what MSI Afterburner gave RTSS:
//! - the on-screen display: Deck writes its text into one RTSS OSD slot (owner "Deck"), RTSS
//!   draws it in games and fills the framerate macros itself (no injection by Deck);
//! - `MAHMSharedMemory`, Afterburner's monitoring memory (v2.0 layout from the RTSS SDK), so
//!   RTSS OverlayEditor's "MSI Afterburner" data source and other readers keep working.
//! Layouts: RTSS SDK `RTSSSharedMemory.h` (v2.x) and `MSIAB\MAHMSharedMemory.h`.

use crate::app::App;
use crate::osd::{self, FrameWindow};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE};

pub use crate::osd::OsdStyle;

/// One second's readings.
#[derive(Default, Clone, Debug)]
pub struct Snap {
    pub gpu_temp: Option<f32>,
    pub gpu_usage: Option<f32>,
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
        let view = View { h, p: v.Value as *mut u8 };
        (rd(view.p, 0) == RTSS_SIG && rd(view.p, 4) >= 0x0002_0000).then_some(view)
    }
}

unsafe fn rd(p: *const u8, off: usize) -> u32 {
    std::ptr::read_volatile(p.add(off) as *const u32)
}

/// Finds Deck's slot (or claims a free one; slot 0 belongs to RTSS's own clients) and writes.
/// Empty `text` releases the slot.
fn write_osd(v: &View, text: &str) -> bool {
    write_osd_with(v, text, None)
}

/// The frametime graph's points, fed by `frame_loop`, and the overlay's width in characters
/// (from the last text written) for a graph that fits it.
static GRAPH: Mutex<(osd::GraphFeed, usize)> = Mutex::new((osd::GraphFeed::new(), 0));

/// Starts the framerate figures and the graph over (the reset key and the buttons).
pub fn reset_stats() {
    FRAMES.lock().0.clear();
    GRAPH.lock().0.clear();
}

/// The graph object as it should be now.
fn graph_now(st: &OsdStyle) -> Vec<u8> {
    let g = GRAPH.lock();
    osd::graph_object(st, g.1, &g.0.points.iter().copied().collect::<Vec<_>>())
}

/// Puts a new graph into Deck's slot between text updates, so the graph moves smoothly.
fn write_graph(v: &View, graph: &[u8]) -> bool {
    unsafe {
        let p = v.p;
        let (size, off, n) = (rd(p, 20) as usize, rd(p, 24) as usize, rd(p, 28) as usize);
        if rd(p, 4) < 0x0002_000C || size < OSD_OBJECTS + graph.len() || n < 2 {
            return false;
        }
        let owner = |e: *mut u8| std::slice::from_raw_parts(e.add(256), 256);
        let Some(e) = (1..n).map(|i| p.add(off + i * size)).find(|e| owner(*e).starts_with(OWNER) && owner(*e)[OWNER.len()] == 0) else { return false };
        let busy = &*(p.add(36) as *const AtomicI32);
        for _ in 0..20 {
            if busy.fetch_or(1, Ordering::SeqCst) & 1 == 0 {
                std::ptr::copy_nonoverlapping(graph.as_ptr(), e.add(OSD_OBJECTS), graph.len());
                (&*(p.add(32) as *const AtomicI32)).fetch_add(1, Ordering::SeqCst);
                busy.fetch_and(!1, Ordering::SeqCst);
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}

/// The OSD entry's object buffer (RTSS_SHARED_MEMORY_OSD_ENTRY.buffer, after szOSD, szOSDOwner and szOSDEx).
const OSD_OBJECTS: usize = 256 + 256 + 4096;

/// `graph`: an embedded object to put at the start of the slot's object buffer, for `<OBJ=00000000>`.
fn write_osd_with(v: &View, text: &str, graph: Option<Vec<u8>>) -> bool {
    unsafe {
        let p = v.p;
        let (size, off, n) = (rd(p, 20) as usize, rd(p, 24) as usize, rd(p, 28) as usize);
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
                let n = bytes.len().min(4095);
                std::ptr::write_bytes(e.add(512), 0, 4096);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), e.add(512), n);
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

/// Framerate and frametime of the foreground app as RTSS measured them.
fn fps(v: &View) -> Option<(f32, f32, String)> {
    unsafe {
        let p = v.p;
        let (size, off, n) = (rd(p, 8) as usize, rd(p, 12) as usize, rd(p, 16) as usize);
        // dwLastForegroundAppProcessID sits after dwDesktopVideoCaptureFlags + Stat[5] + dwLastForegroundApp
        let fg_pid = rd(p, 40 + 4 + 20 + 4);
        let mut best: Option<(f32, f32, String)> = None;
        for i in 0..n.min(256) {
            let e = p.add(off + i * size);
            let pid = rd(e, 0);
            if pid == 0 {
                continue;
            }
            let (t0, t1, frames, ft) = (rd(e, 268), rd(e, 272), rd(e, 276), rd(e, 280));
            if t1 <= t0 || frames == 0 {
                continue;
            }
            let rate = 1000.0 * frames as f32 / (t1 - t0) as f32;
            let name = std::ffi::CStr::from_ptr(e.add(4) as *const i8).to_string_lossy().to_string();
            let name = name.rsplit('\\').next().unwrap_or("").to_string();
            let item = (rate, ft as f32 / 1000.0, name);
            if pid == fg_pid {
                return Some(item);
            }
            if best.as_ref().map(|b| b.0 < item.0).unwrap_or(true) {
                best = Some(item);
            }
        }
        best
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
/// the foreground game's recent frametimes, and the RTSS app slot they came from
static FRAMES: Mutex<(FrameWindow, u32)> = Mutex::new((FrameWindow::new(), u32::MAX));
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
    use windows::core::{HSTRING, PCSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    type SetFlags = unsafe extern "C" fn(u32, u32) -> u32;
    const OSD_VISIBLE: u32 = 1; // RTSSHOOKSFLAG_OSD_VISIBLE in the RTSS SDK
    let Some(dir) = rtss_exe().and_then(|e| e.parent().map(|p| p.to_path_buf())) else { return };
    if let Some(helper) = rtss32_helper() {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new(helper)
            .arg(if visible { "on" } else { "off" })
            .arg(&dir)
            .creation_flags(crate::util::CREATE_NO_WINDOW)
            .status();
    }
    static SET64: OnceLock<Option<SetFlags>> = OnceLock::new();
    let set = SET64.get_or_init(|| unsafe {
        let m = LoadLibraryW(&HSTRING::from(dir.join("RTSSHooks64.dll").as_os_str())).ok()?;
        Some(std::mem::transmute::<_, SetFlags>(GetProcAddress(m, PCSTR(c"SetFlags".as_ptr() as *const u8))?))
    });
    if let Some(set) = *set {
        unsafe { set(!OSD_VISIBLE, if visible { OSD_VISIBLE } else { 0 }) };
    }
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
            .map(|f| std::mem::transmute::<_, unsafe extern "C" fn() -> u32>(f)() & 1 != 0)
    };
    (v32, v64)
}

/// The 32-bit helper, built by build.rs (empty when the i686 target wasn't installed).
static RTSS32: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rtss-osd32.exe"));

/// Writes the helper next to Deck (or into %LOCALAPPDATA%\Deck) the first time it's needed.
fn rtss32_helper() -> Option<std::path::PathBuf> {
    if RTSS32.is_empty() {
        return None;
    }
    let dirs = [std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())), crate::util::env_path("LOCALAPPDATA").map(|p| p.join("Deck"))];
    for d in dirs.into_iter().flatten() {
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

pub fn rtss_exe() -> Option<std::path::PathBuf> {
    let base = crate::util::env_path("ProgramFiles(x86)").unwrap_or_else(|| "C:\\Program Files (x86)".into());
    let p = base.join("RivaTuner Statistics Server").join("RTSS.exe");
    p.exists().then_some(p)
}

pub fn rtss_running() -> bool {
    open_rtss().is_some()
}

/// Starts RTSS (Deck runs as admin; RTSS needs it).
pub fn start_rtss() -> Result<(), String> {
    if rtss_running() {
        return Ok(());
    }
    let exe = rtss_exe().ok_or("RivaTuner Statistics Server is not installed.")?;
    std::process::Command::new(&exe).current_dir(exe.parent().unwrap()).spawn().map(|_| ()).map_err(|e| e.to_string())
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
        let last = text.lines().map(str::trim).filter(|l| l.chars().any(char::is_alphanumeric)).last().unwrap_or("no details");
        return Err(format!("RivaTuner didn't install ({last}). Get it from guru3d.com."));
    }
    start_rtss()
}

/// Overlay on/off (the hotkey and the Sensors page switch).
pub fn toggle(app: &App) {
    let on = {
        let mut s = app.settings.lock();
        s.osd_on = !s.osd_on;
        s.osd_on
    };
    app.save_settings();
    if on && !rtss_running() {
        let _ = start_rtss();
    }
    refresh_now();
}

/// Set when something changed (the overlay switched on, items or style edited): the loop stops
/// sleeping and writes straight away.
static WAKE: AtomicBool = AtomicBool::new(false);

/// Shows a change at once: writes the overlay from the readings the loop already has (it keeps
/// them fresh even while the overlay is off) and wakes the loop for new ones.
pub fn refresh_now() {
    WAKE.store(true, Ordering::SeqCst);
    let Some(app) = APP.get() else { return };
    let (on, items, style) = {
        let s = app.settings.lock();
        (s.osd_on, s.osd_items.clone(), s.osd_render_style())
    };
    let Some((snap, name)) = LAST.lock().clone() else { return };
    let Some(v) = open_rtss() else { return };
    let _w = WRITE.lock();
    if STOP.load(Ordering::SeqCst) {
        return;
    }
    if on {
        let (text, width) = osd::osd_text_width(&items, &style, &snap, name.as_deref().unwrap_or(""));
        GRAPH.lock().1 = width;
        write_osd_with(&v, &text, style.graph.then(|| graph_now(&style)));
    } else {
        write_osd(&v, "");
    }
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
        // the Sensors page's frametime graph: the last 600 frames, and the figures over the whole window
        "ft_recent": FRAMES.lock().0.recent(600),
        "graph": GRAPH.lock().0.points.iter().copied().collect::<Vec<_>>(),
        "stats": {
            "avg": snap.fps_avg, "min": snap.fps_min, "max": snap.fps_max, "low1": snap.fps_1low, "low01": snap.fps_01low,
            "secs": APP.get().map(|a| a.settings.lock().osd_style.stats_secs).unwrap_or(60),
        },
    })
}

/// Gathers the readings from Deck's GPU engine, the hardware engine and Windows.
fn snapshot(app: &App, sys: &mut sysinfo::System, rt: Option<&View>) -> (Snap, Option<String>) {
    let mut s = Snap::default();
    if let Some(g) = app.gpu_engine() {
        let l = g.nvml.live();
        s.gpu_temp = l.temp_c.map(|v| v as f32);
        s.gpu_usage = l.util.map(|v| v as f32);
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
            s.core_reads = cores;
        }
    }
    sys.refresh_cpu_usage();
    s.cores = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    if s.cpu_usage.is_none() {
        s.cpu_usage = Some(sys.global_cpu_usage());
    }
    sys.refresh_cpu_frequency();
    s.thread_reads = sys
            .cpus()
            .iter()
            .enumerate()
            .map(|(i, c)| CoreRead { name: format!("CPU{}", i + 1), load: Some(c.cpu_usage()), clock: Some(c.frequency() as f32).filter(|f| *f > 0.0), temp: None })
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
    if let Some((rate, ft, name)) = rt.and_then(fps) {
        s.fps = Some(rate);
        s.frametime = Some(ft);
        app_name = Some(name);
    }
    {
        let st = FRAMES.lock().0.stats();
        (s.fps_avg, s.fps_min, s.fps_max, s.fps_1low, s.fps_01low) = (st.avg, st.min, st.max, st.low1, st.low01);
    }
    if let Some(pid) = foreground_pid() {
        let p = sysinfo::Pid::from_u32(pid);
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), false);
        s.proc_ram_mb = sys.process(p).map(|p| (p.memory() / (1 << 20)) as f32);
    }
    (s, app_name)
}

fn foreground_pid() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        (pid != 0).then_some(pid)
    }
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

/// Copies new frametimes out of RivaTuner's rings, twenty times a second, while something wants them.
fn frame_loop() {
    use std::collections::HashMap;
    // app slot → (process, ring position last seen); app slot → when its ring last moved
    let mut seen: HashMap<u32, (u32, u32)> = HashMap::new();
    let mut drew: HashMap<u32, Instant> = HashMap::new();
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
            let (size, off, n) = (rd(p, 8) as usize, rd(p, 12) as usize, rd(p, 16) as usize);
            if rd(p, 4) < 0x0002_0005 || size < FT_POS + 4 {
                continue; // too old a RivaTuner for the ring
            }
            fg = rd(p, 64);
            for i in 0..n.min(256) {
                let e = p.add(off + i * size);
                let (key, pid) = (i as u32, rd(e, 0));
                if pid == 0 {
                    seen.remove(&key);
                    drew.remove(&key);
                    continue;
                }
                let pos = rd(e, FT_POS);
                match seen.insert(key, (pid, pos)) {
                    // the same game moved on: what it wrote since is new; on first sight the ring is old history
                    Some((was, last)) if was == pid && last != pos => {
                        let buf: Vec<u32> = (0..FT_LEN).map(|k| rd(e, FT_BUF + k * 4)).collect();
                        fresh.insert(key, ring_since(&buf, last, pos));
                        drew.insert(key, now);
                    }
                    Some((was, _)) if was != pid => {
                        drew.remove(&key);
                    }
                    _ => {}
                }
            }
        }
        let (graph_on, style) = APP.get().map(|a| { let s = a.settings.lock(); (s.osd_on && s.osd_style.graph, s.osd_style.clone()) }).unwrap_or_default();
        let mut new_frames = vec![];
        if let Some(app) = pick_app(fg, &drew, now) {
            let mut f = FRAMES.lock();
            if f.1 != app {
                f.0.clear();
                f.1 = app;
                GRAPH.lock().0.clear();
            }
            new_frames = fresh.remove(&app).unwrap_or_default();
            for ms in &new_frames {
                f.0.push(*ms, now, window);
            }
        }
        let span = Duration::from_secs(style.graph_secs.clamp(2, 120) as u64);
        let stepped = GRAPH.lock().0.feed(&new_frames, now, span, style.graph_smooth);
        if stepped && graph_on {
            let g = graph_now(&style);
            let _w = WRITE.lock();
            if !STOP.load(Ordering::SeqCst) {
                write_graph(v, &g);
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
        let (size, off, n) = (rd(v.p, 8) as usize, rd(v.p, 12) as usize, rd(v.p, 16) as usize);
        for i in 0..n.min(256) {
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
            let mut sys = sysinfo::System::new();
            let mut net = crate::util::NetCounter::new();
            let mut net_at = Instant::now();
            // a slot left by a Deck that crashed would keep showing frozen numbers
            if let Some(v) = open_rtss() {
                write_osd(&v, "");
            }
            let mut wrote = false;
            let mut shown = false;
            let mut was_on: Option<bool> = None;
            while !STOP.load(Ordering::SeqCst) {
                let (on, items, ms, mahm, style) = {
                    let s = app.settings.lock();
                    (s.osd_on, s.osd_items.clone(), s.osd_ms.clamp(250, 2000), s.mahm_on, s.osd_render_style())
                };
                let rt = open_rtss();
                let (mut snap, name) = snapshot(&app, &mut sys, rt.as_ref());
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
                // RivaTuner's own switch follows Deck's overlay, like Afterburner's key: on when it
                // comes on (or RivaTuner (re)starts while it is on), off when it is switched off.
                // Deck starting with its overlay off leaves the switch alone.
                if rt.is_some() {
                    if on && !shown {
                        set_rtss_osd_visible(true);
                    } else if !on && was_on == Some(true) {
                        set_rtss_osd_visible(false);
                    }
                }
                shown = on && rt.is_some();
                was_on = Some(on);
                if let Some(v) = &rt {
                    if on {
                        let (text, width) = osd::osd_text_width(&items, &style, &snap, name.as_deref().unwrap_or(""));
                        GRAPH.lock().1 = width;
                        wrote = write_osd_with(v, &text, style.graph.then(|| graph_now(&style)));
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
                *LAST.lock() = Some((snap, name));
                drop(rt);
                // sleep, but wake at once when something changed
                let t0 = Instant::now();
                while t0.elapsed() < Duration::from_millis(ms as u64) && !WAKE.swap(false, Ordering::SeqCst) && !STOP.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        })
        .ok();
}

/// On quit: frees Deck's OSD slot and marks the monitoring memory dead.
pub fn stop() {
    let _w = WRITE.lock();
    STOP.store(true, Ordering::SeqCst);
    if let Some(v) = open_rtss() {
        write_osd(&v, "");
    }
    unpublish();
}

#[cfg(test)]
mod tests {
    use super::*;

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
                let text = std::ffi::CStr::from_ptr(e.add(512) as *const i8).to_string_lossy().to_string();
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
