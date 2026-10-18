//! CPU temperature (Intel MSR / AMD SMN through PawnIO), CPU load (GetSystemTimes) and GPU temp/load (NVIDIA NVML, AMD ADL).
use crate::hw::pawn::*;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
use windows_sys::Win32::System::Threading::{GetCurrentThread, GetSystemTimes, SetThreadAffinityMask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Vendor {
    Intel,
    Amd,
    Other,
}

/// CPU vendor, family and brand string from CPUID.
pub fn cpu_id() -> (Vendor, u32, String) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        let v = __cpuid(0);
        let mut vend = [0u8; 12];
        vend[..4].copy_from_slice(&v.ebx.to_le_bytes());
        vend[4..8].copy_from_slice(&v.edx.to_le_bytes());
        vend[8..].copy_from_slice(&v.ecx.to_le_bytes());
        let vendor = match &vend {
            b"GenuineIntel" => Vendor::Intel,
            b"AuthenticAMD" | b"HygonGenuine" => Vendor::Amd,
            _ => Vendor::Other,
        };
        let f = __cpuid(1).eax;
        let mut family = (f >> 8) & 0xF;
        if family == 0xF {
            family += (f >> 20) & 0xFF;
        }
        let mut brand = vec![];
        if __cpuid(0x8000_0000).eax >= 0x8000_0004 {
            for leaf in 0x8000_0002u32..=0x8000_0004 {
                let r = __cpuid(leaf);
                for x in [r.eax, r.ebx, r.ecx, r.edx] {
                    brand.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
        let brand = String::from_utf8_lossy(&brand).trim_matches(char::from(0)).trim().to_string();
        (vendor, family, brand)
    }
    #[cfg(not(target_arch = "x86_64"))]
    (Vendor::Other, 0, String::new())
}

enum CpuTemp {
    Intel { m: Module, tjmax: f32 },
    Amd(Module),
    None,
}

pub struct Cpu {
    t: CpuTemp,
    prev: Option<(u64, u64, u64)>,
    pub brand: String,
    pub error: Option<String>,
    cores: Vec<(String, u64)>,
    energy_unit: f64,
    last_energy: Option<(u64, u64, std::time::Instant)>, // package, cores (raw counters)
    threads: crate::hw::sysmon::ThreadLoad,
    /// Windows "% Processor Performance" counter (PDH query, counter): the clock when the MSR driver is not available
    clk: (usize, usize),
    /// this tick's live clock of every logical processor (`clock` reads it, `details` reuses it)
    mhz: Vec<Option<f32>>,
    /// each logical processor's (MPERF, APERF) at the last tick; empty when the driver can't read them
    cycles: Vec<Option<(u64, u64)>>,
    cycles_ok: bool,
}

/// A core's live clock: the fastest of its threads (`mask` = its logical processors).
pub fn core_mhz(mask: u64, threads: &[Option<f32>]) -> Option<f32> {
    (0..64usize).filter(|b| mask & (1u64 << b) != 0).filter_map(|b| threads.get(b).copied().flatten()).reduce(f32::max)
}
/// Live clock of each logical processor in MHz, in processor order (totals left out; nothing
/// until Windows has a performance reading): the base clock times its "% Processor Performance"
/// (above 100 while boosting). That percentage is measured against the base clock on every core,
/// also on E-cores (an E-core at 114 % on an i5-14600KF runs at 4000 MHz). The base is fixed
/// (`base_mhz`): Windows' "Processor Frequency" counter is the current nominal clock, which a
/// power plan moves (2784 in a game, E-cores 2600), and every clock came out 20 % low against it.
pub fn thread_mhz(base: f64, perf: &[(String, f64)]) -> Vec<Option<f32>> {
    // instances are "group,number"; "_Total" and "group,_Total" are sums
    let key = |n: &str| {
        let (g, i) = n.split_once(',')?;
        Some((g.parse::<u32>().ok()?, i.parse::<u32>().ok()?))
    };
    let mut cpus: Vec<((u32, u32), f64)> = perf.iter().filter_map(|(n, v)| Some((key(n)?, *v))).collect();
    cpus.sort_by_key(|c| c.0);
    cpus.iter().map(|(_, p)| (base > 0.0).then(|| (base * p / 100.0).round() as f32)).collect()
}

/// A thread's clock between two reads of its cycle counters (MPERF, APERF): APERF counts the core's
/// own cycles and MPERF base-clock (TSC) cycles, both only while it runs, so this is the clock it
/// ran at, the figure MSI Afterburner and HWiNFO show. None when it ran under a millisecond.
pub fn counted_mhz(tsc_mhz: f64, before: (u64, u64), now: (u64, u64)) -> Option<f32> {
    let dm = now.0.wrapping_sub(before.0);
    let da = now.1.wrapping_sub(before.1);
    (tsc_mhz > 0.0 && dm as f64 >= tsc_mhz * 1000.0 && da > 0).then(|| (tsc_mhz * da as f64 / dm as f64).round() as f32)
}

/// Each thread's counted clock, else Windows' estimate of it.
pub fn merge_clocks(counted: &[Option<f32>], windows: &[Option<f32>]) -> Vec<Option<f32>> {
    (0..counted.len().max(windows.len())).map(|i| counted.get(i).copied().flatten().or_else(|| windows.get(i).copied().flatten())).collect()
}

/// The rate MPERF counts at (the TSC's, the base clock times the real bus clock), measured once;
/// the base clock where it can't be.
pub fn tsc_mhz() -> f64 {
    static TSC: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *TSC.get_or_init(|| {
        #[cfg(target_arch = "x86_64")]
        #[allow(unused_unsafe)]
        {
            use std::arch::x86_64::_rdtsc;
            let t0 = std::time::Instant::now();
            let c0 = unsafe { _rdtsc() };
            std::thread::sleep(std::time::Duration::from_millis(50));
            let c1 = unsafe { _rdtsc() };
            let m = c1.wrapping_sub(c0) as f64 / t0.elapsed().as_secs_f64() / 1e6;
            if m.is_finite() && m > 400.0 && m < 10_000.0 {
                return m;
            }
        }
        base_mhz()
    })
}

/// The CPU's base clock in MHz: the highest MaxMhz Windows gives any logical processor (the same
/// on P- and E-cores; it never moves), 0 when Windows doesn't say.
pub fn base_mhz() -> f64 {
    static BASE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *BASE.get_or_init(|| {
        use windows_sys::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION};
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let mut buf: Vec<PROCESSOR_POWER_INFORMATION> = vec![unsafe { std::mem::zeroed() }; n];
        let st = unsafe { CallNtPowerInformation(ProcessorInformation, std::ptr::null(), 0, buf.as_mut_ptr() as *mut _, (n * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32) };
        if st != 0 {
            return 0.0;
        }
        buf.iter().map(|p| p.MaxMhz as f64).fold(0.0, f64::max)
    })
}

/// Live clock of every logical processor (no admin rights needed), see `thread_mhz`.
pub struct ThreadClocks {
    perf: (usize, usize),
}

impl ThreadClocks {
    pub fn open() -> ThreadClocks {
        ThreadClocks { perf: pdh_open(r"\Processor Information(*)\% Processor Performance") }
    }

    /// Since the previous read (the first one gives nothing: performance is measured between two reads).
    pub fn read(&self) -> Vec<Option<f32>> {
        thread_mhz(base_mhz(), &pdh_values(self.perf))
    }
}

impl Drop for Cpu {
    fn drop(&mut self) {
        if self.clk.0 != 0 {
            unsafe { windows_sys::Win32::System::Performance::PdhCloseQuery(self.clk.0 as _) };
        }
    }
}

impl Drop for ThreadClocks {
    fn drop(&mut self) {
        if self.perf.0 != 0 {
            unsafe { windows_sys::Win32::System::Performance::PdhCloseQuery(self.perf.0 as _) };
        }
    }
}

/// Opens a wildcard PDH counter; (0, 0) when it does not exist.
fn pdh_open(path: &str) -> (usize, usize) {
    use windows_sys::Win32::System::Performance::*;
    unsafe {
        let mut q: PDH_HQUERY = std::ptr::null_mut();
        if PdhOpenQueryW(std::ptr::null(), 0, &mut q) != 0 {
            return (0, 0);
        }
        let w: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut c: PDH_HCOUNTER = std::ptr::null_mut();
        if PdhAddEnglishCounterW(q, w.as_ptr(), 0, &mut c) != 0 || PdhCollectQueryData(q) != 0 {
            PdhCloseQuery(q);
            return (0, 0);
        }
        (q as usize, c as usize)
    }
}

/// Every instance of a wildcard PDH counter as (instance name, value). Not capped at 100: `% Processor
/// Performance` goes over 100 while the CPU boosts above its base clock.
fn pdh_values(h: (usize, usize)) -> Vec<(String, f64)> {
    /// pdh.h's PDH_FMT_NOCAP100 (windows-sys doesn't list it)
    const PDH_FMT_NOCAP100: u32 = 0x8000;
    use windows_sys::Win32::System::Performance::*;
    let mut out = vec![];
    if h.0 == 0 {
        return out;
    }
    unsafe {
        if PdhCollectQueryData(h.0 as PDH_HQUERY) != 0 {
            return out;
        }
        let (mut size, mut count) = (0u32, 0u32);
        if PdhGetFormattedCounterArrayW(h.1 as PDH_HCOUNTER, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100, &mut size, &mut count, std::ptr::null_mut()) != PDH_MORE_DATA || size == 0 {
            return out;
        }
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(h.1 as PDH_HCOUNTER, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100, &mut size, &mut count, items) != 0 {
            return out;
        }
        for i in 0..count as usize {
            let it = &*items.add(i);
            let mut n = 0;
            while *it.szName.add(n) != 0 {
                n += 1;
            }
            let nm = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, n));
            if it.FmtValue.CStatus == 0 {
                out.push((nm, it.FmtValue.Anonymous.doubleValue));
            }
        }
    }
    out
}

/// Runs `f` on logical processor `cpu` (MSRs are per core), then restores the thread affinity.
fn on_cpu<R>(cpu: u32, f: impl FnOnce() -> R) -> R {
    unsafe {
        let th = GetCurrentThread();
        let old = SetThreadAffinityMask(th, 1usize << cpu);
        let r = f();
        if old != 0 {
            SetThreadAffinityMask(th, old);
        }
        r
    }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

impl Cpu {
    pub fn open() -> Cpu {
        let (vendor, family, brand) = cpu_id();
        let (t, error) = match vendor {
            Vendor::Intel => match Module::load(INTEL_MSR) {
                Ok(m) => {
                    let tj = m.exec("ioctl_read_msr", &[0x1A2], 1).ok().map(|v| ((v[0] as u64 >> 16) & 0xFF) as f32).filter(|t| *t > 0.0).unwrap_or(100.0);
                    (CpuTemp::Intel { m, tjmax: tj }, None)
                }
                Err(e) => (CpuTemp::None, Some(e)),
            },
            Vendor::Amd if family >= 0x17 => match Module::load(AMD_FAMILY17) {
                Ok(m) => (CpuTemp::Amd(m), None),
                Err(e) => (CpuTemp::None, Some(e)),
            },
            _ => (CpuTemp::None, Some(format!("CPU temperature not supported for this CPU ({})", brand))),
        };
        let mut c = Cpu { t, prev: None, brand, error, cores: crate::hw::sysmon::cores(), energy_unit: 0.0, last_energy: None, threads: crate::hw::sysmon::ThreadLoad::new(), clk: pdh_open(r"\Processor Information(*)\% Processor Performance"), mhz: vec![], cycles: vec![], cycles_ok: true };
        // RAPL energy unit (Intel 0x606, AMD 0xC0010299): joules = count / 2^bits[12:8]
        let unit_msr = match &c.t {
            CpuTemp::Intel { m, .. } => m.exec("ioctl_read_msr", &[0x606], 1).ok(),
            CpuTemp::Amd(m) => m.exec("ioctl_read_msr", &[0xC001_0299], 1).ok(),
            CpuTemp::None => None,
        };
        if let Some(v) = unit_msr {
            c.energy_unit = 1.0 / (1u64 << ((v[0] as u64 >> 8) & 0x1F)) as f64;
        }
        c
    }

    fn msr(&self, index: u32) -> Option<u64> {
        let m = match &self.t {
            CpuTemp::Intel { m, .. } => m,
            CpuTemp::Amd(m) => m,
            CpuTemp::None => return None,
        };
        m.exec("ioctl_read_msr", &[index as i64], 1).ok().map(|v| v[0] as u64)
    }

    /// Everything HWiNFO shows for the CPU that we can read: per-core temperature / clock / load,
    /// voltage (VID), package and core power. `temp` and `load` are this second's package values.
    pub fn details(&mut self, temp: Option<f32>, load: Option<f32>) -> Vec<crate::hw::sysmon::Sample> {
        use crate::hw::sysmon::s;
        let g = format!("CPU: {}", if self.brand.is_empty() { "processor" } else { &self.brand });
        let mut out = vec![s(&g, "Package temperature", "Â°C", 1, temp.map(|v| v as f64))];
        let thread_loads = self.threads.sample();
        let mut temps = vec![];
        let mut clocks = vec![];
        let mut vid_max: Option<f64> = None;
        let intel = matches!(self.t, CpuTemp::Intel { .. });
        let tjmax = if let CpuTemp::Intel { tjmax, .. } = &self.t { *tjmax as f64 } else { 0.0 };
        let cores = self.cores.clone();
        for (label, mask) in &cores {
            let cpu = mask.trailing_zeros();
            if intel {
                let (therm, perf) = on_cpu(cpu, || (self.msr(0x19C), self.msr(0x198)));
                let t = therm.filter(|v| v & 0x8000_0000 != 0).map(|v| tjmax - ((v >> 16) & 0x7F) as f64);
                let vid = perf.map(|v| ((v >> 32) & 0xFFFF) as f64 / 8192.0).filter(|v| *v > 0.2 && *v < 2.0);
                temps.push((label.clone(), t));
                if let Some(v) = vid {
                    vid_max = Some(vid_max.map_or(v, |m: f64| m.max(v)));
                }
            }
            // not the ratio in IA32_PERF_STATUS: it stays at the top turbo step while the core naps
            if !self.mhz.is_empty() {
                clocks.push((label.clone(), core_mhz(*mask, &self.mhz).map(|c| c as f64)));
            }
        }
        for (l, t) in &temps {
            out.push(s(&g, &format!("{} temperature", l), "Â°C", 0, *t));
        }
        if let Some(m) = temps.iter().filter_map(|t| t.1).reduce(f64::max) {
            out.push(s(&g, "Core max temperature", "Â°C", 0, Some(m)));
        }
        if vid_max.is_some() {
            out.push(s(&g, "Core voltage (VID)", "V", 3, vid_max));
        }
        // power from the RAPL energy counters
        let (pkg_msr, core_msr) = if intel { (0x611, Some(0x639)) } else { (0xC001_029B, None) };
        if self.energy_unit > 0.0 {
            if let Some(pkg) = self.msr(pkg_msr) {
                let core = core_msr.and_then(|m| self.msr(m)).unwrap_or(0);
                let now = std::time::Instant::now();
                if let Some((pp, pc, at)) = self.last_energy {
                    let dt = now.duration_since(at).as_secs_f64().max(0.001);
                    let d = |a: u64, b: u64| ((a & 0xFFFF_FFFF).wrapping_sub(b & 0xFFFF_FFFF) & 0xFFFF_FFFF) as f64 * self.energy_unit / dt;
                    out.push(s(&g, "Package power", "W", 1, Some(d(pkg, pp)).filter(|w| *w < 1000.0)));
                    if core_msr.is_some() {
                        out.push(s(&g, "Cores power", "W", 1, Some(d(core, pc)).filter(|w| *w < 1000.0)));
                    }
                }
                self.last_energy = Some((pkg, core, now));
            }
        }
        for (l, c) in &clocks {
            out.push(s(&g, &format!("{} clock", l), "MHz", 0, *c));
        }
        out.push(s(&g, "Total load", "%", 1, load.map(|v| v as f64)));
        if !thread_loads.is_empty() {
            for (label, mask) in &cores {
                let ls: Vec<f64> = (0..64).filter(|b| mask & (1u64 << b) != 0).filter_map(|b| thread_loads.get(b as usize).copied()).collect();
                if !ls.is_empty() {
                    out.push(s(&g, &format!("{} load", label), "%", 1, Some(ls.iter().sum::<f64>() / ls.len() as f64)));
                }
            }
        }
        out
    }

    /// CPU package temperature in C.
    pub fn temp(&self) -> Option<f32> {
        match &self.t {
            CpuTemp::Intel { m, tjmax } => {
                let eax = m.exec("ioctl_read_msr", &[0x1B1], 1).ok()?[0] as u32;
                (eax & 0x8000_0000 != 0).then(|| tjmax - ((eax >> 16) & 0x7F) as f32)
            }
            CpuTemp::Amd(m) => {
                // Zen: THM_TCON_CUR_TMP (SMN 0x59800), bits 31:21 in 0.125 C; some parts report with a -49 C offset
                let v = m.exec("ioctl_read_smn", &[0x0005_9800], 1).ok()?[0] as u32;
                let offset = v & 0x80000 != 0 || v & 0x30000 == 0x30000;
                let t = (v >> 21) as f32 * 0.125 - if offset { 49.0 } else { 0.0 };
                (t > 0.0 && t < 125.0).then_some(t)
            }
            CpuTemp::None => None,
        }
    }

    /// Fastest live thread clock in MHz: each thread's cycle counters (`counted_mhz`), Windows'
    /// estimate (`thread_mhz`, a few % low) where they have nothing. Also keeps every thread's
    /// clock for `details`.
    pub fn clock(&mut self) -> Option<u32> {
        let counted = self.counted_clocks();
        self.mhz = merge_clocks(&counted, &thread_mhz(base_mhz(), &pdh_values(self.clk)));
        let best = self.mhz.iter().flatten().fold(0.0f32, |a, b| a.max(*b));
        (best > 1.0).then(|| best.round() as u32)
    }

    /// Every logical processor's clock since the last tick from its MPERF / APERF (Intel 0xE7 / 0xE8,
    /// AMD's read-only copies 0xC00000E7 / 0xC00000E8); empty without the driver.
    fn counted_clocks(&mut self) -> Vec<Option<f32>> {
        let (mperf, aperf) = match self.t {
            CpuTemp::Intel { .. } => (0xE7, 0xE8),
            CpuTemp::Amd(_) => (0xC000_00E7, 0xC000_00E8),
            CpuTemp::None => return vec![],
        };
        if !self.cycles_ok {
            return vec![];
        }
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(64);
        let now: Vec<Option<(u64, u64)>> = (0..n).map(|i| on_cpu(i as u32, || Some((self.msr(mperf)?, self.msr(aperf)?)))).collect();
        if now.iter().all(|c| c.is_none()) {
            self.cycles_ok = false; // the driver won't read them: Windows' clocks from now on
            return vec![];
        }
        let tsc = tsc_mhz();
        let out = now.iter().enumerate().map(|(i, c)| counted_mhz(tsc, self.cycles.get(i).copied().flatten()?, (*c)?)).collect();
        self.cycles = now;
        out
    }

    /// Total CPU load in % since the previous call.
    pub fn load(&mut self) -> Option<f32> {
        let z = || FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut i, mut k, mut u) = (z(), z(), z());
        if unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } == 0 {
            return None;
        }
        let cur = (ft(i), ft(k), ft(u));
        let r = self.prev.and_then(|p| {
            let idle = cur.0.wrapping_sub(p.0);
            let total = cur.1.wrapping_sub(p.1) + cur.2.wrapping_sub(p.2); // kernel time already includes idle
            (total != 0).then(|| (100.0 * (1.0 - idle as f64 / total as f64)).clamp(0.0, 100.0) as f32)
        });
        self.prev = Some(cur);
        r
    }
}

// ---------------------------------------------------------------- GPU
#[repr(C)]
#[derive(Clone, Copy)]
struct AdlSensor {
    supported: i32,
    value: i32,
}
#[repr(C)]
struct AdlPmLog {
    size: i32,
    sensors: [AdlSensor; 256],
}
const ADL_TEMP_EDGE: usize = 8;
const ADL_ACTIVITY_GFX: usize = 19;

unsafe extern "C" fn adl_alloc(size: i32) -> *mut c_void {
    // ADL keeps what it allocates for its own lifetime; it is only used for small adapter tables
    Box::leak(vec![0u8; size.max(1) as usize].into_boxed_slice()).as_mut_ptr() as *mut c_void
}

enum GpuApi {
    /// NVIDIA: the one NVML instance Deck shares (crate::gpu::nvml)
    Nvml(&'static crate::gpu::nvml::Nvml),
    Adl { ctx: *mut c_void, index: i32, query: unsafe extern "C" fn(*mut c_void, i32, *mut AdlPmLog) -> i32 },
    /// integrated / other graphics: load from the Windows "GPU Engine" counters (no temperature of its own)
    Pdh { query: *mut c_void, counter: *mut c_void },
    None,
}

pub struct Gpu {
    api: GpuApi,
    pub name: String,
}
unsafe impl Send for Gpu {}

/// A GPU vendor library from System32 (or NVIDIA's NVSMI folder) only, see util::trusted_dll_paths.
fn load_lib(name: &str) -> *mut c_void {
    crate::util::load_trusted_dll(name)
}

impl Gpu {
    pub fn open() -> Gpu {
        unsafe { Self::nvml().or_else(|| Self::adl()).or_else(|| Self::pdh()).unwrap_or(Gpu { api: GpuApi::None, name: String::new() }) }
    }

    /// Load of the busiest graphics adapter from the Windows performance counters (works for Intel graphics).
    unsafe fn pdh() -> Option<Gpu> {
        use windows_sys::Win32::System::Performance::*;
        let mut query: PDH_HQUERY = std::ptr::null_mut();
        if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
            return None;
        }
        let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage".encode_utf16().chain(Some(0)).collect();
        let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
        if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 || PdhCollectQueryData(query) != 0 {
            PdhCloseQuery(query);
            return None;
        }
        // the adapter's name, from the display devices
        let mut name = String::new();
        let mut dd: windows_sys::Win32::Graphics::Gdi::DISPLAY_DEVICEW = std::mem::zeroed();
        dd.cb = std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::DISPLAY_DEVICEW>() as u32;
        if windows_sys::Win32::Graphics::Gdi::EnumDisplayDevicesW(std::ptr::null(), 0, &mut dd, 0) != 0 {
            let n = dd.DeviceString.iter().position(|c| *c == 0).unwrap_or(dd.DeviceString.len());
            name = String::from_utf16_lossy(&dd.DeviceString[..n]);
        }
        Some(Gpu { api: GpuApi::Pdh { query, counter }, name })
    }

    fn pdh_load(&self) -> Option<f32> {
        use windows_sys::Win32::System::Performance::*;
        let GpuApi::Pdh { query, counter } = &self.api else { return None };
        unsafe {
            if PdhCollectQueryData(*query) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(*counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut()) != PDH_MORE_DATA || size == 0 {
                return None;
            }
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(*counter, PDH_FMT_DOUBLE, &mut size, &mut count, items) != 0 {
                return None;
            }
            let mut total = 0.0f64;
            for i in 0..count as usize {
                let it = &*items.add(i);
                let mut n = 0;
                while *it.szName.add(n) != 0 {
                    n += 1;
                }
                let nm = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, n));
                if nm.contains("engtype_3D") && it.FmtValue.CStatus == 0 {
                    total += it.FmtValue.Anonymous.doubleValue;
                }
            }
            Some(total.clamp(0.0, 100.0) as f32)
        }
    }

    unsafe fn nvml() -> Option<Gpu> {
        let n = crate::gpu::nvml::Nvml::shared()?;
        Some(Gpu { api: GpuApi::Nvml(n), name: n.name.clone() })
    }

    unsafe fn adl() -> Option<Gpu> {
        let lib = load_lib("atiadlxx.dll");
        if lib.is_null() {
            return None;
        }
        let sym = |n: &[u8]| GetProcAddress(lib as _, n.as_ptr());
        let create: unsafe extern "C" fn(unsafe extern "C" fn(i32) -> *mut c_void, i32, *mut *mut c_void) -> i32 = std::mem::transmute(sym(b"ADL2_Main_Control_Create\0")?);
        let count: unsafe extern "C" fn(*mut c_void, *mut i32) -> i32 = std::mem::transmute(sym(b"ADL2_Adapter_NumberOfAdapters_Get\0")?);
        let query: unsafe extern "C" fn(*mut c_void, i32, *mut AdlPmLog) -> i32 = std::mem::transmute(sym(b"ADL2_New_QueryPMLogData_Get\0")?);
        let mut ctx = std::ptr::null_mut();
        if create(adl_alloc, 1, &mut ctx) != 0 {
            return None;
        }
        let mut n = 0;
        count(ctx, &mut n);
        // the first adapter index that reports an edge temperature is the GPU
        for index in 0..n.clamp(0, 64) {
            let mut log: AdlPmLog = std::mem::zeroed();
            log.size = std::mem::size_of::<AdlPmLog>() as i32;
            if query(ctx, index, &mut log) == 0 && log.sensors[ADL_TEMP_EDGE].supported != 0 {
                return Some(Gpu { api: GpuApi::Adl { ctx, index, query }, name: "AMD Radeon".into() });
            }
        }
        None
    }

    fn adl_log(&self) -> Option<AdlPmLog> {
        let GpuApi::Adl { ctx, index, query } = &self.api else { return None };
        unsafe {
            let mut log: AdlPmLog = std::mem::zeroed();
            log.size = std::mem::size_of::<AdlPmLog>() as i32;
            (query(*ctx, *index, &mut log) == 0).then_some(log)
        }
    }

    pub fn temp(&self) -> Option<f32> {
        match &self.api {
            GpuApi::Nvml(n) => n.temp().map(|t| t as f32),
            GpuApi::Adl { .. } => self.adl_log().map(|l| l.sensors[ADL_TEMP_EDGE]).filter(|s| s.supported != 0).map(|s| s.value as f32),
            GpuApi::Pdh { .. } | GpuApi::None => None,
        }
    }

    /// Everything HWiNFO shows for the GPU that the vendor library gives us.
    pub fn details(&self, temp: Option<f32>, load: Option<f32>) -> Vec<crate::hw::sysmon::Sample> {
        use crate::hw::sysmon::s;
        let g = format!("GPU: {}", if self.name.is_empty() { "graphics" } else { &self.name });
        let mut out = vec![s(&g, "Temperature", "Â°C", 0, temp.map(|v| v as f64)), s(&g, "Core load", "%", 0, load.map(|v| v as f64))];
        match &self.api {
            GpuApi::Nvml(n) => {
                if let Some((_, mem)) = n.util_rates() {
                    out.push(s(&g, "Memory controller load", "%", 0, Some(mem as f64)));
                }
                if let Some((used, total)) = n.memory() {
                    out.push(s(&g, "Memory used", "MB", 0, Some(used as f64 / 1048576.0)));
                    out.push(s(&g, "Memory total", "MB", 0, Some(total as f64 / 1048576.0)));
                }
                for (name, kind) in [("Core clock", crate::gpu::nvml::CLOCK_GRAPHICS), ("Memory clock", crate::gpu::nvml::CLOCK_MEM)] {
                    out.push(s(&g, name, "MHz", 0, n.clock(kind).map(|v| v as f64)));
                }
                out.push(s(&g, "Power", "W", 1, n.power_mw().map(|v| v as f64 / 1000.0)));
                out.push(s(&g, "Fan", "%", 0, n.fan_speed(0).ok().map(|v| v as f64)));
            }
            GpuApi::Adl { .. } => {
                if let Some(l) = self.adl_log() {
                    let hot = l.sensors[27];
                    if hot.supported != 0 {
                        out.push(s(&g, "Hot spot temperature", "Â°C", 0, Some(hot.value as f64)));
                    }
                }
            }
            GpuApi::Pdh { .. } | GpuApi::None => {}
        }
        out
    }

    pub fn load(&self) -> Option<f32> {
        match &self.api {
            GpuApi::Nvml(n) => n.util_rates().map(|u| u.0 as f32),
            GpuApi::Adl { .. } => self.adl_log().map(|l| l.sensors[ADL_ACTIVITY_GFX]).filter(|s| s.supported != 0).map(|s| s.value as f32),
            GpuApi::Pdh { .. } => self.pdh_load(),
            GpuApi::None => None,
        }
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    fn named(v: &[(&str, f64)]) -> Vec<(String, f64)> {
        v.iter().map(|(n, x)| (n.to_string(), *x)).collect()
    }

    #[test]
    fn every_thread_counts_against_the_base_clock() {
        // "% Processor Performance" is measured against the base clock (3500 on an i5-14600KF) on
        // every core: an E-core at 114 % runs at 4000 MHz (its own register said 40x), not 2964
        let perf = named(&[("0,2", 114.3), ("0,0", 91.0), ("0,1", 150.0), ("_Total", 100.0), ("0,_Total", 100.0)]);
        assert_eq!(thread_mhz(3500.0, &perf), vec![Some(3185.0), Some(5250.0), Some(4001.0)], "in logical-processor order, totals left out");
    }

    /// Windows' "Processor Frequency" is the current nominal clock, not the base: in a game under a
    /// power plan it read 2784 (E-cores 2600) while the base stayed 3500, and every clock came out
    /// 20 % low (2150 MHz where HWiNFO showed 27 x 99.8 = 2693).
    #[test]
    fn the_base_is_fixed_whatever_windows_calls_the_current_clock() {
        let perf = named(&[("0,0", 76.9), ("0,12", 79.8)]);
        assert_eq!(thread_mhz(3500.0, &perf), vec![Some(2692.0), Some(2793.0)]);
        assert!(thread_mhz(0.0, &perf).iter().all(|m| m.is_none()), "no base known: no clock rather than a made-up one");
    }

    /// A user's i5-14600KF in a game: MSI Afterburner showed 5292 on every P-core (53 x 99.85) and
    /// 3994 on every E-core, Windows' "% Processor Performance" gave 4836-5227 and 3225-3878. The
    /// core's own counters (APERF: its cycles, MPERF: base-clock cycles, both only while it runs)
    /// give Afterburner's figure.
    #[test]
    fn the_clock_comes_from_the_cores_own_cycle_counters() {
        let tsc = 3494.75; // 35 x 99.85
        // a second at 53x, half of it asleep: both counters stop while it sleeps
        let half = 1_747_375_000u64;
        assert_eq!(counted_mhz(tsc, (1_000, 9_000), (1_000 + half, 9_000 + half * 53 / 35)), Some(5292.0));
        // an E-core at 40x
        assert_eq!(counted_mhz(tsc, (0, 0), (half, half * 40 / 35)), Some(3994.0));
        // the counters wrap around
        assert_eq!(counted_mhz(tsc, (u64::MAX - 10, u64::MAX - 10), (half - 11, half - 11)), Some(3495.0));
    }

    /// A thread that barely ran gives no reading (not the 800 MHz Afterburner shows for a sleeping
    /// core), and nothing is made up without a base clock.
    #[test]
    fn a_thread_that_slept_has_no_counted_clock() {
        assert_eq!(counted_mhz(3494.75, (0, 0), (3_000, 6_000)), None, "under a millisecond of running");
        assert_eq!(counted_mhz(3494.75, (5, 5), (5, 5)), None);
        assert_eq!(counted_mhz(0.0, (0, 0), (1 << 32, 1 << 33)), None);
    }

    #[test]
    fn windows_fills_in_where_the_counters_have_nothing() {
        let counted = [Some(5292.0), None, Some(3994.0)];
        let windows = [Some(5227.0), Some(4907.0), Some(3824.0), Some(3785.0)];
        assert_eq!(merge_clocks(&counted, &windows), vec![Some(5292.0), Some(4907.0), Some(3994.0), Some(3785.0)]);
        assert_eq!(merge_clocks(&[], &windows), windows.to_vec(), "no driver: Windows' clocks");
        assert_eq!(merge_clocks(&counted, &[]), counted.to_vec());
    }

    #[test]
    fn processor_groups_come_in_order() {
        let perf = named(&[("1,0", 50.0), ("0,1", 0.0), ("0,0", 100.0)]);
        assert_eq!(thread_mhz(3000.0, &perf), vec![Some(3000.0), Some(0.0), Some(1500.0)]);
        assert!(thread_mhz(3000.0, &[]).is_empty());
    }

    /// A core runs at the clock of its busiest thread; the ratio register it used to come from
    /// stays at the top turbo step while the core naps, so every core read 5300 / 4000.
    #[test]
    fn a_core_shows_the_live_clock_of_its_threads() {
        let threads = [Some(5210.0), Some(5250.0), Some(2840.0), None, Some(1200.0)];
        assert_eq!(core_mhz(0b11, &threads), Some(5250.0), "a P-core with two threads");
        assert_eq!(core_mhz(0b100, &threads), Some(2840.0), "an E-core");
        assert_eq!(core_mhz(0b1000, &threads), None, "no reading yet");
        assert_eq!(core_mhz(1 << 40, &threads), None, "a thread this PC doesn't have");
    }

    /// Reads this PC's live thread clocks (read-only).
    #[test]
    #[ignore]
    fn live_thread_clocks() {
        let c = ThreadClocks::open();
        c.read();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let v = c.read();
        println!("{v:?}");
        assert_eq!(v.len(), std::thread::available_parallelism().unwrap().get());
        assert!(v.iter().all(|m| m.is_some_and(|m| m > 100.0)));
    }
}
