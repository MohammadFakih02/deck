//! CPU turbo off / max-clock presets, applied through the active Windows power plan (no MSR writes, fully reversible),
//! plus an optional, guarded turbo-ratio override for CPUs whose multiplier is unlocked.
use std::sync::OnceLock;


/// What this CPU can do, read from the CPU itself.
#[derive(Clone, Debug)]
pub struct Limits {
    /// guaranteed (base) clock in MHz
    pub base: i32,
    /// highest single-core turbo in MHz
    pub turbo: i32,
    /// lowest clock the slider offers
    pub min: i32,
    /// false when the turbo speed could not be read and `turbo` is a guess
    pub turbo_known: bool,
    /// the multiplier can be raised above turbo (unlocked CPU, MSR driver working)
    pub oc_possible: bool,
    /// the multiplier lock could be checked (the MSR driver loaded)
    pub oc_checked: bool,
    pub source: &'static str,
}

fn pin_cpu0<R>(f: impl FnOnce() -> R) -> R {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};
    unsafe {
        let th = GetCurrentThread();
        let old = SetThreadAffinityMask(th, 1);
        let r = f();
        if old != 0 {
            SetThreadAffinityMask(th, old);
        }
        r
    }
}

/// CPUID leaf 0x16: base and maximum frequency in MHz (Intel Skylake and newer, many AMD parts).
fn cpuid_freqs() -> Option<(i32, i32)> {
    pin_cpu0(|| {
        use std::arch::x86_64::__cpuid;
        if __cpuid(0).eax < 0x16 {
            return None;
        }
        let r = __cpuid(0x16);
        let (b, m) = ((r.eax & 0xFFFF) as i32, (r.ebx & 0xFFFF) as i32);
        (b >= 400).then_some((b, m.max(b)))
    })
}

/// Windows' own idea of the CPU: MaxMhz is the base clock.
fn windows_base() -> Option<i32> {
    use windows_sys::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION};
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut buf: Vec<PROCESSOR_POWER_INFORMATION> = vec![unsafe { std::mem::zeroed() }; n];
    let st = unsafe { CallNtPowerInformation(ProcessorInformation, std::ptr::null(), 0, buf.as_mut_ptr() as *mut _, (n * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32) };
    (st == 0 && buf[0].MaxMhz >= 400).then(|| buf[0].MaxMhz as i32)
}

/// Runs `f` once per logical CPU with the thread pinned to it (MSRs are per core).
fn each_cpu(mut f: impl FnMut()) {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(64);
    unsafe {
        let th = GetCurrentThread();
        let mut old = 0usize;
        for cpu in 0..n {
            let prev = SetThreadAffinityMask(th, 1usize << cpu);
            if cpu == 0 {
                old = prev;
            }
            f();
        }
        if old != 0 {
            SetThreadAffinityMask(th, old);
        }
    }
}

fn intel_module() -> Option<crate::hw::pawn::Module> {
    let (vendor, _, _) = crate::hw::sensors::cpu_id();
    if !matches!(vendor, crate::hw::sensors::Vendor::Intel) {
        return None;
    }
    crate::hw::pawn::Module::load(crate::hw::pawn::INTEL_MSR).ok()
}

fn read_msr(m: &crate::hw::pawn::Module, index: i64) -> Option<u64> {
    m.exec("ioctl_read_msr", &[index], 1).ok().map(|v| v[0] as u64)
}

/// Base / turbo / minimum clock of this CPU. Cached: it does not change while the program runs.
pub fn limits() -> &'static Limits {
    static L: OnceLock<Limits> = OnceLock::new();
    L.get_or_init(|| {
        let mut l = Limits { base: 0, turbo: 0, min: 800, turbo_known: false, oc_possible: false, oc_checked: false, source: "estimated" };
        if let Some((b, t)) = cpuid_freqs() {
            l.base = b;
            l.turbo = t;
            l.turbo_known = t > b;
            l.source = "CPUID";
        } else if let Some(b) = windows_base() {
            l.base = b;
            l.turbo = b;
            l.source = "Windows";
        }
        // the CPU's own registers are exact when the driver loads (Intel)
        if let Some(m) = intel_module() {
            if let Some(pi) = read_msr(&m, 0xCE) {
                let base = ((pi >> 8) & 0xFF) as i32 * 100;
                let minr = ((pi >> 40) & 0xFF) as i32 * 100;
                if base >= 400 {
                    l.base = base;
                }
                if minr >= 400 && minr < l.base {
                    l.min = minr;
                }
                l.oc_checked = true;
                l.oc_possible = pi & (1 << 28) != 0; // "programmable turbo ratio limits"
            }
            if let Some(tr) = read_msr(&m, 0x1AD) {
                let top = (tr & 0xFF) as i32 * 100;
                if top >= l.base && top <= 8000 {
                    l.turbo = top;
                    l.turbo_known = true;
                    l.source = "CPU registers";
                }
            }
        }
        if l.base == 0 {
            l.base = 2000;
            l.turbo = 4000;
        }
        if !l.turbo_known && l.turbo <= l.base {
            l.turbo = (l.base + 2400).min(5500); // unknown boost: a generous range; caps above the real turbo do no harm
        }
        l.min = l.min.min(l.base).max(400);
        l
    })
}


use crate::profiles::plans;
use windows::core::GUID;

/// The minimum-state values the lock changes, with the names their originals are kept under.
const MIN_KEYS: [(&str, GUID); 4] = [
    ("PROCTHROTTLEMIN", plans::STATE_MIN[0]),
    ("PROCTHROTTLEMIN1", plans::STATE_MIN[1]),
    ("PROCTHROTTLEMIN2", plans::STATE_MIN[2]),
    ("IDLEDISABLE", plans::IDLE_DISABLE),
];

/// Minimum clock: asks Windows to keep every core class at or above `min` MHz (`min` equal to the maximum = held at full
/// speed). `ref_max` is the clock that counts as 100 %.
/// The first time, the plan's original values are remembered in `s` and put back when `min` is None.
/// Windows only passes this on as a request: the CPU's firmware still has the last word.
pub fn apply_min(min: Option<i32>, ref_max: i32, s: &crate::hw::kv::Settings) {
    // the lock lives in one plan: remember which, so it is undone there even after a plan switch
    let target = match min {
        Some(_) if s.int("CpuMinSet", 0) != 1 => plans::active(),
        _ => plans::parse(&s.get("CpuMinPlan", "")).or_else(plans::active),
    };
    let Some(g) = target else { return };
    match min {
        Some(m) => {
            if s.int("CpuMinSet", 0) != 1 {
                for (name, key) in &MIN_KEYS {
                    let (ac, dc) = plans::read(&g, key).map(|(a, d)| (a as i32, d as i32)).unwrap_or((-1, -1));
                    s.set_int(&format!("CpuO_{name}_A"), ac);
                    s.set_int(&format!("CpuO_{name}_D"), dc);
                }
                s.set_int("CpuMinSet", 1);
                s.set("CpuMinPlan", &plans::guid_str(&g));
                s.save();
            }
            let pct = if m >= ref_max { 100 } else { ((m as f32 / ref_max.max(1) as f32) * 100.0).round() as i32 }.clamp(0, 100);
            for (_, key) in &MIN_KEYS[..3] {
                let _ = plans::write(&g, key, pct as u32);
            }
            let _ = plans::write(&g, &plans::IDLE_DISABLE, 0); // idle states stay on: off makes a laptop run flat out and hot even when idle
        }
        None => {
            if s.int("CpuMinSet", 0) != 1 {
                return;
            }
            for (name, key) in &MIN_KEYS {
                let (ac, dc) = (s.int(&format!("CpuO_{name}_A"), -1), s.int(&format!("CpuO_{name}_D"), -1));
                if ac >= 0 && dc >= 0 {
                    let _ = plans::write_pair(&g, key, ac as u32, dc as u32);
                }
            }
            s.set_int("CpuMinSet", 0);
            s.save();
        }
    }
    // Windows picks changed values of the active plan up when it is set active again
    if plans::active() == Some(g) {
        let _ = plans::set_active(&g);
    }
}

/// How far above the CPU's own top turbo the multiplier may be raised.
pub const OC_MAX_EXTRA: i32 = 300;

/// Turbo-ratio override (Intel unlocked CPUs only): raises the turbo ratio limit register above the stock top turbo.
/// `target` = Some(MHz) to override, None to put the original value back; `saved` is the original register value
/// remembered from the first override. Returns (value to remember, message). The write is read back, and undone when the
/// CPU or BIOS ignores it, so on a locked CPU this changes nothing.
pub fn apply_oc(target: Option<i32>, saved: Option<u64>) -> (Option<u64>, String) {
    let Some(m) = intel_module() else { return (saved, "Overclocking needs an Intel CPU and the MSR driver, which is not available here.".into()) };
    let Some(pi) = read_msr(&m, 0xCE) else { return (saved, "Could not read the CPU's platform info.".into()) };
    let cur = read_msr(&m, 0x1AD).unwrap_or(0);
    let write_all = |v: u64| {
        each_cpu(|| {
            let _ = m.exec("ioctl_write_msr", &[0x1AD, v as i64], 0);
        })
    };
    match target {
        None => match saved {
            Some(o) => {
                write_all(o);
                (None, "Turbo limit put back to stock.".into())
            }
            None => (None, String::new()),
        },
        Some(mhz) => {
            if pi & (1 << 28) == 0 {
                return (saved, "This CPU's multiplier is locked, so it cannot go above its turbo clock.".into());
            }
            let orig = saved.unwrap_or(cur);
            let stock = (orig & 0xFF) as i32;
            let ratio = ((mhz + 50) / 100).clamp(stock, stock + OC_MAX_EXTRA / 100);
            let v = shift_bins(orig, ratio - stock);
            write_all(v);
            let back = read_msr(&m, 0x1AD).unwrap_or(0);
            if (back & 0xFF) as i32 == ratio {
                (Some(orig), format!("Turbo limit raised to {} MHz.", ratio * 100))
            } else {
                write_all(orig);
                (None, "The BIOS keeps the multiplier locked; nothing was changed.".into())
            }
        }
    }
}

/// MSR 0x1AD holds one turbo ratio per active-core count (byte 0 = 1 core ... byte 7 = 8 cores).
/// Raising the top turbo by `up` bins raises each one by the same `up`, so the all-core clocks
/// keep their stock distance below it (empty bytes stay empty).
pub fn shift_bins(orig: u64, up: i32) -> u64 {
    let mut b = orig.to_le_bytes();
    for x in b.iter_mut() {
        if *x != 0 {
            *x = (*x as i32 + up).clamp(1, 255) as u8;
        }
    }
    u64::from_le_bytes(b)
}

/// The original turbo-limit register value remembered from the first override (settings hold 32-bit numbers).
pub fn saved_oc(s: &crate::hw::kv::Settings) -> Option<u64> {
    (s.int("CpuOcSet", 0) == 1).then(|| (s.int("CpuOcLo", 0) as u32 as u64) | ((s.int("CpuOcHi", 0) as u32 as u64) << 32))
}

pub fn store_oc(s: &crate::hw::kv::Settings, v: Option<u64>) {
    s.set_int("CpuOcSet", v.is_some() as i32);
    s.set_int("CpuOcLo", v.unwrap_or(0) as u32 as i32);
    s.set_int("CpuOcHi", (v.unwrap_or(0) >> 32) as u32 as i32);
}


#[cfg(test)]
mod tests {
    #[test]
    fn oc_keeps_the_per_core_steps() {
        // 53 x for 1-2 cores, 52 for 3-4, 51, 50, 49, 48
        let stock = u64::from_le_bytes([53, 53, 52, 52, 51, 50, 49, 48]);
        assert_eq!(super::shift_bins(stock, 1).to_le_bytes(), [54, 54, 53, 53, 52, 51, 50, 49]);
        assert_eq!(super::shift_bins(u64::from_le_bytes([50, 48, 0, 0, 0, 0, 0, 0]), 2).to_le_bytes(), [52, 50, 0, 0, 0, 0, 0, 0]);
    }
}
