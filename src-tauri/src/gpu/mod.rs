//! GPU tuning and monitoring (NVIDIA): official NVML for power limit, fans and the temperature
//! limit; NvAPI's voltage/frequency table for clock offsets and curves (Afterburner's method).

pub mod curve;
pub mod nvapi;
pub mod nvml;

use std::fmt::Write as _;

/// Appends one formatted line (the probes also run in release builds, which have no console).
macro_rules! w {
    ($o:expr, $($t:tt)*) => {{ let _ = writeln!($o, $($t)*); }};
}

/// `--gpu-probe`: what this card and driver support. Reads only; with `write`, small reverted
/// test writes (power limit, a −15 MHz curve point, fan 40%) — never while a game is open.
pub fn probe(write: bool) -> String {
    let mut o = String::new();
    let Some(n) = nvml::Nvml::shared() else {
        w!(o, "NVML: not available");
        return o;
    };
    w!(o, "admin: {}", crate::elevation::is_admin());
    w!(o, "GPU {}  driver {}  pci {:?}", n.name, n.driver, n.pci_id());
    w!(o, "power limit (cur,min,max,default mW): {:?}", n.power_limit().map_err(nvml::err_text));
    w!(o, "nvml core offset (cur,min,max): {:?}", n.offset(nvml::CLOCK_GRAPHICS).map_err(nvml::err_text));
    w!(o, "nvml mem offset  (cur,min,max): {:?}", n.offset(nvml::CLOCK_MEM).map_err(nvml::err_text));
    w!(o, "fans: {}  range {:?}  speeds {:?}", n.fans(), n.fan_range(), (0..n.fans()).map(|i| n.fan_speed(i)).collect::<Vec<_>>());
    for (k, name) in [(nvml::TEMP_ACOUSTIC_MIN, "acoustic min"), (nvml::TEMP_ACOUSTIC_CURR, "acoustic cur"), (nvml::TEMP_ACOUSTIC_MAX, "acoustic max"), (nvml::TEMP_SLOWDOWN, "slowdown")] {
        w!(o, "temp {name}: {:?}", n.temp_threshold(k).map_err(nvml::err_text));
    }
    w!(o, "max clocks core {:?} mem {:?}", n.max_clock(nvml::CLOCK_GRAPHICS), n.max_clock(nvml::CLOCK_MEM));
    w!(o, "live {:?}", n.live());
    let a = nvapi::NvApi::load();
    match &a {
        None => w!(o, "NvAPI: not available"),
        Some(a) => {
            w!(o, "NvAPI ok. volt boost {:?}  voltage {:?} mV", a.volt_boost(), a.voltage_mv());
            match curve::read(a) {
                Ok(c) => {
                    w!(o, "curve: {} core points, {} memory points", c.core.len(), c.mem.len());
                    for p in c.core.iter().step_by(10) {
                        w!(o, "  {:7.2} mV  base {:6.0} MHz  delta {:+5.0}", p.mv, p.base_mhz, p.delta_mhz);
                    }
                    for p in &c.mem {
                        w!(o, "  mem point {}: base {:6.0} MHz delta {:+5.0} editable {}", p.index, p.base_mhz, p.delta_mhz, p.editable);
                    }
                }
                Err(e) => w!(o, "curve: {e}"),
            }
        }
    }
    if write {
        write_tests(&mut o, n, a.as_ref());
    }
    o
}

fn write_tests(o: &mut String, n: &nvml::Nvml, a: Option<&nvapi::NvApi>) {
    use std::thread::sleep;
    use std::time::Duration;
    let pause = || sleep(Duration::from_secs(3));
    if let Ok((cur, lo, hi, _)) = n.power_limit() {
        let t = (cur + 10_000).clamp(lo, hi);
        w!(o, "power limit -> {t} mW: {:?}", n.set_power_limit(t).map_err(nvml::err_text));
        pause();
        w!(o, "  read back {:?}", n.power_limit().map(|p| p.0));
        w!(o, "  restore {cur}: {:?}", n.set_power_limit(cur).map_err(nvml::err_text));
    }
    if let Some(a) = a {
        if let Ok(before) = curve::read(a) {
            let mut c = before.clone();
            if let Some(last) = c.core.last_mut() {
                last.delta_mhz -= 15.0;
            }
            w!(o, "curve: top point -15 MHz: {:?}", curve::write(a, &c));
            pause();
            let back = curve::read(a).map(|r| r.core.last().map(|p| p.delta_mhz));
            w!(o, "  read back top delta {:?} (was {:?})", back, before.core.last().map(|p| p.delta_mhz));
            w!(o, "  restore: {:?}", curve::write(a, &before));
        }
    }
    for i in 0..n.fans() {
        w!(o, "fan {i} -> 40%: {:?}", n.set_fan(i, 40).map_err(nvml::err_text));
    }
    pause();
    w!(o, "  fans now {:?}", (0..n.fans()).map(|i| n.fan_speed(i)).collect::<Vec<_>>());
    for i in 0..n.fans() {
        w!(o, "fan {i} -> auto: {:?}", n.fan_auto(i).map_err(nvml::err_text));
    }
}

/// Set while Deck holds a GPU fan at a manual speed; cleared when it hands it back.
pub static FAN_MANUAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Set once Deck is quitting: the GPU loop writes nothing more.
pub static STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Held while the fans are written, so the release on quit can't interleave with a write.
pub static FAN_GATE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
/// Set while a profile locks the core clock to one curve point.
pub static CLOCK_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Set when Windows wakes from sleep; the GPU loop re-applies the profile (a limit can be lost).
pub static RESUMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Counts wakes from sleep; the lighting threads compare it to re-init controllers that lost power.
pub static WAKES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Sets `RESUMED` on every wake from sleep or hibernate (no window needed).
pub fn watch_resume() {
    use windows_sys::Win32::System::Power::{PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC};
    unsafe extern "system" fn on_power(_ctx: *const core::ffi::c_void, kind: u32, _setting: *const core::ffi::c_void) -> u32 {
        if kind == PBT_APMRESUMEAUTOMATIC {
            RESUMED.store(true, std::sync::atomic::Ordering::SeqCst);
            WAKES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        0
    }
    // called from several threads; register only once
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut first = false;
    ONCE.call_once(|| first = true);
    if !first {
        return;
    }
    // registered for the life of the process: the parameters must stay alive, so they are leaked
    let params = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS { Callback: Some(on_power), Context: std::ptr::null_mut() }));
    let mut handle = std::ptr::null_mut();
    unsafe { PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_CALLBACK, params as *mut _ as _, &mut handle) };
}

/// On quit / crash / session end: gives the GPU fans back to the driver's own curve if Deck
/// was driving them, and lets go of a clock lock (a locked card would not idle down).
/// Clocks and limits stay (they reset at restart anyway).
pub fn release() {
    STOPPING.store(true, std::sync::atomic::Ordering::SeqCst);
    let _gate = FAN_GATE.lock(); // waits for a fan write in progress
    let fans = FAN_MANUAL.swap(false, std::sync::atomic::Ordering::SeqCst);
    let lock = CLOCK_LOCKED.swap(false, std::sync::atomic::Ordering::SeqCst);
    if fans || lock {
        if let Some(n) = nvml::Nvml::shared() {
            // the hardware first; the log after (it only queues a line)
            let mut failed = vec![];
            if fans {
                for i in 0..n.fans() {
                    if let Err(e) = n.fan_auto(i) {
                        failed.push(format!("fan {i}: {e}"));
                    }
                }
            }
            if lock {
                if let Err(e) = n.unlock_clocks() {
                    failed.push(format!("clock lock: {e}"));
                }
            }
            if failed.is_empty() {
                crate::log::info(format!("gpu: handed back to the driver ({}{})", if fans { "fans" } else { "" }, if lock { if fans { ", clock lock" } else { "clock lock" } } else { "" }));
            } else {
                crate::log::warn(format!("gpu: not everything went back to the driver: {}", failed.join(" · ")));
            }
        } else {
            crate::log::warn("gpu: NVML wasn't available to hand the fans or clock lock back");
        }
    }
}

pub mod engine;
pub mod guard;
pub use engine::Gpu;

/// MSI Afterburner is running (it applies its own profiles and would fight Deck's).
pub fn afterburner_running() -> bool {
    use std::time::{Duration, Instant};
    static CACHE: parking_lot::Mutex<Option<(Instant, bool)>> = parking_lot::Mutex::new(None);
    if let Some((at, v)) = *CACHE.lock() {
        if at.elapsed() < Duration::from_secs(15) {
            return v;
        }
    }
    let v = scan_afterburner();
    *CACHE.lock() = Some((Instant::now(), v));
    v
}

fn scan_afterburner() -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, false);
    sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case("MSIAfterburner.exe"))
}
