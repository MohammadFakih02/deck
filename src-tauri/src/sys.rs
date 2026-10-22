//! PC vitals (CPU, RAM, GPU, disks, network) and the process list.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use sysinfo::{Disks, Pid, ProcessesToUpdate, System};

struct Inner {
    sys: System,
    /// per-adapter network counters: the rates are what moved since the last reading
    net: crate::util::NetCounter,
    disks: Disks,
    disks_at: Instant,
    net_at: Instant,
    cpu_hist: VecDeque<f32>,
    ram_hist: VecDeque<f32>,
    down_hist: VecDeque<f64>,
    up_hist: VecDeque<f64>,
    gpu_hist: VecDeque<f32>,
    vram_hist: VecDeque<f32>,
    procs_at: Option<Instant>,
}

pub struct Sys {
    inner: Mutex<Inner>,
}

const HIST: usize = 60;

fn push<T>(q: &mut VecDeque<T>, v: T) {
    if q.len() >= HIST {
        q.pop_front();
    }
    q.push_back(v);
}

/// Processes that are part of Windows itself; never offered for killing.
const PROTECTED: &[&str] = &[
    "system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe",
    "svchost.exe", "dwm.exe", "fontdrvhost.exe", "memory compression", "system idle process", "idle", "secure system",
    "lsaiso.exe", "sihost.exe", "ctfmon.exe", "audiodg.exe", "spoolsv.exe", "searchindexer.exe", "msmpeng.exe",
    "securityhealthservice.exe", "wudfhost.exe", "conhost.exe", "runtimebroker.exe", "startmenuexperiencehost.exe",
    "shellexperiencehost.exe", "textinputhost.exe", "searchhost.exe", "explorer.exe", "deck.exe",
];

/// The file that holds the code of one of the kernel's own processes (System, Registry, Memory
/// Compression and the idle process run inside ntoskrnl.exe; Secure System is securekernel.exe),
/// when it is there. These processes have no program file of their own.
fn kernel_file(name: &str) -> Option<String> {
    let file = match name.to_ascii_lowercase().as_str() {
        "system" | "registry" | "memory compression" | "system idle process" | "[system process]" => "ntoskrnl.exe",
        "secure system" => "securekernel.exe",
        _ => return None,
    };
    let p = crate::util::windows_dir().join("System32").join(file);
    p.is_file().then(|| p.display().to_string())
}

pub fn is_protected(name: &str) -> bool {
    PROTECTED.contains(&name.to_ascii_lowercase().as_str())
}

/// Whether Windows marks the process critical (ending it is a blue screen).
/// Deck runs as administrator, so the name list above isn't enough on its own.
/// None: Windows wouldn't say, which is treated as "don't end it".
pub fn is_critical(pid: u32) -> Option<bool> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{IsProcessCritical, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut c = windows::core::BOOL(0);
        let r = IsProcessCritical(h, &mut c);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(c.as_bool())
    }
}

/// Safe for Deck to end: not on the Windows list and not marked critical.
fn may_end(name: &str, pid: u32) -> bool {
    !is_protected(name) && is_critical(pid) == Some(false)
}

impl Sys {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Self {
            inner: Mutex::new(Inner {
                sys,
                net: crate::util::NetCounter::new(),
                disks: Disks::new_with_refreshed_list(),
                disks_at: Instant::now(),
                net_at: Instant::now(),
                cpu_hist: VecDeque::new(),
                ram_hist: VecDeque::new(),
                down_hist: VecDeque::new(),
                up_hist: VecDeque::new(),
                gpu_hist: VecDeque::new(),
                vram_hist: VecDeque::new(),
                procs_at: None,
            }),
        }
    }

    /// Current vitals. Call about once a second: CPU and network rates are
    /// measured between calls.
    pub fn snapshot(&self) -> Value {
        let mut g = self.inner.lock();
        let i = &mut *g;
        i.sys.refresh_cpu_usage();
        i.sys.refresh_memory();
        let cpu = i.sys.global_cpu_usage();
        let total = i.sys.total_memory();
        let used = i.sys.used_memory();
        let ram_pct = if total > 0 { used as f32 * 100.0 / total as f32 } else { 0.0 };

        let dt = i.net_at.elapsed().as_secs_f64().max(0.2);
        i.net_at = Instant::now();
        let (rx, tx) = i.net.sample();
        let down = rx as f64 / dt;
        let up = tx as f64 / dt;

        if i.disks_at.elapsed().as_secs() >= 10 {
            i.disks.refresh(true);
            i.disks_at = Instant::now();
        }
        let disks: Vec<Value> = i
            .disks
            .iter()
            .map(|d| {
                json!({
                    "mount": d.mount_point().to_string_lossy().trim_end_matches('\\'),
                    "name": d.name().to_string_lossy(),
                    "total": d.total_space(),
                    "free": d.available_space(),
                    "removable": d.is_removable(),
                })
            })
            .collect();

        let gpu = crate::gpu::nvml::Nvml::shared().map(|n| {
            let mem = n.memory();
            json!({
                "name": n.name.replace("NVIDIA GeForce ", ""),
                "temp": n.temp(),
                "util": n.util_rates().map(|u| u.0),
                "vram_used": mem.map(|m| m.0),
                "vram_total": mem.map(|m| m.1),
                "power_w": n.power_mw().map(|mw| mw / 1000),
                "clock": n.clock(crate::gpu::nvml::CLOCK_GRAPHICS),
                "fan": n.fan_speed(0).ok(),
            })
        });

        push(&mut i.cpu_hist, cpu);
        push(&mut i.ram_hist, ram_pct);
        push(&mut i.down_hist, down);
        push(&mut i.up_hist, up);
        if let Some(u) = gpu.as_ref().and_then(|g| g["util"].as_f64()) {
            push(&mut i.gpu_hist, u as f32);
        }
        if let Some((u, t)) = gpu.as_ref().and_then(|g| Some((g["vram_used"].as_f64()?, g["vram_total"].as_f64()?))) {
            push(&mut i.vram_hist, if t > 0.0 { (u * 100.0 / t) as f32 } else { 0.0 });
        }
        json!({
            "cpu": cpu,
            "cores": i.sys.cpus().len(),
            "ram_used": used,
            "ram_total": total,
            "ram_pct": ram_pct,
            "gpu": gpu,
            "disks": disks,
            "down": down,
            "up": up,
            "uptime": System::uptime(),
            "cpu_hist": i.cpu_hist,
            "ram_hist": i.ram_hist,
            "down_hist": i.down_hist,
            "up_hist": i.up_hist,
            "gpu_hist": i.gpu_hist,
            "vram_hist": i.vram_hist,
        })
    }

    /// Running programs grouped by executable name, biggest first.
    pub fn processes(&self, windowed: &HashMap<u32, (String, bool)>) -> Vec<Value> {
        let mut g = self.inner.lock();
        // CPU% needs two samples; take a quick second one on the first call.
        let first = g.procs_at.is_none();
        g.sys.refresh_processes(ProcessesToUpdate::All, true);
        if first {
            drop(g);
            std::thread::sleep(std::time::Duration::from_millis(250));
            g = self.inner.lock();
            g.sys.refresh_processes(ProcessesToUpdate::All, true);
        }
        g.procs_at = Some(Instant::now());
        let cores = g.sys.cpus().len().max(1) as f32;
        #[derive(Default)]
        struct Group {
            name: String,
            exe: Option<String>,
            pids: Vec<u32>,
            cpu: f32,
            mem: u64,
            title: Option<String>,
            hung: bool,
            /// a kernel process: `exe` is the kernel file its code lives in
            kernel: bool,
            /// each process of the program: (pid, cpu %, memory, its window's title)
            kids: Vec<(u32, f32, u64, Option<String>)>,
        }
        let mut groups: HashMap<String, Group> = HashMap::new();
        for (pid, p) in g.sys.processes() {
            if p.thread_kind().is_some() {
                continue;
            }
            let name = p.name().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            let key = name.to_ascii_lowercase();
            let e = groups.entry(key).or_default();
            e.name = name;
            if e.exe.is_none() {
                // protected processes refuse the library's richer access; the limited-information
                // right Task Manager uses still gives their file. The "System" process is the kernel's file.
                e.exe = p
                    .exe()
                    .map(|x| x.to_string_lossy().to_string())
                    // some protected processes give an empty path rather than none
                    .filter(|x| !x.is_empty())
                    .or_else(|| crate::util::process_path(pid.as_u32()))
                    .or_else(|| crate::util::process_path_by_id(pid.as_u32()));
            }
            e.pids.push(pid.as_u32());
            e.cpu += p.cpu_usage() / cores;
            e.mem += p.memory();
            e.kids.push((pid.as_u32(), p.cpu_usage() / cores, p.memory(), windowed.get(&pid.as_u32()).map(|(t, _)| t.clone())));
            if let Some((t, hung)) = windowed.get(&pid.as_u32()) {
                if e.title.is_none() {
                    e.title = Some(t.clone());
                }
                e.hung |= *hung;
            }
        }
        let mut out: Vec<Group> = groups.into_values().collect();
        // the kernel's own processes have no file of theirs; their code is in a kernel file, which is
        // what gets scanned for them (only where no path was found)
        for g in out.iter_mut().filter(|g| g.exe.is_none()) {
            if let Some(f) = kernel_file(&g.name) {
                g.exe = Some(f);
                g.kernel = true;
            }
        }
        // programs with no file to scan, said in the log when the set changes (so "why can't it
        // check these" can be answered from what the PC really said)
        static NO_FILE: Mutex<Vec<String>> = parking_lot::const_mutex(Vec::new());
        let mut none: Vec<String> = out.iter().filter(|g| g.exe.is_none()).map(|g| format!("{} (pid {})", g.name, g.pids.first().copied().unwrap_or(0))).collect();
        none.sort();
        let mut last = NO_FILE.lock();
        if *last != none {
            crate::log::info(format!("processes: {} programs have no file to scan: {}", none.len(), none.join(", ")));
            *last = none;
        }
        drop(last);
        out.sort_by_key(|b| std::cmp::Reverse(b.mem));
        out.into_iter()
            .map(|mut g| {
                g.kids.sort_by_key(|k| std::cmp::Reverse(k.2));
                let procs: Vec<Value> = g.kids.iter().map(|(pid, cpu, mem, title)| json!({ "pid": pid, "cpu": cpu, "mem": mem, "title": title })).collect();
                json!({
                    "name": g.name,
                    "exe": g.exe,
                    "pids": g.pids,
                    "procs": procs,
                    "count": g.pids.len(),
                    "cpu": g.cpu,
                    "mem": g.mem,
                    "title": g.title,
                    "app": g.title.is_some(),
                    "hung": g.hung,
                    "kernel": g.kernel,
                    "protected": is_protected(&g.name) || g.pids.iter().any(|p| is_critical(*p) == Some(true)),
                })
            })
            .collect()
    }

    /// Ends every process with this executable name. Returns how many ended.
    pub fn kill_name(&self, name: &str) -> Result<usize, String> {
        if is_protected(name) {
            return Err(format!("{name} is part of Windows"));
        }
        let mut g = self.inner.lock();
        g.sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut n = 0;
        let mut failed = 0;
        let mut critical = 0;
        for p in g.sys.processes().values() {
            if p.name().to_string_lossy().eq_ignore_ascii_case(name) {
                if !may_end(name, p.pid().as_u32()) {
                    critical += 1;
                    continue;
                }
                if p.kill() {
                    n += 1;
                } else {
                    failed += 1;
                }
            }
        }
        if n == 0 && critical > 0 {
            Err(format!("{name} is part of Windows"))
        } else if n == 0 && failed > 0 {
            Err(format!("Windows didn't let Deck close {name} (it may run as administrator)"))
        } else {
            Ok(n)
        }
    }

    pub fn kill_pid(&self, pid: u32) -> bool {
        let mut g = self.inner.lock();
        let p = Pid::from_u32(pid);
        g.sys.refresh_processes(ProcessesToUpdate::Some(&[p]), true);
        g.sys.process(p).map(|p| may_end(&p.name().to_string_lossy(), pid) && p.kill()).unwrap_or(false)
    }

    /// Is any process with this executable name running?
    pub fn running(&self, name: &str) -> bool {
        let mut g = self.inner.lock();
        g.sys.refresh_processes(ProcessesToUpdate::All, true);
        g.sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case(name))
    }

    /// CPU use now. Fresh even while Deck is hidden (nothing else samples then), so the
    /// "when the PC goes idle" plan never runs on a reading from when the panel was last open.
    pub fn cpu_now(&self) -> f32 {
        let mut g = self.inner.lock();
        Self::sample_if_stale(&mut g);
        g.cpu_hist.back().copied().unwrap_or(0.0)
    }

    /// Download rate now (bytes/s), fresh in the same way.
    pub fn down_now(&self) -> f64 {
        let mut g = self.inner.lock();
        Self::sample_if_stale(&mut g);
        g.down_hist.back().copied().unwrap_or(0.0)
    }

    /// A light sample (CPU and network only) when `snapshot` hasn't run for 2 s.
    fn sample_if_stale(i: &mut Inner) {
        if i.net_at.elapsed().as_secs_f64() < 2.0 {
            return;
        }
        i.sys.refresh_cpu_usage();
        let cpu = i.sys.global_cpu_usage();
        let dt = i.net_at.elapsed().as_secs_f64();
        i.net_at = Instant::now();
        let (rx, tx) = i.net.sample();
        push(&mut i.cpu_hist, cpu);
        push(&mut i.down_hist, rx as f64 / dt);
        push(&mut i.up_hist, tx as f64 / dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_path_can_be_asked_of_the_system_by_id() {
        let me = std::env::current_exe().unwrap().display().to_string();
        let asked = crate::util::process_path_by_id(std::process::id()).expect("own path");
        assert!(asked.eq_ignore_ascii_case(me.trim_start_matches(r"\\?\")), "{asked} vs {me}");
        assert_eq!(crate::util::process_path_by_id(0), None);
        assert_eq!(crate::util::process_path_by_id(u32::MAX - 3), None);
    }

    /// Which running programs still have no file Deck can scan on this PC (prints them).
    #[test]
    #[ignore = "reads this PC's processes"]
    fn programs_without_a_file_here() {
        let list = Sys::new().processes(&HashMap::new());
        let none: Vec<&str> = list.iter().filter(|p| p["exe"].as_str().is_none_or(str::is_empty)).map(|p| p["name"].as_str().unwrap_or("?")).collect();
        println!("{} programs, {} without a file: {none:?}", list.len(), none.len());
        // the kernel's own processes are scanned through the kernel file they run from
        assert!(none.is_empty(), "everything should have a file: {none:?}");
        let kernel: Vec<_> = list.iter().filter(|p| p["kernel"] == true).map(|p| (p["name"].as_str().unwrap_or("?"), p["exe"].as_str().unwrap_or(""))).collect();
        println!("kernel: {kernel:?}");
        // rows that share one file count once in "Check all N programs"
        let mut by_file: HashMap<String, Vec<&str>> = HashMap::new();
        for p in &list {
            by_file.entry(p["exe"].as_str().unwrap_or("").to_lowercase()).or_default().push(p["name"].as_str().unwrap_or("?"));
        }
        let shared: Vec<_> = by_file.iter().filter(|(_, v)| v.len() > 1).collect();
        println!("{} rows, {} files; rows sharing a file: {shared:?}", list.len(), by_file.len());
        assert!(kernel.iter().all(|(_, f)| f.to_lowercase().ends_with("ntoskrnl.exe") || f.to_lowercase().ends_with("securekernel.exe")));
    }

    #[test]
    fn readings_stay_fresh_while_deck_is_hidden() {
        let s = Sys::new();
        let _ = s.snapshot(); // the panel was open once
        let n = s.inner.lock().down_hist.len();
        let _ = s.down_now();
        assert_eq!(s.inner.lock().down_hist.len(), n, "a reading under 2 s old is reused");
        std::thread::sleep(std::time::Duration::from_millis(2100));
        let _ = s.cpu_now();
        let _ = s.down_now();
        let g = s.inner.lock();
        assert_eq!(g.down_hist.len(), n + 1, "the panel is closed: a fresh sample was taken (once)");
        assert!(g.net_at.elapsed().as_secs_f64() < 1.0);
    }

    #[test]
    fn processes_windows_needs_are_never_ended() {
        assert_eq!(is_critical(std::process::id()), Some(false));
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        // csrss.exe is marked critical by Windows: ending it is a blue screen
        let csrss = sys.processes().values().find(|p| p.name().eq_ignore_ascii_case("csrss.exe")).map(|p| p.pid().as_u32());
        if let Some(pid) = csrss {
            assert_ne!(is_critical(pid), Some(false), "csrss must never look safe to end");
        }
    }
}

#[cfg(test)]
mod net_live {
    /// Live: Deck's reading over 3 s next to Windows' own adapter totals (the real adapters only).
    #[test]
    #[ignore]
    fn net_matches_windows() {
        let mut c = crate::util::NetCounter::new();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let (rx, tx) = c.sample();
        println!("NET deck: {:.2} MB/s down, {:.3} MB/s up", rx as f64 / 3e6, tx as f64 / 3e6);
    }
}
