//! MoboCtl's hardware engine inside Deck: fan chips and curves, board/RAM RGB, the cooler
//! display, sensors, CPU power mode, all on worker threads. Deck's UI talks to [Shared].

use crate::util::LockAnyway;
pub mod cfg;
pub mod cpupower;
pub mod devices;
pub mod fx;
pub mod game;
pub mod hid;
pub mod hp;
pub mod kv;
pub mod lcd;
pub mod loopback;
pub mod memclean;
pub mod pawn;
pub mod powermode;
pub mod rapl;
pub mod screen;
pub mod sensors;
pub mod sio;
pub mod smart;
pub mod smbios;
pub mod sysmon;
pub mod winutil;

use crate::hw::devices::*;
use crate::hw::fx::*;
use crate::hw::sensors::*;
use crate::hw::kv::*;
use crate::hw::sio::{Control, FanChip};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};
use std::sync::{Arc, Mutex};
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};

/// Temperatures every fan can follow; the board chip's own sensors are appended after these.
pub const BASE_SOURCES: [&str; 4] = ["CPU", "GPU", "CPU/GPU hotter", "CPU/GPU average"];

/// What was found on this PC (filled at start-up, shown on the "This PC" page and in the report).
#[derive(Clone)]
pub struct HwInfo {
    pub board: crate::hw::smbios::Info,
    pub chip: String,             // fan chip name, or the reason none was found
    pub control: Control,
    pub fan_names: Vec<String>,
    pub temp_names: Vec<String>,
    pub cpu_brand: String,
    pub cpu_temp_note: String,
    pub gpu_name: String,
    pub chip_report: String,
}

#[derive(Default, Clone)]
pub struct Live {
    pub cpu_t: Option<f32>,   // what everything uses: the CPU's own sensor, else the board's CPU sensor
    pub cpu_msr: Option<f32>, // the CPU's own sensor
    pub cpu_board: Option<f32>,
    pub cpu_l: Option<f32>,
    pub cpu_mhz: Option<u32>,
    pub gpu_t: Option<f32>,
    pub gpu_l: Option<f32>,
    pub fan_pct: Vec<Option<f32>>,
    pub fan_rpm: Vec<u32>,
    pub board_t: Vec<Option<f32>>, // matches HwInfo::temp_names
    pub volts: Vec<(String, f32)>,
    pub audio: crate::hw::loopback::Levels,
    pub screen: Vec<Rgb>,
    pub screen_avg: Rgb,
}

impl Live {
    pub fn source(&self, src: usize) -> Option<f32> {
        let both = |f: fn(f32, f32) -> f32| match (self.cpu_t, self.gpu_t) {
            (Some(a), Some(b)) => Some(f(a, b)),
            (a, b) => a.or(b),
        };
        match src {
            0 => self.cpu_t,
            1 => self.gpu_t,
            2 => both(f32::max),
            3 => both(|a, b| (a + b) / 2.0),
            n => self.board_t.get(n - 4).copied().flatten(),
        }
    }
}

/// Five minutes of once-a-second samples for the Monitor page (NaN = no reading).
pub const HIST: usize = 300;
#[derive(Default, Clone)]
pub struct History {
    pub cpu_t: VecDeque<f32>,
    pub gpu_t: VecDeque<f32>,
    pub cpu_l: VecDeque<f32>,
    pub gpu_l: VecDeque<f32>,
    /// CPU package and GPU board power, W
    pub cpu_w: VecDeque<f32>,
    pub gpu_w: VecDeque<f32>,
    pub board: Vec<VecDeque<f32>>,
    pub rpm: Vec<VecDeque<f32>>,
}

fn push(q: &mut VecDeque<f32>, v: Option<f32>) {
    q.push_back(v.unwrap_or(f32::NAN));
    while q.len() > HIST {
        q.pop_front();
    }
}

/// Fan behaviour shared by all channels plus each channel's temperature source.
#[derive(Clone)]
pub struct FanCfg {
    pub src: Vec<usize>,
    pub hyst: f32,  // C the temperature must fall before fans slow down
    pub delay: f32, // s between slow-downs
    pub stop: f32,  // system fans stop below this C (0 = never)
    pub allow_experimental: bool,
}

#[derive(Default, Clone)]
pub struct Status {
    pub rgb: (String, bool),
    pub ram: String,
    pub disp: (String, bool),
    pub fans: Option<String>, // error text, None = fine
}

/// The lights test: every zone red, green and blue (~15 s) with the lights held. `None` when a test
/// already holds them. The hold is always released: HidD_GetFeature has no timeout and a stalled
/// controller (or a panic) used to leave it on, which froze every lighting control until Deck restarted.
pub fn lights_test(sh: &Arc<Shared>) -> Option<String> {
    if sh.rgb_hold.swap(true, Relaxed) {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(rgb_diag());
    });
    let r = rx.recv_timeout(Duration::from_secs(40)).unwrap_or_else(|_| "The light controller didn't finish the test within 40 s (it stopped answering); the test was abandoned.\r\n".into());
    sh.reconnect.store(true, Relaxed);
    sh.rgb_hold.store(false, Relaxed);
    Some(r)
}

pub struct Shared {
    pub s: Settings,
    pub hw: Mutex<HwInfo>,
    chip: Mutex<Option<Box<dyn FanChip>>>,
    pub ep: Mutex<EffectParams>,
    pub dp: Mutex<DisplayParams>,
    pub zmask: AtomicUsize,
    pub n1: AtomicUsize,
    pub n2: AtomicUsize,
    pub modes: Mutex<Vec<String>>,
    pub fancfg: Mutex<FanCfg>,
    pub curves: Mutex<Vec<Curve>>,
    pub live: Mutex<Live>,
    pub hist: Mutex<History>,
    pub table: Mutex<crate::hw::sysmon::Table>,
    pub status: Mutex<Status>,
    pub running: AtomicBool,
    /// the lights-off button: RGB and RAM go dark, the chosen mode is kept
    pub lights_off: AtomicBool,
    /// a full-screen game is in front right now
    pub in_game: AtomicBool,
    /// what the power-mode switch last did, shown under its buttons
    pub power_msg: Mutex<String>,
    /// short form for the narrow space beside the buttons ("15 / 25 W")
    pub power_short: Mutex<String>,
    /// the minimum-clock lock was released automatically because the CPU got too hot
    pub lock_released: AtomicBool,
    /// the lights self-test is running: the board thread stays off the controller
    pub rgb_hold: AtomicBool,
    pub reconnect: AtomicBool,
    pub dirty: AtomicBool,
    pub t0: Instant,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        // find the hardware first: the UI needs the fan/sensor names to load settings
        let board = crate::hw::smbios::read();
        let (chip, hw) = match crate::hw::sio::open(&board) {
            Ok(c) => {
                let hw = HwInfo { board: board.clone(), chip: c.name(), control: c.control(), fan_names: c.fan_names(), temp_names: c.temp_names(), cpu_brand: String::new(), cpu_temp_note: String::new(), gpu_name: String::new(), chip_report: c.report() };
                (Some(c), hw)
            }
            Err(e) => (None, HwInfo { board: board.clone(), chip: if e.contains("did not appear") { "No supported fan chip on this PC (laptops usually control their fans themselves)".to_string() } else { e }, control: Control::ReadOnly("no fan chip".into()), fan_names: vec![], temp_names: vec![], cpu_brand: String::new(), cpu_temp_note: String::new(), gpu_name: String::new(), chip_report: String::new() }),
        };
        let n = hw.fan_names.len();
        let nt = hw.temp_names.len();
        let status = Status { ram: "RAM: scanning...".into(), fans: chip.is_none().then(|| hw.chip.clone()), ..Default::default() };
        Arc::new(Shared {
            s: Settings::load(),
            hw: Mutex::new(hw),
            chip: Mutex::new(chip),
            ep: Mutex::new(EffectParams::default()),
            dp: Mutex::new(DisplayParams::default()),
            zmask: AtomicUsize::new(7),
            n1: AtomicUsize::new(60),
            n2: AtomicUsize::new(60),
            modes: Mutex::new(vec!["Auto (BIOS)".to_string(); n]),
            fancfg: Mutex::new(FanCfg { src: vec![0; n], hyst: 2.0, delay: 3.0, stop: 0.0, allow_experimental: false }),
            curves: Mutex::new(Curve::load_all()),
            live: Mutex::new(Live { fan_pct: vec![None; n], fan_rpm: vec![0; n], board_t: vec![None; nt], ..Default::default() }),
            table: Mutex::new(crate::hw::sysmon::Table::default()),
            hist: Mutex::new(History { board: vec![VecDeque::new(); nt], rpm: vec![VecDeque::new(); n], ..Default::default() }),
            status: Mutex::new(status),
            running: AtomicBool::new(true),
            lights_off: AtomicBool::new(false),
            in_game: AtomicBool::new(false),
            power_msg: Mutex::new(String::new()),
            power_short: Mutex::new(String::new()),
            lock_released: AtomicBool::new(false),
            rgb_hold: AtomicBool::new(false),
            reconnect: AtomicBool::new(true),
            dirty: AtomicBool::new(true),
            t0: Instant::now(),
        })
    }
    pub fn t(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }
    fn up(&self) -> bool {
        self.running.load(Relaxed)
    }
    /// The lighting effect actually shown (Off while the lights-off button is on).
    pub fn effect(&self) -> EffectParams {
        let mut p = self.ep.lock_anyway().clone();
        if self.lights_off.load(Relaxed) {
            p.mode = "Off".into();
        } else if p.auto_game && self.in_game.load(Relaxed) {
            p.mode = "Game".into();
        }
        p
    }

    pub fn fx_in(&self) -> FxIn {
        let l = self.live.lock_anyway();
        FxIn { temp: l.cpu_t.unwrap_or(40.0), audio: l.audio, screen: l.screen.clone(), screen_avg: l.screen_avg }
    }
    /// All temperature sources a fan can follow, by name.
    pub fn sources(&self) -> Vec<String> {
        let mut v: Vec<String> = BASE_SOURCES.iter().map(|s| s.to_string()).collect();
        v.extend(self.hw.lock_anyway().temp_names.iter().map(|n| format!("Board: {}", n)));
        v
    }
    /// Whether Deck may write to the fan chip right now.
    pub fn fan_control_allowed(&self) -> bool {
        match self.hw.lock_anyway().control {
            Control::Verified => true,
            Control::Experimental => self.fancfg.lock_anyway().allow_experimental,
            Control::ReadOnly(_) => false,
        }
    }
}

/// True when the CPU (own sensor, else the board's) is at or above the lock cut-off temperature.
fn l_temp_hot(cpu: Option<f32>, board: Option<f32>) -> bool {
    cpu.or(board).map(|t| t >= 92.0).unwrap_or(false)
}

/// Target duty for a mode string; None = hand the header back to the BIOS.
fn target(sh: &Shared, mode: &str, temp: Option<f32>) -> Option<f32> {
    if mode.starts_with("Auto") {
        return None;
    }
    if let Some(v) = mode.strip_prefix("Fixed ") {
        return v.trim_end_matches('%').parse().ok();
    }
    if let Some(n) = mode.strip_prefix("Curve: ") {
        let c = sh.curves.lock_anyway().iter().find(|c| c.name == n)?.clone();
        return Some(temp.map(|t| c.eval(t)).unwrap_or(100.0)); // no temperature reading -> be safe
    }
    None
}

pub struct Workers {
    pub fans: JoinHandle<()>,
    pub others: Vec<JoinHandle<()>>,
}

/// Appends one line with every sensor of the table; a new header is written whenever the set of sensors changes.
fn csv_line(path: &std::path::Path, table: &crate::hw::sysmon::Table, last_header: &mut String) {
    use std::io::Write;
    let header = std::iter::once("time".to_string()).chain(table.rows.iter().map(|r| format!("{} / {} [{}]", r.group, r.name, r.unit).replace(',', ";"))).collect::<Vec<_>>().join(",");
    let rotate = std::fs::metadata(path).map(|m| m.len() > 50_000_000).unwrap_or(false);
    let f = if rotate { std::fs::File::create(path) } else { std::fs::OpenOptions::new().create(true).append(true).open(path) };
    let Ok(mut f) = f else { return };
    if rotate || *last_header != header {
        let _ = writeln!(f, "{}", header);
        *last_header = header;
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let vals: Vec<String> = table.rows.iter().map(|r| r.cur.map(|v| format!("{:.*}", r.dec, v)).unwrap_or_default()).collect();
    let _ = writeln!(f, "{},{}", secs, vals.join(","));
}

/// A header that may stop below the "stop" temperature: one named as a case fan.
pub fn fan_may_stop(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    (l.contains("system") || l.contains("chassis") || l.contains("case") || l.starts_with("sys")) && !l.contains("cpu") && !l.contains("pump") && !l.contains("aio")
}

/// Sleeps in short slices so quitting never waits long for a worker.
fn nap(s: &Shared, ms: u64) {
    let t0 = Instant::now();
    while s.up() && t0.elapsed() < Duration::from_millis(ms) {
        sleep(Duration::from_millis(100));
    }
}

pub fn spawn_all(sh: &Arc<Shared>) -> Workers {
    let mut others = vec![];

    // sensors: CPU/GPU temperature + load once a second; history, CSV log and heat alerts
    let s = sh.clone();
    others.push(spawn(move || {
        let mut cpu = Cpu::open();
        let gpu = Gpu::open();
        {
            let mut hw = s.hw.lock_anyway();
            hw.cpu_brand = cpu.brand.clone();
            hw.cpu_temp_note = cpu.error.clone().unwrap_or_default();
            hw.gpu_name = if gpu.name.is_empty() { "no supported GPU (NVIDIA / AMD Radeon) found".into() } else { gpu.name.clone() };
        }
        let mut last_alert: [Option<Instant>; 2] = [None, None];
        let mut hot_secs = 0u32;
        let mut drives = crate::hw::sysmon::drives();
        let mut net = crate::hw::sysmon::Net::new();
        let mut csv_header = String::new();
        while s.up() {
            let (ct, cl, gt, gl, mhz) = (cpu.temp(), cpu.load(), gpu.temp(), gpu.load(), cpu.clock());
            // safety: a locked minimum clock must never cook the CPU - give the lock up after a few seconds above 92 C
            if s.s.int("CpuMinSet", 0) == 1 && l_temp_hot(ct, s.live.lock_anyway().cpu_board) {
                hot_secs += 1;
                if hot_secs >= 4 {
                    let lim = crate::hw::cpupower::limits();
                    crate::hw::cpupower::apply_min(None, lim.turbo.max(lim.base), &s.s);
                    s.s.set_bool("CpuMinOn", false);
                    s.s.save();
                    s.lock_released.store(true, Relaxed);
                    hot_secs = 0;
                }
            } else {
                hot_secs = 0;
            }
            let l = {
                let mut l = s.live.lock_anyway();
                l.cpu_msr = ct;
                l.cpu_t = ct.or(l.cpu_board);
                l.cpu_l = cl;
                l.cpu_mhz = mhz;
                l.gpu_t = gt;
                l.gpu_l = gl;
                l.clone()
            };
            {
                let mut h = s.hist.lock_anyway();
                push(&mut h.cpu_t, l.cpu_t);
                push(&mut h.gpu_t, l.gpu_t);
                push(&mut h.cpu_l, l.cpu_l);
                push(&mut h.gpu_l, l.gpu_l);
                for (i, q) in h.board.iter_mut().enumerate() {
                    push(q, l.board_t.get(i).copied().flatten());
                }
                for (i, q) in h.rpm.iter_mut().enumerate() {
                    push(q, l.fan_rpm.get(i).map(|r| *r as f32));
                }
            }
            // the HWiNFO-style table: every sensor with current / min / max / average
            {
                use crate::hw::sysmon::s as smp;
                let hw = s.hw.lock_anyway().clone();
                let mut all = cpu.details(ct, cl);
                all.extend(gpu.details(gt, gl));
                let bg = format!("Motherboard: {} {}", hw.board.board_vendor.replace("Micro-Star International Co., Ltd.", "MSI"), hw.board.board_name);
                if !hw.fan_names.is_empty() || !hw.temp_names.is_empty() {
                    let bg = format!("{} ({})", bg, hw.chip);
                    for (i, n) in hw.temp_names.iter().enumerate() {
                        all.push(smp(&bg, &format!("{} temperature", n), "°C", 1, l.board_t.get(i).copied().flatten().map(|v| v as f64)));
                    }
                    for (n, v) in &l.volts {
                        all.push(smp(&bg, &format!("{} voltage", n), "V", 3, Some(*v as f64)));
                    }
                    for (i, n) in hw.fan_names.iter().enumerate() {
                        all.push(smp(&bg, &format!("{} speed", n), "RPM", 0, l.fan_rpm.get(i).map(|r| *r as f64)));
                    }
                    for (i, n) in hw.fan_names.iter().enumerate() {
                        all.push(smp(&bg, &format!("{} duty", n), "%", 0, l.fan_pct.get(i).copied().flatten().map(|v| v as f64)));
                    }
                }
                all.extend(crate::hw::sysmon::memory());
                for d in drives.iter_mut() {
                    all.extend(d.sample());
                }
                all.extend(net.sample());
                {
                    let watts = |grp: &str, name: &str| all.iter().find(|r| r.group.starts_with(grp) && r.name == name && r.unit == "W").and_then(|r| r.v).map(|v| v as f32);
                    let mut h = s.hist.lock_anyway();
                    push(&mut h.cpu_w, watts("CPU:", "Package power"));
                    push(&mut h.gpu_w, watts("GPU:", "Power"));
                }
                let mut t = s.table.lock_anyway();
                t.update(all);
                if s.s.bool("LogCsv", false) {
                    csv_line(&dir().join("monitor.csv"), &t, &mut csv_header);
                }
            }
            if s.s.bool("AlertOn", false) {
                let checks = [(l.cpu_t, s.s.int("AlertCpu", 90) as f32, "CPU"), (l.gpu_t, s.s.int("AlertGpu", 85) as f32, "GPU")];
                for (k, (v, lim, name)) in checks.iter().enumerate() {
                    if let Some(v) = v {
                        if *v >= *lim && last_alert[k].map(|t| t.elapsed().as_secs() > 300).unwrap_or(true) {
                            last_alert[k] = Some(Instant::now());
                            crate::hw::winutil::notify(&format!("{} is hot: {:.0}°C", name, v), &format!("Above your {:.0}°C alert limit. Check the fans and airflow.", lim));
                        }
                    }
                }
            }
            sleep(Duration::from_millis(1000));
        }
    }));

    // power mode (saver / balanced / performance): applied when chosen, re-asserted if the firmware resets the limits
    let s = sh.clone();
    others.push(spawn(move || {
        let rapl = crate::hw::rapl::Rapl::open();
        let mut done: Option<i32> = None;
        while s.up() {
            let want = s.s.int("PowerMode", crate::hw::powermode::BALANCED);
            let pl1 = s.s.int("PowerPl1", 0) as f32;
            let drift = rapl.as_ref().map(|r| crate::hw::powermode::drifted(want, r) || (want == crate::hw::powermode::CUSTOM && crate::hw::powermode::drifted_custom(pl1, r))).unwrap_or(false);
            // a new custom wattage is a change too
            let want_key = want * 1_000_000 + if want == crate::hw::powermode::CUSTOM { (pl1 as i32).clamp(0, 999) * 1000 + s.s.int("PowerPl2", 0).clamp(0, 999) } else { 0 };
            if done != Some(want_key) && !(done.is_none() && want == crate::hw::powermode::BALANCED && s.s.int("PowerTouched", 0) == 0) || drift {
                let msg = crate::hw::powermode::apply(want, &s.s, rapl.as_ref());
                *s.power_msg.lock_anyway() = format!("{}: {}", if want == crate::hw::powermode::CUSTOM { "Custom" } else { crate::hw::powermode::NAMES[want.clamp(0, 2) as usize] }, msg);
                *s.power_short.lock_anyway() = rapl.as_ref().and_then(|r| r.read()).map(|l| format!("{:.0} / {:.0} W", l.pl1, l.pl2)).unwrap_or_default();
            }
            done = Some(want_key);
            nap(&s, 2000);
        }
    }));

    // fans
    let s = sh.clone();
    let fans = spawn(move || {
        let mut chip = s.chip.lock_anyway().take();
        let n = chip.as_ref().map(|c| c.fan_names().len()).unwrap_or(0);
        let cpu_idx = chip.as_ref().and_then(|c| c.cpu_temp());
        let mut last: Vec<Option<f32>> = vec![None; n];
        let mut written: Vec<Option<(u8, Instant)>> = vec![None; n];
        let mut driven = vec![false; n];
        let mut used: Vec<Option<(f32, Instant)>> = vec![None; n]; // temperature the curve follows + when it last fell
        let mut stopped = vec![false; n];
        // only fans named as case fans may stop; CPU, pump and unnamed ("Fan 3") headers never do
        let may_stop: Vec<bool> = chip.as_ref().map(|c| c.fan_names()).unwrap_or_default().iter().map(|name| fan_may_stop(name)).collect();
        while s.up() {
            if let Some(io) = chip.as_mut() {
                let rd = io.read();
                let live = {
                    let mut l = s.live.lock_anyway();
                    if let Some(r) = rd {
                        l.fan_pct = r.pct;
                        l.fan_rpm = r.rpm;
                        l.cpu_board = cpu_idx.and_then(|i| r.temps.get(i).copied().flatten());
                        l.board_t = r.temps;
                        l.volts = r.volts;
                        if l.cpu_msr.is_none() {
                            l.cpu_t = l.cpu_board; // e.g. a CPU whose own sensor can't be read
                        }
                    }
                    l.clone()
                };
                let allowed = s.fan_control_allowed();
                let modes = s.modes.lock_anyway().clone();
                let cfg = s.fancfg.lock_anyway().clone();
                for i in 0..n {
                    let mode = if allowed { modes.get(i).map(|m| m.as_str()).unwrap_or("Auto (BIOS)") } else { "Auto (BIOS)" };
                    // the channel's own source, falling back to the CPU if that sensor has no reading
                    let raw = live.source(cfg.src.get(i).copied().unwrap_or(0)).or(live.cpu_t);
                    // right after start the first reading may not exist yet; leave the BIOS in charge meanwhile
                    if raw.is_none() && s.t0.elapsed().as_secs() < 20 && !mode.starts_with("Auto") {
                        continue;
                    }
                    // hysteresis + response time: rises follow at once, falls only after `hyst` C and `delay` s
                    let temp = match (raw, used[i]) {
                        (Some(t), Some((u, at))) => {
                            if t >= u || (u - t >= cfg.hyst && at.elapsed().as_secs_f32() >= cfg.delay) {
                                used[i] = Some((t, Instant::now()));
                                Some(t)
                            } else {
                                Some(u)
                            }
                        }
                        (Some(t), None) => {
                            used[i] = Some((t, Instant::now()));
                            Some(t)
                        }
                        (None, _) => None,
                    };
                    match target(&s, mode, temp) {
                        None => {
                            if driven[i] {
                                io.release(i);
                                driven[i] = false;
                                last[i] = None;
                                written[i] = None;
                                stopped[i] = false;
                            }
                        }
                        Some(mut t) => {
                            let hot = live.cpu_t.map(|x| x >= 85.0).unwrap_or(false) || temp.map(|x| x >= 90.0).unwrap_or(false);
                            if hot {
                                t = 100.0;
                            }
                            // optional fan stop for case fans, 3 C to restart
                            let can_stop = may_stop.get(i).copied().unwrap_or(false) && cfg.stop > 0.0 && mode.starts_with("Curve") && !hot;
                            if can_stop {
                                if let Some(tt) = temp {
                                    stopped[i] = if stopped[i] { tt < cfg.stop + 3.0 } else { tt < cfg.stop };
                                }
                            } else {
                                stopped[i] = false;
                            }
                            let v = if stopped[i] {
                                0.0
                            } else {
                                // never below 20%, and ramp downwards gently so fans don't hunt
                                let cur = last[i].filter(|c| *c > 0.0).unwrap_or(t); // first takeover / restart: straight to the curve value
                                if t >= cur { t } else { t.max(cur - 4.0) }.clamp(20.0, 100.0)
                            };
                            last[i] = Some(v);
                            let b = v.round() as u8;
                            let stale = written[i].map(|(w, at)| w != b || at.elapsed().as_secs() >= 10).unwrap_or(true);
                            if stale {
                                io.set_percent(i, v);
                                written[i] = Some((b, Instant::now()));
                                driven[i] = true;
                            }
                        }
                    }
                }
            }
            sleep(Duration::from_millis(1000));
        }
        if let Some(io) = chip.as_mut() {
            io.release_all(); // always give the fans back to the BIOS on exit
        }
    });

    // music level (only captures while a lighting mode uses it)
    let s = sh.clone();
    others.push(spawn(move || {
        let mut lb: Option<crate::hw::loopback::Loopback> = None;
        let mut retry = Instant::now() - Duration::from_secs(10);
        let mut gm: Option<crate::hw::game::GameMeter> = None;
        let mut imp = crate::hw::game::Impact::default();
        let mut gpid: Option<u32> = None;
        let mut gcheck = Instant::now() - Duration::from_secs(10);
        while s.up() {
            let ef = s.effect();
            let game = ef.mode == "Game" && ef.game_sound != "Off";
            let want = ef.mode == "Music" || game || {
                let d = s.dp.lock_anyway();
                !d.off && (d.content == "Music meter" || (d.content == "Rotate" && d.rotate.iter().any(|c| c == "Music meter")))
            };
            if want {
                if lb.is_none() && retry.elapsed().as_secs() >= 3 {
                    retry = Instant::now();
                    lb = crate::hw::loopback::Loopback::open();
                }
                if game && gm.is_none() {
                    gm = crate::hw::game::GameMeter::open();
                }
                if let Some(l) = lb.as_mut() {
                    let mut a = l.poll();
                    if game {
                        if gcheck.elapsed().as_millis() > 1000 {
                            gcheck = Instant::now();
                            gpid = crate::hw::game::foreground_game();
                        }
                        if let Some(m) = gm.as_mut() {
                            a.game = m.peak(gpid, !ef.game_audio_only);
                            let (i, since) = imp.update(a.game);
                            a.impact = i;
                            a.since_impact = since;
                        }
                    }
                    s.live.lock_anyway().audio = a;
                }
                sleep(Duration::from_millis(15));
            } else {
                lb = None;
                gm = None;
                s.live.lock_anyway().audio = Default::default();
                sleep(Duration::from_millis(250));
            }
        }
    }));

    // screen colours (only captured while the Screen lighting mode is on)
    let s = sh.clone();
    others.push(spawn(move || {
        // held only while a screen mode is on (a duplication session costs the compositor a little)
        let mut grab: Option<crate::hw::screen::Grabber> = None;
        while s.up() {
            let mode = s.effect().mode;
            if mode == "Screen" || mode == "Game" {
                if let Some((cols, avg)) = grab.get_or_insert_with(crate::hw::screen::Grabber::new).sample() {
                    let mut l = s.live.lock_anyway();
                    if mode == "Game" && l.screen.len() == cols.len() {
                        // smooth the scene colours so the lights do not flicker; big scene cuts follow faster
                        let mix = |a: Rgb, b: Rgb| {
                            let diff = (a.r as i32 - b.r as i32).abs() + (a.g as i32 - b.g as i32).abs() + (a.b as i32 - b.b as i32).abs();
                            let k = if diff > 300 { 0.7 } else { 0.3 };
                            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k) as u8;
                            Rgb::new(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b))
                        };
                        let old = std::mem::take(&mut l.screen);
                        l.screen = old.iter().zip(cols.iter()).map(|(a, b)| mix(*a, *b)).collect();
                        l.screen_avg = mix(l.screen_avg, avg);
                    } else {
                        l.screen = cols;
                        l.screen_avg = avg;
                    }
                }
                sleep(Duration::from_millis(50));
            } else {
                grab = None;
                sleep(Duration::from_millis(250));
            }
        }
    }));

    // is a game in front? (only checked when the automatic Game preset is on)
    let s = sh.clone();
    others.push(spawn(move || {
        while s.up() {
            if s.ep.lock_anyway().auto_game {
                s.in_game.store(crate::hw::game::foreground_game().is_some(), Relaxed);
            } else {
                s.in_game.store(false, Relaxed);
            }
            nap(&s, 1500);
        }
    }));

    // motherboard RGB
    let s = sh.clone();
    others.push(spawn(move || {
        let mut board = Board::new();
        let mut last_key: Option<(Rgb, Rgb, Rgb, String)> = None;
        let mut last_send = Instant::now() - Duration::from_secs(10);
        let mut retry = Instant::now() - Duration::from_secs(10);
        let mut hold_since: Option<Instant> = None;
        let mut cfg_check = Instant::now();
        crate::gpu::watch_resume();
        let mut wakes = crate::gpu::WAKES.load(Relaxed);
        let mut wake_at: Option<Instant> = None;
        while s.up() {
            // after sleep the controller can lose its config packet: reconnect a few seconds after wake
            let w = crate::gpu::WAKES.load(Relaxed);
            if w != wakes {
                wakes = w;
                wake_at = Some(Instant::now());
            }
            if wake_at.is_some_and(|t| t.elapsed().as_secs() >= 4) {
                wake_at = None;
                s.reconnect.store(true, Relaxed);
            }
            if s.rgb_hold.load(Relaxed) {
                // a lights test holds the lights for ~15 s; a hold older than a minute is stuck
                // (the controller never answered), and would freeze every lighting control for good
                let since = *hold_since.get_or_insert_with(Instant::now);
                if since.elapsed().as_secs() > 60 {
                    s.rgb_hold.store(false, Relaxed);
                    hold_since = None;
                    s.reconnect.store(true, Relaxed);
                } else {
                    last_key = None;
                    sleep(Duration::from_millis(100));
                    continue;
                }
            } else {
                hold_since = None;
            }
            if s.reconnect.swap(false, Relaxed) || (!board.connected() && retry.elapsed().as_secs() >= 10) {
                retry = Instant::now();
                let err = board.connect(s.n1.load(Relaxed), s.n2.load(Relaxed));
                s.status.lock_anyway().rgb = match &err {
                    None => ("Motherboard RGB connected. Close SignalRGB / MSI Center / Dragon Center - they fight over the same controller.".into(), true),
                    Some(e) => (e.clone(), false),
                };
                last_key = None;
            }
            if board.connected() && cfg_check.elapsed().as_secs() >= 5 {
                cfg_check = Instant::now();
                if !board.config_intact() {
                    // the board lost Deck's config (it ignores colours without it): write it again
                    s.reconnect.store(true, Relaxed);
                }
            }
            if board.connected() {
                if s.dirty.swap(false, Relaxed) {
                    last_key = None;
                }
                let p = s.effect();
                let inp = s.fx_in();
                let t = s.t();
                let z = s.zmask.load(Relaxed);
                let (na, nb) = (board.n1, board.n2);
                let a1: Vec<Rgb> = (0..na).map(|i| if z & 2 != 0 { pixel_in(&p, t, i, na, &inp) } else { Rgb::BLACK }).collect();
                let a2: Vec<Rgb> = (0..nb).map(|i| if z & 4 != 0 { pixel_in(&p, t, i, nb, &inp) } else { Rgb::BLACK }).collect();
                let c12 = if z & 1 != 0 { pixel_in(&p, t, 0, 1, &inp) } else { Rgb::BLACK };
                let key = (c12, a1[0], a2[0], p.mode.clone());
                if !is_static(&p.mode) || last_key.as_ref() != Some(&key) || last_send.elapsed().as_secs() >= 2 {
                    last_key = Some(key);
                    last_send = Instant::now();
                    if !board.send(c12, &a1, &a2) {
                        s.status.lock_anyway().rgb = ("Lost the controller - reconnecting...".into(), false);
                        s.reconnect.store(true, Relaxed);
                        sleep(Duration::from_millis(1000));
                    }
                }
            }
            sleep(Duration::from_millis(33));
        }
    }));

    // RAM (SMBus is slow, so it lives on its own thread)
    let s = sh.clone();
    others.push(spawn(move || {
        let mut ram = FuryRam::new();
        ram.scan();
        s.status.lock_anyway().ram = ram.status.clone() + if !ram.sticks.is_empty() { "  (close Fury CTRL / MSI Center if colours flicker)" } else { "" };
        crate::gpu::watch_resume();
        let mut wakes = crate::gpu::WAKES.load(Relaxed);
        let mut wake_at: Option<Instant> = None;
        // Right after a cold boot the sticks' controllers (or the SMBus) may not answer yet: with
        // fewer RGB sticks found than Fury modules Windows lists, look again, a little less often
        // each time, for 10 minutes after start or wake. Otherwise the RAM keeps its own onboard
        // effect while everything else follows the saved lighting (lights off included).
        let fury = FuryRam::fury_modules();
        let mut retry_from = Instant::now();
        let (mut retry_at, mut retry_gap) = (Instant::now() + Duration::from_secs(5), 5u64);
        while s.up() {
            if ram.sticks.len() < fury && retry_from.elapsed().as_secs() < 600 && Instant::now() >= retry_at {
                ram.scan_missing();
                s.status.lock_anyway().ram = ram.status.clone() + if !ram.sticks.is_empty() { "  (close Fury CTRL / MSI Center if colours flicker)" } else { "" };
                retry_gap = (retry_gap * 2).min(60);
                retry_at = Instant::now() + Duration::from_secs(retry_gap);
            }
            // the sticks power-cycle in sleep and fall back to their onboard effect: rescan (re-enters direct mode)
            let w = crate::gpu::WAKES.load(Relaxed);
            if w != wakes {
                wakes = w;
                wake_at = Some(Instant::now());
            }
            if wake_at.is_some_and(|t| t.elapsed().as_secs() >= 4) {
                wake_at = None;
                ram = FuryRam::new();
                ram.scan();
                retry_from = Instant::now();
                (retry_at, retry_gap) = (Instant::now() + Duration::from_secs(5), 5);
                s.status.lock_anyway().ram = ram.status.clone() + if !ram.sticks.is_empty() { "  (close Fury CTRL / MSI Center if colours flicker)" } else { "" };
            }
            let p = s.effect();
            let inp = s.fx_in();
            ram.frame(&p, s.t(), &inp);
            sleep(Duration::from_millis(if per_led(&p.mode) || p.mode == "Music" { 50 } else { 120 }));
        }
    }));

    // Thermalright cooler display
    let s = sh.clone();
    others.push(spawn(move || {
        let mut disp = CoolerDisplay::new();
        let mut last: Option<Vec<u8>> = None;
        let mut last_t = Instant::now() - Duration::from_secs(10);
        while s.up() {
            if !disp.connected() {
                let ok = disp.connect();
                s.status.lock_anyway().disp = (disp.status.clone(), ok);
                if !ok {
                    nap(&s, 5000);
                    continue;
                }
            }
            let p = s.dp.lock_anyway().clone();
            let l = s.live.lock_anyway().clone();
            let light = Some((s.effect(), s.fx_in()));
            let inp = crate::hw::lcd::DispIn { cpu_t: l.cpu_t, cpu_l: l.cpu_l, gpu_t: l.gpu_t, gpu_l: l.gpu_l, audio: l.audio, light, ..Default::default() }.now();
            let led = crate::hw::lcd::bytes(&crate::hw::lcd::frame(&p, s.t(), &inp));
            if last.as_ref() != Some(&led) || last_t.elapsed().as_secs() >= 1 {
                if !disp.send(&led) {
                    s.status.lock_anyway().disp = ("Lost the cooler display - reconnecting...".into(), false);
                }
                last = Some(led);
                last_t = Instant::now();
            }
            sleep(Duration::from_millis(50));
        }
    }));

    Workers { fans, others }
}

/// Stops every worker; the fan thread hands the fans back to the BIOS before it ends.
pub fn shutdown(sh: &Shared, w: Workers) {
    sh.running.store(false, SeqCst);
    let _ = w.fans.join();
    for h in w.others {
        let _ = h.join();
    }
}

/// Plain-text description of this PC and what Deck can do on it (for "Copy report").
pub fn report(sh: &Shared) -> String {
    let hw = sh.hw.lock_anyway().clone();
    let mut r = format!("Deck {} hardware report\r\n", env!("CARGO_PKG_VERSION"));
    r.push_str(&format!("Motherboard: {} {}  (BIOS {})\r\n", hw.board.board_vendor, hw.board.board_name, hw.board.bios));
    r.push_str(&report_body(sh));
    for (m, p) in &hw.board.dimms {
        r.push_str(&format!("  DIMM: {} {}\r\n", m, p));
    }
    r
}

/// What the engine reads and controls, without the board and memory list (the support report
/// already has those): temperatures, every fan and sensor, the fan chip, lighting, the display.
pub fn report_body(sh: &Shared) -> String {
    let hw = sh.hw.lock_anyway().clone();
    let st = sh.status.lock_anyway().clone();
    let l = sh.live.lock_anyway().clone();
    let deg = |t: Option<f32>| t.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "no reading".into());
    let mut r = String::new();
    let mut line = |s: String| {
        r.push_str(&s);
        r.push_str("\r\n");
    };
    line(format!("CPU: {} · {}{}", hw.cpu_brand, deg(l.cpu_t), if hw.cpu_temp_note.is_empty() { String::new() } else { format!(" ({})", hw.cpu_temp_note) }));
    line(format!("GPU: {} · {} · load {}", hw.gpu_name, deg(l.gpu_t), l.gpu_l.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "?".into())));
    line(format!("Fan chip: {} · control {:?}", hw.chip, hw.control));
    for (i, n) in hw.fan_names.iter().enumerate() {
        let rpm = l.fan_rpm.get(i).copied().unwrap_or(0);
        let pct = l.fan_pct.get(i).copied().flatten().map(|p| format!("{p:.0}%")).unwrap_or_else(|| "?".into());
        line(format!("  {n}: {pct} · {rpm} rpm{}", if rpm == 0 { " (nothing connected, or stopped)" } else { "" }));
    }
    for (i, n) in hw.temp_names.iter().enumerate() {
        line(format!("  {n}: {}", deg(l.board_t.get(i).copied().flatten())));
    }
    if !hw.chip_report.is_empty() {
        line(format!("  {}", hw.chip_report));
    }
    line(format!("Board lighting: {}", st.rgb.0));
    line(format!("RAM lighting: {}", st.ram.strip_prefix("RAM: ").unwrap_or(&st.ram)));
    line(format!("Cooler display: {}", st.disp.0));
    r
}

// ------------------------------------------------------------------ Deck glue

static HW: std::sync::OnceLock<Arc<Shared>> = std::sync::OnceLock::new();
static WORKERS: Mutex<Option<Workers>> = Mutex::new(None);
/// Set once the workers are joined (the fans are back with the BIOS).
static STOPPED: AtomicBool = AtomicBool::new(false);

/// Starts the hardware engine (admin only: the fan/RGB/MSR driver needs it). Safe to call once.
pub fn start(notify: Box<dyn Fn(&str, &str) + Send + Sync>) -> Option<Arc<Shared>> {
    if !crate::elevation::is_admin() {
        return None;
    }
    if let Some(h) = HW.get() {
        return Some(h.clone());
    }
    winutil::set_notify(notify);
    if let Some(m) = kv::auto_import() {
        crate::log::info(format!("hw: {m}"));
    }
    let sh = Shared::new();
    let c = cfg::load(&sh);
    cfg::apply(&sh, &c);
    sh.lights_off.store(sh.s.bool("L_Off", false), Relaxed);
    let w = spawn_all(&sh);
    *WORKERS.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
    let _ = HW.set(sh.clone());
    // fans are Deck's now: if Deck is ended from outside, the watchdog gives them back
    crate::watchdog::start();
    Some(sh)
}

/// Stops every worker; the fans go back to the BIOS. Called on quit, session end and panics.
pub fn stop() {
    let Some(sh) = HW.get() else { return };
    let w = match WORKERS.try_lock() {
        Ok(mut g) => g.take(),
        Err(_) => None,
    };
    match w {
        Some(w) => {
            sh.s.save();
            shutdown(sh, w);
            STOPPED.store(true, SeqCst);
        }
        None => {
            // someone else is stopping: wait for the fans to be released before the process may end
            sh.running.store(false, SeqCst);
            let t0 = Instant::now();
            while !STOPPED.load(SeqCst) && t0.elapsed() < Duration::from_secs(4) {
                sleep(Duration::from_millis(50));
            }
        }
    }
}

thread_local! {
    static WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The current thread is one of the engine's workers (a panic there stops the engine).
pub fn on_worker_thread() -> bool {
    WORKER.with(|w| w.get())
}

/// Spawns a worker thread that knows it is one.
fn spawn<F: FnOnce() + Send + 'static>(f: F) -> JoinHandle<()> {
    std::thread::spawn(move || {
        WORKER.with(|w| w.set(true));
        f()
    })
}

/// From the panic hook: never joins (the panicking thread may be a worker). Stops the workers,
/// gives the fan thread a moment to release the fans itself, then releases them directly.
pub fn on_panic() {
    let Some(sh) = HW.get() else { return };
    if !sh.running.swap(false, SeqCst) {
        return; // already stopping
    }
    std::thread::sleep(Duration::from_millis(1300));
    let _ = release_fans_direct();
}

/// Last resort after a crash: hands every fan header straight back to the BIOS.
pub fn release_fans_direct() -> String {
    match sio::open(&smbios::read()) {
        Ok(mut s) => {
            s.release_all();
            format!("Fans released ({}).", s.name())
        }
        Err(e) => e,
    }
}

/// Which Hardware pages have something to show on this PC.
pub fn pages(sh: Option<&Shared>, gpu: bool) -> Vec<&'static str> {
    let mut p = vec!["cpu"];
    if gpu {
        p.push("gpu");
    }
    match sh {
        None => p.extend(["fans", "lighting", "sensors", "thispc"]), // not admin: the pages explain "Set up"
        Some(sh) => {
            if !sh.hw.lock_anyway().fan_names.is_empty() {
                p.push("fans");
            }
            let st = sh.status.lock_anyway().clone();
            if st.rgb.1 || st.ram.contains("stick(s) at") {
                p.push("lighting");
            }
            if st.disp.1 {
                p.push("lcd");
            }
            p.extend(["sensors", "thispc"]);
        }
    }
    p
}

/// `--hw-selftest`: what the engine finds on this PC. Reads only (no RGB, no fan writes).
pub fn selftest() -> String {
    let mut o = String::new();
    let mut line = |s: String| {
        o.push_str(&s);
        o.push_str("\r\n");
    };
    line(format!("admin: {}", crate::elevation::is_admin()));
    let info = smbios::read();
    line(format!("board: {} {} (BIOS {})", info.board_vendor, info.board_name, info.bios));
    let mut cpu = Cpu::open();
    line(format!("cpu: {}  temp error: {:?}", cpu.brand, cpu.error));
    let _ = cpu.load();
    std::thread::sleep(Duration::from_millis(500));
    line(format!("cpu temp {:?}  clock {:?}  load {:?}", cpu.temp(), cpu.clock(), cpu.load()));
    let gpu = Gpu::open();
    line(format!("gpu '{}' temp {:?} load {:?}", gpu.name, gpu.temp(), gpu.load()));
    match sio::open(&info) {
        Ok(mut s) => {
            line(format!("fan chip: {}  control {:?}", s.name(), s.control()));
            line(format!("  {}", s.report()));
            if let Some(r) = s.read() {
                for (i, n) in s.fan_names().iter().enumerate() {
                    line(format!("  {} = {:?}%  {} rpm", n, r.pct[i], r.rpm[i]));
                }
                for (i, n) in s.temp_names().iter().enumerate() {
                    line(format!("  {} = {:?} C", n, r.temps[i]));
                }
            }
        }
        Err(e) => line(format!("fan chip: {e}")),
    }
    for (m, p) in &info.dimms {
        line(format!("dimm: {m} {p}"));
    }
    let l = cpupower::limits();
    line(format!("cpu limits: base {} turbo {} min {} ({}), oc possible {}", l.base, l.turbo, l.min, l.source, l.oc_possible));
    if let Some(r) = rapl::Rapl::open() {
        line(format!("rapl: {:?}", r.read().map(|x| (x.pl1, x.pl2))));
    }
    o
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_case_fans_may_stop() {
        for n in ["System Fan #1", "System Fan #6", "SYS_FAN2", "Chassis Fan 1"] {
            assert!(super::fan_may_stop(n), "{n}");
        }
        for n in ["CPU Fan", "Pump Fan", "AIO Pump", "Fan 3", "CPU_OPT", "System Pump"] {
            assert!(!super::fan_may_stop(n), "{n}");
        }
    }
}
