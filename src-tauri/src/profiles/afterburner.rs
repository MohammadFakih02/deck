//! MSI Afterburner import: its five profile slots and [Startup] become GPU profiles, its
//! Ctrl+Alt hotkeys come along, and its "show in OSD" choices become Deck's overlay items.
//! Also "stop Afterburner from tuning" (backup, clear its startup profile and hotkeys) with undo.

use super::{GpuFan, GpuProfile};
use crate::gpu::curve;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn profiles_dir() -> Option<PathBuf> {
    let base = crate::util::env_path("ProgramFiles(x86)").unwrap_or_else(|| PathBuf::from("C:\\Program Files (x86)"));
    let d = base.join("MSI Afterburner").join("Profiles");
    d.is_dir().then_some(d)
}

/// `[section] key=value` (Afterburner's files are plain INI, CRLF).
pub type Ini = Vec<(String, Vec<(String, String)>)>;

pub fn parse_ini(text: &str) -> Ini {
    let mut out: Ini = vec![];
    for line in text.lines() {
        let l = line.trim();
        if let Some(name) = l.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            out.push((name.to_string(), vec![]));
        } else if let Some((k, v)) = l.split_once('=') {
            if let Some(sec) = out.last_mut() {
                sec.1.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    out
}

fn section<'a>(ini: &'a Ini, name: &str) -> Option<HashMap<&'a str, &'a str>> {
    ini.iter().find(|s| s.0.eq_ignore_ascii_case(name)).map(|s| s.1.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect())
}

/// This card's profile file: `VEN_10DE&DEV_2484&SUBSYS_87C11043&REV_A1&BUS_1&DEV_0&FN_0.cfg`
/// for a PCI id like `VEN_10DE&DEV_2484&SUBSYS_87C11043&BUS_1&DEV_0`.
pub fn gpu_cfg(dir: &Path, pci: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = pci.split('&').collect();
    let head = parts.iter().take(3).copied().collect::<Vec<_>>().join("&").to_uppercase();
    let tail = parts.iter().skip(3).copied().collect::<Vec<_>>().join("&").to_uppercase();
    let mut best = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_uppercase();
        if n.starts_with(&head) && n.ends_with(".CFG") {
            if tail.is_empty() || n.contains(&format!("&{tail}&")) {
                return Some(e.path());
            }
            best = Some(e.path());
        }
    }
    best
}

/// The card's factory temperature target, as Afterburner recorded it (`[Defaults] ThermalLimit`).
/// NVML only reports the current target, so this is how a limit Afterburner left behind is undone.
pub fn default_temp(pci: &str) -> Option<u32> {
    let text = std::fs::read_to_string(gpu_cfg(&profiles_dir()?, pci)?).ok()?;
    let ini = parse_ini(&text);
    section(&ini, "Defaults")?.get("ThermalLimit")?.trim().parse::<f64>().ok().map(|v| v.round() as u32).filter(|v| *v > 0)
}

/// `default_temp`, remembered in Deck's own folder so it outlives an Afterburner uninstall.
pub fn default_temp_saved(pci: &str) -> Option<u32> {
    remember_temp(&crate::settings::Settings::dir().join("gpu-defaults.json"), pci, default_temp(pci))
}

/// Saves `found` under `pci` in `file` (a `{pci: temp}` map) or, when None, returns the saved one.
fn remember_temp(file: &Path, pci: &str, found: Option<u32>) -> Option<u32> {
    let mut map: serde_json::Map<String, Value> = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    match found {
        Some(t) => {
            if map.get(pci).and_then(Value::as_u64) != Some(t as u64) {
                map.insert(pci.to_string(), json!(t));
                if let Some(d) = file.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(file, Value::Object(map).to_string());
            }
            Some(t)
        }
        None => map.get(pci).and_then(Value::as_u64).map(|t| t as u32),
    }
}

/// One Afterburner profile section as a GPU profile. None for an empty slot.
pub fn to_profile(sec: &HashMap<&str, &str>, name: &str) -> Option<GpuProfile> {
    let num = |k: &str| sec.get(k).and_then(|v| v.trim().parse::<f64>().ok());
    // an empty slot has no values at all
    num("PowerLimit")?;
    let mut p = GpuProfile { id: super::new_id(), name: name.into(), ..Default::default() };
    p.power_pct = num("PowerLimit").unwrap_or(100.0).round().clamp(1.0, 200.0) as u32;
    p.temp_limit = num("ThermalLimit").map(|v| v.round() as u32).filter(|v| *v > 0);
    p.core_mhz = (num("CoreClkBoost").unwrap_or(0.0) / 1000.0).round() as i32;
    p.mem_mhz = (num("MemClkBoost").unwrap_or(0.0) / 1000.0).round() as i32;
    p.volt_pct = num("CoreVoltageBoost").map(|v| v.round().clamp(0.0, 100.0) as u32).filter(|v| *v > 0);
    // a shaped curve wins over the plain offset; a uniform one is just the offset
    if let Some(pts) = sec.get("VFCurve").and_then(|h| curve::decode_ab(h)) {
        let first = pts.first().map(|q| q.delta).unwrap_or(0.0);
        if pts.iter().any(|q| (q.delta - first).abs() > 0.5) {
            p.curve = Some(pts.iter().map(|q| (q.mv, q.delta)).collect());
            p.core_mhz = 0;
        }
    }
    // FanMode 1 = automatic, 0 = fixed FanSpeed; Afterburner's software curve is global, not per profile
    p.fan = match num("FanMode").map(|v| v as i32) {
        Some(0) => GpuFan { mode: "fixed".into(), pct: num("FanSpeed").unwrap_or(50.0).round().clamp(0.0, 100.0) as u32, ..Default::default() },
        _ => GpuFan::default(),
    };
    p.icon = if p.core_mhz > 0 || p.curve.as_ref().map(|c| c.iter().any(|x| x.1 > 0.0)).unwrap_or(false) {
        "gpu-flame.ico".into()
    } else if p.power_pct < 80 {
        "gpu-leaf.ico".into()
    } else {
        "gpu-green.ico".into()
    };
    Some(p)
}

/// `ProfileNHotkey=0006004Fh`: high word = Afterburner's modifiers (1 Shift, 2 Ctrl, 4 Alt,
/// 8 extended key), low word = virtual key. Returns Deck's "Ctrl+Alt+O" or None.
pub fn hotkey(v: &str) -> Option<String> {
    let n = u32::from_str_radix(v.trim().trim_end_matches(['h', 'H']), 16).ok()?;
    let (m, vk) = (n >> 16, n & 0xFFFF);
    if vk == 0 {
        return None;
    }
    let mut mods = 0u32;
    if m & 1 != 0 {
        mods |= super::hotkeys::MOD_SHIFT;
    }
    if m & 2 != 0 {
        mods |= super::hotkeys::MOD_CONTROL;
    }
    if m & 4 != 0 {
        mods |= super::hotkeys::MOD_ALT;
    }
    let s = super::hotkeys::format(mods, vk);
    // a bare key (Afterburner allowed Page Up alone) would take that key away from every app
    super::hotkeys::parse(&s).map(|_| s)
}

/// Afterburner's monitoring source names → Deck overlay items.
pub fn osd_item(source: &str) -> Option<&'static str> {
    Some(match source {
        "GPU temperature" => "gpu_temp",
        "GPU usage" => "gpu_usage",
        "Memory usage" => "gpu_mem",
        "Core clock" => "gpu_clock",
        "Memory clock" => "gpu_memclock",
        "Power" | "Power percent" => "gpu_power",
        "Fan speed" | "Fan tachometer" => "gpu_fan",
        "GPU voltage" => "gpu_volt",
        "CPU temperature" => "cpu_temp",
        "CPU usage" => "cpu_usage",
        "CPU clock" => "cpu_clock",
        "CPU power" => "cpu_power",
        "RAM usage" => "ram",
        "RAM usage \\ process" => "proc_ram",
        "Memory usage \\ process" => "proc_vram",
        "Commit charge" => "commit",
        "Framerate" => "fps",
        "Frametime" => "frametime",
        s if s.starts_with("CPU") && s.ends_with(" usage") => "cpu_cores",
        _ => return None,
    })
}

#[derive(Debug, Default, Clone)]
pub struct Import {
    pub profiles: Vec<GpuProfile>,
    /// index into `profiles` of the [Startup] copy, if it isn't one of the slots
    pub startup: Option<String>,
    pub osd: Vec<String>,
    pub osd_hotkey: Option<String>,
    /// Afterburner's framerate limiter key
    pub limiter_hotkey: Option<String>,
    pub notes: Vec<String>,
}

/// Reads both files (text already loaded, for tests).
pub fn import_from(gpu_cfg: &str, main_cfg: Option<&str>) -> Import {
    let ini = parse_ini(gpu_cfg);
    let main = main_cfg.map(parse_ini);
    let msec = main.as_ref().and_then(|m| section(m, "Settings"));
    let mut out = Import::default();
    for n in 1..=5 {
        if let Some(mut p) = section(&ini, &format!("Profile{n}")).and_then(|s| to_profile(&s, &format!("AB Profile {n}"))) {
            p.id = super::new_id();
            if let Some(h) = msec.as_ref().and_then(|m| m.get(format!("Profile{n}Hotkey").as_str()).copied()).and_then(hotkey) {
                p.hotkey = h;
            }
            out.profiles.push(p);
        }
    }
    if let Some(mut st) = section(&ini, "Startup").and_then(|s| to_profile(&s, "AB Startup")) {
        // the startup profile usually equals one of the slots: point at that one instead
        let same = out.profiles.iter().find(|p| p.same_settings(&st));
        match same {
            Some(p) => out.startup = Some(p.id.clone()),
            None => {
                st.id = super::new_id();
                out.startup = Some(st.id.clone());
                out.profiles.push(st);
            }
        }
    }
    if let Some(m) = &main {
        for (name, kv) in m {
            if let Some(src) = name.strip_prefix("Source ") {
                if kv.iter().any(|(k, v)| k == "ShowInOSD" && v == "1") {
                    if let Some(item) = osd_item(src) {
                        if !out.osd.iter().any(|x| x == item) {
                            out.osd.push(item.to_string());
                        }
                    }
                }
            }
        }
        if let Some(h) = msec.as_ref().and_then(|s| s.get("LimiterToggleHotkey").copied()).and_then(hotkey) {
            out.limiter_hotkey = Some(h);
        }
        match msec.as_ref().and_then(|s| s.get("OSDToggleHotkey").copied()) {
            Some(v) if v.trim_start_matches('0').trim_end_matches(['h', 'H']).is_empty() => {}
            Some(v) => match hotkey(v) {
                Some(h) => out.osd_hotkey = Some(h),
                None => out.notes.push("Afterburner's overlay key has no Ctrl/Alt/Win, so it was not copied; pick one on the Sensors page.".into()),
            },
            None => {}
        }
    }
    out
}

/// Reads this PC's Afterburner files for the card `pci`.
pub fn import(pci: &str) -> Result<Import, String> {
    let dir = profiles_dir().ok_or("MSI Afterburner's settings were not found.")?;
    let f = gpu_cfg(&dir, pci).ok_or("Afterburner has no settings for this graphics card.")?;
    let gpu = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
    let main = std::fs::read_to_string(dir.join("MSIAfterburner.cfg")).ok();
    let r = import_from(&gpu, main.as_deref());
    if r.profiles.is_empty() {
        return Err("Afterburner's profile slots are empty.".into());
    }
    Ok(r)
}

pub fn summary(i: &Import) -> Value {
    json!({
        "profiles": i.profiles.iter().map(|p| json!({"id": p.id, "name": p.name, "summary": super::gpu_summary(p), "hotkey": p.hotkey})).collect::<Vec<_>>(),
        "startup": i.startup, "osd": i.osd, "osd_hotkey": i.osd_hotkey, "limiter_hotkey": i.limiter_hotkey, "notes": i.notes,
    })
}

// ----------------------------------------------------------------- stop Afterburner tuning

fn backups() -> PathBuf {
    crate::settings::Settings::dir().join("backups")
}

/// Rewrites INI text with `edit(section, key) -> Some(new value)`; keeps everything else byte for byte.
pub fn rewrite(text: &str, edit: impl Fn(&str, &str) -> Option<String>) -> String {
    let mut sec = String::new();
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let end = &line[body.len()..];
        if let Some(n) = body.trim().strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            sec = n.to_string();
        } else if let Some((k, _)) = body.split_once('=') {
            if let Some(v) = edit(&sec, k.trim()) {
                out.push_str(&format!("{}={}{}", k, v, end));
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

fn kill_afterburner() -> bool {
    let was = crate::gpu::afterburner_running();
    if was {
        let _ = crate::util::run_hidden("taskkill.exe", &["/IM", "MSIAfterburner.exe", "/F"]);
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    was
}

fn start_afterburner() {
    let base = crate::util::env_path("ProgramFiles(x86)").unwrap_or_else(|| PathBuf::from("C:\\Program Files (x86)"));
    let exe = base.join("MSI Afterburner").join("MSIAfterburner.exe");
    if exe.exists() {
        let _ = crate::util::launch_as_user(&exe.to_string_lossy(), "", "");
    }
}

/// Whether Afterburner would still change the card: a startup profile, a profile hotkey or its
/// own fan control. False after `stop_tuning` (it then only monitors).
pub fn tunes(gpu_cfg: &str, main_cfg: Option<&str>) -> bool {
    let g = parse_ini(gpu_cfg);
    let startup = section(&g, "Startup").map(|s| s.iter().any(|(k, v)| *k != "Format" && !v.trim().is_empty())).unwrap_or(false);
    let m = main_cfg.map(parse_ini).unwrap_or_default();
    let settings = section(&m, "Settings");
    let keys = settings.as_ref().map(|s| s.iter().any(|(k, v)| k.starts_with("Profile") && k.ends_with("Hotkey") && !matches!(v.trim(), "" | "00000000h"))).unwrap_or(false);
    let fan = settings.as_ref().and_then(|s| s.get("SwAutoFanControl").map(|v| v.trim() == "1")).unwrap_or(false);
    startup || keys || fan
}

/// `tunes` for this card's files (true when they can't be read: assume the worst).
pub fn tunes_card(pci: &str) -> bool {
    let Some(dir) = profiles_dir() else { return true };
    let Some(gpu) = gpu_cfg(&dir, pci).and_then(|f| std::fs::read_to_string(f).ok()) else { return true };
    tunes(&gpu, std::fs::read_to_string(dir.join("MSIAfterburner.cfg")).ok().as_deref())
}

/// Backs up Afterburner's profiles, closes it, clears its startup profile, its profile hotkeys
/// and its fan control, then starts it again (it stays usable for monitoring).
pub fn stop_tuning() -> Result<String, String> {
    let dir = profiles_dir().ok_or("MSI Afterburner's settings were not found.")?;
    let to = backups().join(format!("afterburner-{}", crate::util::now_s()));
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        if e.path().extension().map(|x| x.eq_ignore_ascii_case("cfg")).unwrap_or(false) {
            std::fs::copy(e.path(), to.join(e.file_name())).map_err(|e| format!("backup failed: {e}"))?;
        }
    }
    let was = kill_afterburner();
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if !name.to_lowercase().ends_with(".cfg") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let new = if name.eq_ignore_ascii_case("MSIAfterburner.cfg") {
            rewrite(&text, |sec, k| (sec == "Settings" && k.starts_with("Profile") && k.ends_with("Hotkey")).then(|| "00000000h".into()).or_else(|| (sec == "Settings" && k == "SwAutoFanControl").then(|| "0".into())))
        } else {
            rewrite(&text, |sec, k| (sec == "Startup" && k != "Format").then(String::new).or_else(|| (k == "FanMode" && sec.starts_with("Profile")).then(|| "1".into())))
        };
        if new != text {
            std::fs::write(&p, new).map_err(|e| format!("couldn't write {name}: {e}"))?;
        }
    }
    if was {
        start_afterburner();
    }
    Ok(to.to_string_lossy().to_string())
}

/// Puts the newest backup back.
pub fn undo_stop() -> Result<(), String> {
    let dir = profiles_dir().ok_or("MSI Afterburner's settings were not found.")?;
    let mut all: Vec<PathBuf> = std::fs::read_dir(backups()).map_err(|_| "No backup found.")?.flatten().map(|e| e.path()).filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("afterburner-")).unwrap_or(false)).collect();
    all.sort();
    let last = all.pop().ok_or("No backup found.")?;
    let was = kill_afterburner();
    for e in std::fs::read_dir(&last).map_err(|e| e.to_string())?.flatten() {
        std::fs::copy(e.path(), dir.join(e.file_name())).map_err(|e| e.to_string())?;
    }
    if was {
        start_afterburner();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_temp_survives_uninstall() {
        let f = std::env::temp_dir().join(format!("deck-gpu-defaults-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&f);
        assert_eq!(super::remember_temp(&f, "VEN_10DE&DEV_2704", None), None);
        assert_eq!(super::remember_temp(&f, "VEN_10DE&DEV_2704", Some(83)), Some(83));
        assert_eq!(super::remember_temp(&f, "VEN_10DE&DEV_2704", None), Some(83));
        assert_eq!(super::remember_temp(&f, "VEN_10DE&DEV_9999", None), None);
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn tunes_until_stopped() {
        let gpu = "[Startup]
Format=2
PowerLimit=41
ThermalLimit=
[Profile1]
PowerLimit=112
";
        let main = "[Settings]
Profile1Hotkey=00120030h
SwAutoFanControl=0
";
        assert!(tunes(gpu, Some(main)));
        let gpu_off = rewrite(gpu, |sec, k| (sec == "Startup" && k != "Format").then(String::new));
        assert!(tunes(&gpu_off, Some(main)), "a profile hotkey still tunes");
        let main_off = rewrite(main, |sec, k| (sec == "Settings" && k.starts_with("Profile") && k.ends_with("Hotkey")).then(|| "00000000h".into()));
        assert!(!tunes(&gpu_off, Some(&main_off)));
        assert!(tunes(&gpu_off, Some("[Settings]
SwAutoFanControl=1
")));
    }

    use super::*;

    const GPU: &str = include_str!("testdata/ab-gpu.cfg");
    const MAIN: &str = include_str!("testdata/ab-main.cfg");

    #[test]
    fn imports_this_pcs_profiles() {
        let i = import_from(GPU, Some(MAIN));
        let names: Vec<&str> = i.profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(names.starts_with(&["AB Profile 1", "AB Profile 2", "AB Profile 3", "AB Profile 4", "AB Profile 5"]), "{names:?}");
        let p1 = &i.profiles[0];
        assert_eq!((p1.power_pct, p1.core_mhz, p1.mem_mhz, p1.volt_pct), (112, 150, 500, Some(100)));
        assert!(p1.curve.is_none(), "a uniform +150 curve is just the offset");
        assert_eq!(p1.hotkey, "Ctrl+Alt+O");
        // slots 2-5 hold shaped curves (4 and 5 are flattened "L" curves): the curve wins
        let p4 = &i.profiles[3];
        assert_eq!((p4.power_pct, p4.core_mhz, p4.mem_mhz), (41, 0, 0));
        let c4 = p4.curve.as_ref().expect("profile 4 is a curve");
        assert_eq!(c4.len(), 127);
        assert_eq!(c4.last().map(|x| x.1.round()), Some(-493.0));
        assert_eq!(p4.hotkey, "Ctrl+Alt+]");
        assert_eq!(i.profiles[4].mem_mhz, -500);
        // [Startup] is the same as slot 5 (its CoreClkBoost is ignored because the curve is shaped)
        assert_eq!(i.startup.as_deref(), Some(i.profiles[4].id.as_str()));
        assert_eq!(i.profiles.len(), 5);
        assert!(i.osd.contains(&"fps".to_string()) && i.osd.contains(&"gpu_temp".to_string()) && i.osd.contains(&"cpu_cores".to_string()));
        assert!(!i.osd.contains(&"gpu_fb".to_string()));
        // the overlay key was Page Up alone: copied as it is
        assert_eq!(i.osd_hotkey.as_deref(), Some("PageUp"));
        assert_eq!(i.notes.len(), 0);
    }

    #[test]
    fn fan_and_curve_mapping() {
        let mut s: HashMap<&str, &str> = HashMap::new();
        s.insert("PowerLimit", "90");
        s.insert("FanMode", "0");
        s.insert("FanSpeed", "45");
        // header + 2 points: (700 mV, 1500 MHz, +0) and (800 mV, 1700, -100)
        let mut b = vec![0u8, 0, 2, 0, 2, 0, 0, 0, 0, 0, 0, 0];
        for v in [700f32, 1500.0, 0.0, 800.0, 1700.0, -100.0] {
            b.extend(v.to_le_bytes());
        }
        let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
        s.insert("VFCurve", &hex);
        let p = to_profile(&s, "x").unwrap();
        assert_eq!(p.fan.mode, "fixed");
        assert_eq!(p.fan.pct, 45);
        assert_eq!(p.curve, Some(vec![(700.0, 0.0), (800.0, -100.0)]));
        assert!(to_profile(&HashMap::new(), "empty").is_none());
    }

    #[test]
    fn hotkey_words() {
        assert_eq!(hotkey("0006004Fh").as_deref(), Some("Ctrl+Alt+O"));
        assert_eq!(hotkey("000600DBh").as_deref(), Some("Ctrl+Alt+["));
        assert_eq!(hotkey("00080021h").as_deref(), Some("PageUp"), "Page Up alone");
        assert_eq!(hotkey("00000000h"), None);
    }

    #[test]
    fn rewrite_keeps_the_rest() {
        let t = "[Startup]\r\nFormat=2\r\nPowerLimit=41\r\n[Profile1]\r\nFanMode=0\r\nPowerLimit=41\r\n";
        let r = rewrite(t, |sec, k| (sec == "Startup" && k != "Format").then(String::new).or_else(|| (k == "FanMode" && sec.starts_with("Profile")).then(|| "1".into())));
        assert_eq!(r, "[Startup]\r\nFormat=2\r\nPowerLimit=\r\n[Profile1]\r\nFanMode=1\r\nPowerLimit=41\r\n");
    }

    #[test]
    fn finds_card_file() {
        let d = std::env::temp_dir().join(format!("deck-ab-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("VEN_10DE&DEV_2484&SUBSYS_87C11043&REV_A1&BUS_1&DEV_0&FN_0.cfg"), "").unwrap();
        std::fs::write(d.join("VEN_10DE&DEV_1C82&SUBSYS_862A1043&REV_A1&BUS_1&DEV_0&FN_0.cfg"), "").unwrap();
        let f = gpu_cfg(&d, "VEN_10DE&DEV_2484&SUBSYS_87C11043&BUS_1&DEV_0").unwrap();
        assert!(f.to_string_lossy().contains("2484"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
