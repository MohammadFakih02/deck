//! NVIDIA's official management library (nvml.dll, shipped with the driver), loaded at run
//! time. Reads work for everyone; writes (power limit, clock offsets, fans, temperature
//! limit) need admin. Every call returns the raw NVML code so the caller can tell
//! "not supported" from "no permission".

use std::ffi::c_void;
use windows::core::PCSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

pub type Dev = *mut c_void;
pub const SUCCESS: i32 = 0;
pub const ERR_NOT_SUPPORTED: i32 = 3;
pub const ERR_NO_PERMISSION: i32 = 4;

/// NVML_CLOCK_GRAPHICS / NVML_CLOCK_MEM
pub const CLOCK_GRAPHICS: u32 = 0;
pub const CLOCK_MEM: u32 = 2;

pub fn err_text(code: i32) -> &'static str {
    match code {
        0 => "ok",
        1 => "NVML not initialised",
        2 => "invalid value",
        3 => "not supported by this card or driver",
        4 => "needs admin rights",
        6 => "not found",
        8 => "GPU is lost",
        15 => "GPU in use / busy",
        _ => "driver error",
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct ClockOffset {
    pub version: u32,
    pub clock_type: u32,
    pub pstate: u32,
    pub offset_mhz: i32,
    pub min_mhz: i32,
    pub max_mhz: i32,
}

impl ClockOffset {
    fn new(clock_type: u32) -> Self {
        // NVML_STRUCT_VERSION(ClockOffset, 1) = sizeof | (1 << 24)
        Self { version: std::mem::size_of::<Self>() as u32 | (1 << 24), clock_type, pstate: 0, ..Default::default() }
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Util {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct MemInfo {
    total: u64,
    free: u64,
    used: u64,
}

macro_rules! fns {
    ($($name:ident : fn($($a:ty),*) ;)*) => {
        #[allow(non_snake_case)]
        struct Fns { $($name: Option<unsafe extern "C" fn($($a),*) -> i32>,)* }
        impl Fns {
            unsafe fn load(m: HMODULE) -> Self {
                Self { $($name: GetProcAddress(m, PCSTR(concat!(stringify!($name), "\0").as_ptr())).map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "C" fn($($a),*) -> i32>(f)),)* }
            }
        }
    };
}

fns! {
    nvmlInit_v2: fn();
    nvmlDeviceGetCount_v2: fn(*mut u32);
    nvmlDeviceGetHandleByIndex_v2: fn(u32, *mut Dev);
    nvmlDeviceGetName: fn(Dev, *mut u8, u32);
    nvmlSystemGetDriverVersion: fn(*mut u8, u32);
    nvmlDeviceGetPowerManagementLimit: fn(Dev, *mut u32);
    nvmlDeviceGetPowerManagementLimitConstraints: fn(Dev, *mut u32, *mut u32);
    nvmlDeviceGetPowerManagementDefaultLimit: fn(Dev, *mut u32);
    nvmlDeviceSetPowerManagementLimit: fn(Dev, u32);
    nvmlDeviceGetPowerUsage: fn(Dev, *mut u32);
    nvmlDeviceGetClockOffsets: fn(Dev, *mut ClockOffset);
    nvmlDeviceSetClockOffsets: fn(Dev, *mut ClockOffset);
    nvmlDeviceGetGpcClkVfOffset: fn(Dev, *mut i32);
    nvmlDeviceSetGpcClkVfOffset: fn(Dev, i32);
    nvmlDeviceGetMemClkVfOffset: fn(Dev, *mut i32);
    nvmlDeviceSetMemClkVfOffset: fn(Dev, i32);
    nvmlDeviceGetGpcClkMinMaxVfOffset: fn(Dev, *mut i32, *mut i32);
    nvmlDeviceGetMemClkMinMaxVfOffset: fn(Dev, *mut i32, *mut i32);
    nvmlDeviceGetNumFans: fn(Dev, *mut u32);
    nvmlDeviceGetFanSpeed_v2: fn(Dev, u32, *mut u32);
    nvmlDeviceGetTargetFanSpeed: fn(Dev, u32, *mut u32);
    nvmlDeviceSetFanSpeed_v2: fn(Dev, u32, u32);
    nvmlDeviceSetDefaultFanSpeed_v2: fn(Dev, u32);
    nvmlDeviceGetMinMaxFanSpeed: fn(Dev, *mut u32, *mut u32);
    nvmlDeviceGetTemperature: fn(Dev, u32, *mut u32);
    nvmlDeviceGetTemperatureThreshold: fn(Dev, u32, *mut u32);
    nvmlDeviceSetTemperatureThreshold: fn(Dev, u32, *mut i32);
    nvmlDeviceGetClockInfo: fn(Dev, u32, *mut u32);
    nvmlDeviceGetMaxClockInfo: fn(Dev, u32, *mut u32);
    nvmlDeviceGetUtilizationRates: fn(Dev, *mut Util);
    nvmlDeviceGetMemoryInfo: fn(Dev, *mut MemInfo);
    nvmlDeviceGetCurrentClocksEventReasons: fn(Dev, *mut u64);
    nvmlDeviceGetCurrentClocksThrottleReasons: fn(Dev, *mut u64);
    nvmlDeviceGetPciInfo_v3: fn(Dev, *mut u8);
    nvmlDeviceSetGpuLockedClocks: fn(Dev, u32, u32);
    nvmlDeviceResetGpuLockedClocks: fn(Dev);
}

pub struct Nvml {
    f: Fns,
    pub dev: Dev,
    pub name: String,
    pub driver: String,
}

unsafe impl Send for Nvml {}
unsafe impl Sync for Nvml {}

/// NVML_TEMPERATURE_THRESHOLD_ACOUSTIC_{MIN,CURR,MAX}: the "temperature limit" slider.
pub const TEMP_ACOUSTIC_MIN: u32 = 4;
pub const TEMP_ACOUSTIC_CURR: u32 = 5;
pub const TEMP_ACOUSTIC_MAX: u32 = 6;
/// NVML_TEMPERATURE_THRESHOLD_SLOWDOWN
pub const TEMP_SLOWDOWN: u32 = 1;

#[derive(Default, Clone, Debug, serde::Serialize)]
pub struct Live {
    pub core_mhz: Option<u32>,
    pub mem_mhz: Option<u32>,
    pub power_w: Option<f32>,
    pub temp_c: Option<u32>,
    pub util: Option<u32>,
    pub mem_used_mb: Option<u64>,
    pub mem_total_mb: Option<u64>,
    pub fan_pct: Vec<u32>,
    /// What holds the clock back right now: "power", "thermal", "voltage", "idle" or "".
    pub limit: String,
}

impl Nvml {
    pub fn load() -> Option<Nvml> {
        unsafe {
            let m = LoadLibraryW(windows::core::w!("nvml.dll")).ok()?;
            let f = Fns::load(m);
            if (f.nvmlInit_v2?)() != SUCCESS {
                return None;
            }
            let mut n = 0u32;
            (f.nvmlDeviceGetCount_v2?)(&mut n);
            if n == 0 {
                return None;
            }
            let mut dev: Dev = std::ptr::null_mut();
            if (f.nvmlDeviceGetHandleByIndex_v2?)(0, &mut dev) != SUCCESS {
                return None;
            }
            let mut buf = [0u8; 96];
            (f.nvmlDeviceGetName?)(dev, buf.as_mut_ptr(), buf.len() as u32);
            let name = cstr(&buf);
            let mut d = [0u8; 80];
            if let Some(g) = f.nvmlSystemGetDriverVersion {
                g(d.as_mut_ptr(), d.len() as u32);
            }
            Some(Nvml { f, dev, name, driver: cstr(&d) })
        }
    }

    // ---- power limit (milliwatts)

    /// (current, min, max, default) in mW.
    pub fn power_limit(&self) -> Result<(u32, u32, u32, u32), i32> {
        unsafe {
            let (mut c, mut lo, mut hi, mut d) = (0, 0, 0, 0);
            call(self.f.nvmlDeviceGetPowerManagementLimit.map(|g| g(self.dev, &mut c)))?;
            call(self.f.nvmlDeviceGetPowerManagementLimitConstraints.map(|g| g(self.dev, &mut lo, &mut hi)))?;
            call(self.f.nvmlDeviceGetPowerManagementDefaultLimit.map(|g| g(self.dev, &mut d)))?;
            Ok((c, lo, hi, d))
        }
    }
    pub fn set_power_limit(&self, mw: u32) -> Result<(), i32> {
        unsafe { call(self.f.nvmlDeviceSetPowerManagementLimit.map(|g| g(self.dev, mw))) }
    }

    // ---- clock offsets (MHz). New API first (R555+), the older per-domain calls as fallback.

    /// (offset, min, max) for CLOCK_GRAPHICS or CLOCK_MEM.
    pub fn offset(&self, clock: u32) -> Result<(i32, i32, i32), i32> {
        unsafe {
            if let Some(g) = self.f.nvmlDeviceGetClockOffsets {
                let mut o = ClockOffset::new(clock);
                if g(self.dev, &mut o) == SUCCESS {
                    return Ok((o.offset_mhz, o.min_mhz, o.max_mhz));
                }
            }
            let (mut v, mut lo, mut hi) = (0i32, 0i32, 0i32);
            if clock == CLOCK_GRAPHICS {
                call(self.f.nvmlDeviceGetGpcClkVfOffset.map(|g| g(self.dev, &mut v)))?;
                call(self.f.nvmlDeviceGetGpcClkMinMaxVfOffset.map(|g| g(self.dev, &mut lo, &mut hi)))?;
            } else {
                call(self.f.nvmlDeviceGetMemClkVfOffset.map(|g| g(self.dev, &mut v)))?;
                call(self.f.nvmlDeviceGetMemClkMinMaxVfOffset.map(|g| g(self.dev, &mut lo, &mut hi)))?;
            }
            Ok((v, lo, hi))
        }
    }
    pub fn set_offset(&self, clock: u32, mhz: i32) -> Result<(), i32> {
        unsafe {
            if let Some(s) = self.f.nvmlDeviceSetClockOffsets {
                let mut o = ClockOffset::new(clock);
                o.offset_mhz = mhz;
                let r = s(self.dev, &mut o);
                if r == SUCCESS || r == ERR_NO_PERMISSION {
                    return if r == SUCCESS { Ok(()) } else { Err(r) };
                }
            }
            if clock == CLOCK_GRAPHICS {
                call(self.f.nvmlDeviceSetGpcClkVfOffset.map(|g| g(self.dev, mhz)))
            } else {
                call(self.f.nvmlDeviceSetMemClkVfOffset.map(|g| g(self.dev, mhz)))
            }
        }
    }

    // ---- fans (percent)

    pub fn fans(&self) -> u32 {
        let mut n = 0;
        unsafe {
            let _ = call(self.f.nvmlDeviceGetNumFans.map(|g| g(self.dev, &mut n)));
        }
        n
    }
    pub fn fan_speed(&self, i: u32) -> Result<u32, i32> {
        let mut v = 0;
        unsafe { call(self.f.nvmlDeviceGetFanSpeed_v2.map(|g| g(self.dev, i, &mut v))).map(|_| v) }
    }
    pub fn fan_range(&self) -> Result<(u32, u32), i32> {
        let (mut lo, mut hi) = (0, 0);
        unsafe { call(self.f.nvmlDeviceGetMinMaxFanSpeed.map(|g| g(self.dev, &mut lo, &mut hi))).map(|_| (lo, hi)) }
    }
    pub fn set_fan(&self, i: u32, pct: u32) -> Result<(), i32> {
        unsafe { call(self.f.nvmlDeviceSetFanSpeed_v2.map(|g| g(self.dev, i, pct))) }
    }
    /// Gives the fan back to the driver's own curve.
    pub fn fan_auto(&self, i: u32) -> Result<(), i32> {
        unsafe { call(self.f.nvmlDeviceSetDefaultFanSpeed_v2.map(|g| g(self.dev, i))) }
    }

    // ---- temperature

    pub fn temp_threshold(&self, kind: u32) -> Result<u32, i32> {
        let mut v = 0;
        unsafe { call(self.f.nvmlDeviceGetTemperatureThreshold.map(|g| g(self.dev, kind, &mut v))).map(|_| v) }
    }
    pub fn set_temp_limit(&self, c: i32) -> Result<(), i32> {
        let mut v = c;
        unsafe { call(self.f.nvmlDeviceSetTemperatureThreshold.map(|g| g(self.dev, TEMP_ACOUSTIC_CURR, &mut v))) }
    }

    /// Keeps the core clock between `lo` and `hi` MHz (Afterburner's curve lock uses lo = hi).
    pub fn lock_clocks(&self, lo: u32, hi: u32) -> Result<(), i32> {
        unsafe { call(self.f.nvmlDeviceSetGpuLockedClocks.map(|g| g(self.dev, lo, hi))) }
    }
    pub fn unlock_clocks(&self) -> Result<(), i32> {
        unsafe { call(self.f.nvmlDeviceResetGpuLockedClocks.map(|g| g(self.dev))) }
    }

    pub fn max_clock(&self, clock: u32) -> Option<u32> {
        let mut v = 0;
        unsafe { call(self.f.nvmlDeviceGetMaxClockInfo.map(|g| g(self.dev, clock, &mut v))).ok().map(|_| v) }
    }

    /// PCI ids in Afterburner's form, e.g. "VEN_10DE&DEV_2484&SUBSYS_87C11043&REV_A1&BUS_1&DEV_0&FN_0".
    pub fn pci_id(&self) -> Option<String> {
        // nvmlPciInfo_t: busIdLegacy[16], domain, bus, device, pciDeviceId, pciSubSystemId, busId[32]
        let mut b = [0u8; 16 + 4 * 5 + 32];
        unsafe { call(self.f.nvmlDeviceGetPciInfo_v3.map(|g| g(self.dev, b.as_mut_ptr()))).ok()? };
        let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let (bus, device, dev_id, subsys) = (u(20), u(24), u(28), u(32));
        Some(format!("VEN_{:04X}&DEV_{:04X}&SUBSYS_{:08X}&BUS_{}&DEV_{}", dev_id & 0xFFFF, dev_id >> 16, subsys, bus, device))
    }

    pub fn live(&self) -> Live {
        let mut l = Live::default();
        unsafe {
            let mut v = 0u32;
            if call(self.f.nvmlDeviceGetClockInfo.map(|g| g(self.dev, CLOCK_GRAPHICS, &mut v))).is_ok() {
                l.core_mhz = Some(v);
            }
            if call(self.f.nvmlDeviceGetClockInfo.map(|g| g(self.dev, CLOCK_MEM, &mut v))).is_ok() {
                l.mem_mhz = Some(v);
            }
            if call(self.f.nvmlDeviceGetPowerUsage.map(|g| g(self.dev, &mut v))).is_ok() {
                l.power_w = Some(v as f32 / 1000.0);
            }
            if call(self.f.nvmlDeviceGetTemperature.map(|g| g(self.dev, 0, &mut v))).is_ok() {
                l.temp_c = Some(v);
            }
            let mut u = Util::default();
            if call(self.f.nvmlDeviceGetUtilizationRates.map(|g| g(self.dev, &mut u))).is_ok() {
                l.util = Some(u.gpu);
            }
            let mut m = MemInfo::default();
            if call(self.f.nvmlDeviceGetMemoryInfo.map(|g| g(self.dev, &mut m))).is_ok() {
                l.mem_used_mb = Some(m.used / 1_048_576);
                l.mem_total_mb = Some(m.total / 1_048_576);
            }
            for i in 0..self.fans() {
                if let Ok(p) = self.fan_speed(i) {
                    l.fan_pct.push(p);
                }
            }
            let mut r = 0u64;
            let ok = call(self.f.nvmlDeviceGetCurrentClocksEventReasons.map(|g| g(self.dev, &mut r))).is_ok()
                || call(self.f.nvmlDeviceGetCurrentClocksThrottleReasons.map(|g| g(self.dev, &mut r))).is_ok();
            if ok {
                // GpuIdle 0x1, SwPowerCap 0x4, HwSlowdown 0x8, SwThermal 0x20, HwThermal 0x40, HwPowerBrake 0x80
                l.limit = if r & 0x4 != 0 || r & 0x80 != 0 {
                    "power"
                } else if r & 0x60 != 0 || r & 0x8 != 0 {
                    "thermal"
                } else if r & 0x1 != 0 {
                    "idle"
                } else {
                    ""
                }
                .into();
            }
        }
        l
    }
}

unsafe fn call(r: Option<i32>) -> Result<(), i32> {
    match r {
        None => Err(ERR_NOT_SUPPORTED),
        Some(SUCCESS) => Ok(()),
        Some(e) => Err(e),
    }
}

fn cstr(b: &[u8]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).to_string()
}
