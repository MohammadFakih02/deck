//! "Setup snapshots": this PC's Windows look-and-feel values, wallpaper and Windhawk mods packed into a folder
//! that is sent like any other folder, and applied on the other PC from the Received tab.
//! Applying always backs up what it is about to change first, and can be undone.
//! Only a whitelist of UI values is touched - never display scaling, drivers, power or anything hardware related.
use crate::beam::winutil::wide;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;

const NO_WINDOW: u32 = 0x0800_0000;
pub const MARK: &str = "AppBeamSetup";
const WH_DATA: &str = r"C:\ProgramData\Windhawk";
const WH_EXE: &str = r"C:\Program Files\Windhawk\windhawk.exe";
const WH_KEY: &str = r"HKLM\SOFTWARE\Windhawk";

/// (key under HKCU, values) that make up "look and feel"
const LOOK: &[(&str, &[&str])] = &[
    (r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", &["AppsUseLightTheme", "SystemUsesLightTheme", "EnableTransparency", "ColorPrevalence"]),
    (r"Software\Microsoft\Windows\DWM", &["AccentColor", "AccentColorInactive", "ColorPrevalence"]),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
        &["TaskbarAl", "ShowTaskViewButton", "TaskbarDa", "TaskbarMn", "TaskbarSmallIcons", "HideFileExt", "Hidden", "ShowSuperHidden", "LaunchTo", "UseCompactMode"],
    ),
    (r"Software\Microsoft\Windows\CurrentVersion\Search", &["SearchboxTaskbarMode"]),
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegVal {
    pub path: String,
    pub name: String,
    /// "dword" or "sz"
    pub kind: String,
    /// None = the value doesn't exist (used in backups so undo can remove it again)
    pub data: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Manifest {
    pub app: String,
    pub from: String,
    pub time: u64,
    pub look: Vec<RegVal>,
    /// file name of the wallpaper inside the snapshot
    pub wallpaper: Option<String>,
    pub windhawk_version: Option<String>,
    pub windhawk_mods: Vec<String>,
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()?;
    (m.app == MARK).then_some(m)
}

// ------------------------------------------------------------------------------------------ registry (HKCU)
fn reg_get(path: &str, name: &str) -> Option<(String, String)> {
    unsafe {
        let mut h: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_READ, &mut h) != 0 {
            return None;
        }
        let n = wide(name);
        let (mut ty, mut buf, mut len) = (0u32, [0u8; 2048], 2048u32);
        let r = RegQueryValueExW(h, n.as_ptr(), std::ptr::null(), &mut ty, buf.as_mut_ptr(), &mut len);
        RegCloseKey(h);
        if r != 0 {
            return None;
        }
        match ty {
            4 if len >= 4 => Some(("dword".into(), u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]).to_string())),
            1 => {
                let u: Vec<u16> = buf[..len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
                Some(("sz".into(), String::from_utf16_lossy(&u)))
            }
            _ => None,
        }
    }
}

fn reg_set(v: &RegVal) -> bool {
    unsafe {
        let mut h: HKEY = std::ptr::null_mut();
        let mut disp = 0u32;
        if RegCreateKeyExW(HKEY_CURRENT_USER, wide(&v.path).as_ptr(), 0, std::ptr::null(), 0, KEY_WRITE, std::ptr::null(), &mut h, &mut disp) != 0 {
            return false;
        }
        let n = wide(&v.name);
        let ok = match (&v.data, v.kind.as_str()) {
            (Some(d), "dword") => match d.parse::<u32>() {
                Ok(x) => RegSetValueExW(h, n.as_ptr(), 0, 4, x.to_le_bytes().as_ptr(), 4) == 0,
                Err(_) => false,
            },
            (Some(d), _) => {
                let w = wide(d);
                RegSetValueExW(h, n.as_ptr(), 0, 1, w.as_ptr() as *const u8, (w.len() * 2) as u32) == 0
            }
            (None, _) => RegDeleteValueW(h, n.as_ptr()) == 0 || true,
        };
        RegCloseKey(h);
        ok
    }
}

fn current(path: &str, name: &str, want_kind: &str) -> RegVal {
    match reg_get(path, name) {
        Some((k, d)) => RegVal { path: path.into(), name: name.into(), kind: k, data: Some(d) },
        None => RegVal { path: path.into(), name: name.into(), kind: want_kind.into(), data: None },
    }
}

// ------------------------------------------------------------------------------------------ helpers
fn run(prog: &str, args: &[&str]) -> bool {
    Command::new(prog).args(args).creation_flags(NO_WINDOW).status().map(|s| s.success()).unwrap_or(false)
}

fn copy_dir(from: &Path, to: &Path, only_new: bool) -> std::io::Result<usize> {
    let mut n = 0;
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let dst = to.join(e.file_name());
        let t = e.file_type()?;
        if t.is_dir() {
            n += copy_dir(&e.path(), &dst, only_new)?;
        } else if t.is_file() && !(only_new && dst.exists()) {
            std::fs::copy(e.path(), &dst)?;
            n += 1;
        }
    }
    Ok(n)
}

pub fn windhawk_installed() -> bool {
    Path::new(WH_EXE).exists()
}

fn windhawk_profile() -> Option<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(Path::new(WH_DATA).join("userprofile.json")).ok()?).ok()
}

pub fn windhawk_version() -> Option<String> {
    windhawk_profile()?["app"]["version"].as_str().map(|s| s.to_string())
}

/// ids of the mods Windhawk has settings for
fn windhawk_mod_ids() -> Vec<String> {
    let out = Command::new("reg.exe").args(["query", &format!(r"{WH_KEY}\Engine\Mods")]).creation_flags(NO_WINDOW).output();
    let Ok(o) = out else { return vec![] };
    String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().rsplit_once(r"\Mods\").map(|(_, id)| id.trim().to_string())).filter(|id| !id.is_empty() && !id.contains('\\')).collect()
}

// ------------------------------------------------------------------------------------------ build
pub struct Counts {
    pub look: usize,
    pub wallpaper: bool,
    pub windhawk: Vec<String>,
}

pub fn preview() -> Counts {
    let look = LOOK.iter().flat_map(|(p, ns)| ns.iter().map(move |n| (p, n))).filter(|(p, n)| reg_get(p, n).is_some()).count();
    Counts { look, wallpaper: wallpaper_path().is_some(), windhawk: if windhawk_installed() { windhawk_mod_ids() } else { vec![] } }
}

fn wallpaper_path() -> Option<PathBuf> {
    let p = reg_get(r"Control Panel\Desktop", "WallPaper")?.1;
    let p = PathBuf::from(p);
    (p.is_file() && std::fs::metadata(&p).map(|m| m.len() < 40 << 20).unwrap_or(false)).then_some(p)
}

/// packs the chosen parts into a fresh folder under `out_root` and returns it
pub fn build(out_root: &Path, machine: &str, look: bool, wallpaper: bool, windhawk: bool) -> Result<PathBuf, String> {
    let dir = out_root.join(format!("Setup from {machine}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut m = Manifest { app: MARK.into(), from: machine.into(), time: crate::beam::config::now_ms(), ..Default::default() };
    if look {
        for (p, ns) in LOOK {
            for n in *ns {
                if let Some((k, d)) = reg_get(p, n) {
                    m.look.push(RegVal { path: p.to_string(), name: n.to_string(), kind: k, data: Some(d) });
                }
            }
        }
    }
    if wallpaper {
        if let Some(p) = wallpaper_path() {
            let name = format!("wallpaper.{}", p.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "jpg".into()));
            std::fs::copy(&p, dir.join(&name)).map_err(|e| format!("wallpaper: {e}"))?;
            m.wallpaper = Some(name);
        }
    }
    if windhawk {
        if !windhawk_installed() {
            return Err("Windhawk isn't installed on this PC".into());
        }
        let wh = dir.join("windhawk");
        let data = Path::new(WH_DATA);
        copy_dir(&data.join("ModsSource"), &wh.join("ModsSource"), false).map_err(|e| format!("Windhawk mod sources: {e}"))?;
        for bits in ["32", "64"] {
            let src = data.join("Engine").join("Mods").join(bits);
            let dst = wh.join("Mods").join(bits);
            let _ = std::fs::create_dir_all(&dst);
            // the mods' own DLLs only; Windhawk's runtime files (.whl, shim) come with Windhawk itself
            for e in std::fs::read_dir(&src).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.ends_with(".dll") && !n.starts_with("windhawk-mod-shim") {
                    std::fs::copy(e.path(), dst.join(&n)).map_err(|e| format!("Windhawk mod {n}: {e}"))?;
                }
            }
        }
        std::fs::copy(data.join("userprofile.json"), wh.join("userprofile.json")).map_err(|e| format!("Windhawk profile: {e}"))?;
        if !run("reg.exe", &["export", WH_KEY, &wh.join("windhawk.reg").display().to_string(), "/y"]) {
            return Err("couldn't export Windhawk's settings".into());
        }
        m.windhawk_version = windhawk_version();
        m.windhawk_mods = windhawk_mod_ids();
    }
    if m.look.is_empty() && m.wallpaper.is_none() && m.windhawk_version.is_none() {
        return Err("nothing to put in the snapshot".into());
    }
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&m).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(dir)
}

// ------------------------------------------------------------------------------------------ apply (normal rights)
fn backup_dir(dir: &Path) -> PathBuf {
    dir.join("_backup")
}

pub fn has_backup(dir: &Path) -> bool {
    backup_dir(dir).join("look.json").exists() || backup_dir(dir).join("windhawk").exists()
}

fn restart_explorer() {
    let _ = Command::new("taskkill.exe").args(["/F", "/IM", "explorer.exe"]).creation_flags(NO_WINDOW).status();
    std::thread::sleep(std::time::Duration::from_millis(700));
    let _ = Command::new("explorer.exe").spawn();
}

fn set_wallpaper(p: &str) {
    unsafe {
        SystemParametersInfoW(0x14, 0, wide(p).as_ptr() as *mut _, 3);
    }
}

/// returns a one-line summary
pub fn apply_look(dir: &Path, m: &Manifest, look: bool, wallpaper: bool, restart: bool) -> Result<String, String> {
    let bk = backup_dir(dir);
    std::fs::create_dir_all(&bk).map_err(|e| e.to_string())?;
    // back up the current values before touching them (only the first apply - a second one must not overwrite the originals)
    if !bk.join("look.json").exists() {
        let mut saved: Vec<RegVal> = m.look.iter().map(|v| current(&v.path, &v.name, &v.kind)).collect();
        let old_wp = reg_get(r"Control Panel\Desktop", "WallPaper").map(|x| x.1);
        saved.push(RegVal { path: "wallpaper".into(), name: "WallPaper".into(), kind: "sz".into(), data: old_wp });
        std::fs::write(bk.join("look.json"), serde_json::to_vec_pretty(&saved).unwrap_or_default()).map_err(|e| e.to_string())?;
    }
    let mut n = 0;
    if look {
        for v in &m.look {
            if reg_set(v) {
                n += 1;
            }
        }
    }
    let mut wp = false;
    if wallpaper {
        if let Some(f) = &m.wallpaper {
            let src = dir.join(f);
            if src.is_file() {
                set_wallpaper(&src.display().to_string());
                wp = true;
            }
        }
    }
    if look && restart {
        restart_explorer();
    }
    Ok(format!("{n} settings{} applied", if wp { " and the wallpaper" } else { "" }))
}

pub fn undo_look(dir: &Path, restart: bool) -> Result<String, String> {
    let f = backup_dir(dir).join("look.json");
    let saved: Vec<RegVal> = serde_json::from_slice(&std::fs::read(&f).map_err(|_| "no backup found".to_string())?).map_err(|e| e.to_string())?;
    for v in &saved {
        if v.path == "wallpaper" {
            if let Some(p) = &v.data {
                set_wallpaper(p);
            }
        } else {
            reg_set(v);
        }
    }
    let _ = std::fs::remove_file(&f);
    if restart {
        restart_explorer();
    }
    Ok("Settings restored to how they were".into())
}

// ------------------------------------------------------------------------------------------ Windhawk (runs elevated)
/// run as `AppBeam.exe --apply-windhawk <dir>` / `--undo-windhawk <dir>` with administrator rights; exit code 0 = ok.
/// The reason for a failure goes to `<dir>/_wh_result.txt`.
pub fn windhawk_elevated(dir: &Path, undo: bool) -> i32 {
    let res = if undo { windhawk_undo(dir) } else { windhawk_apply(dir) };
    let (code, msg) = match res {
        Ok(m) => (0, m),
        Err(e) => (1, e),
    };
    let _ = std::fs::write(dir.join("_wh_result.txt"), msg);
    code
}

fn windhawk_apply(dir: &Path) -> Result<String, String> {
    let m = read_manifest(dir).ok_or("not a setup snapshot")?;
    let src = dir.join("windhawk");
    if !src.exists() {
        return Err("this snapshot has no Windhawk data".into());
    }
    if !windhawk_installed() {
        return Err("Windhawk isn't installed on this PC - install it first (you can send its installer with AppBeam)".into());
    }
    let (mine, theirs) = (windhawk_version().unwrap_or_default(), m.windhawk_version.clone().unwrap_or_default());
    if mine != theirs {
        return Err(format!("Windhawk versions differ (this PC {mine}, snapshot {theirs}). Update Windhawk on the PC that is behind, then try again"));
    }
    let data = Path::new(WH_DATA);
    let bk = backup_dir(dir).join("windhawk");
    if !bk.exists() {
        std::fs::create_dir_all(&bk).map_err(|e| e.to_string())?;
        let _ = copy_dir(&data.join("ModsSource"), &bk.join("ModsSource"), false);
        let _ = std::fs::copy(data.join("userprofile.json"), bk.join("userprofile.json"));
        if !run("reg.exe", &["export", WH_KEY, &bk.join("windhawk.reg").display().to_string(), "/y"]) {
            return Err("couldn't back up Windhawk's settings, so nothing was changed".into());
        }
        let names = |p: PathBuf| -> Vec<String> { std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect() };
        let list = serde_json::json!({
            "mods": windhawk_mod_ids(),
            "sources": names(data.join("ModsSource")),
            "dll32": names(data.join("Engine/Mods/32")),
            "dll64": names(data.join("Engine/Mods/64")),
        });
        std::fs::write(bk.join("before.json"), list.to_string()).map_err(|e| e.to_string())?;
    }
    copy_dir(&src.join("ModsSource"), &data.join("ModsSource"), false).map_err(|e| format!("copying mod sources: {e}"))?;
    for bits in ["32", "64"] {
        let from = src.join("Mods").join(bits);
        if from.exists() {
            copy_dir(&from, &data.join("Engine").join("Mods").join(bits), true).map_err(|e| format!("copying mods: {e}"))?;
        }
    }
    // keep this PC's own profile id, take the mod list
    if let (Some(mut mine), Ok(theirs)) = (windhawk_profile(), std::fs::read(src.join("userprofile.json"))) {
        if let Ok(t) = serde_json::from_slice::<serde_json::Value>(&theirs) {
            mine["mods"] = t["mods"].clone();
            std::fs::write(data.join("userprofile.json"), serde_json::to_vec_pretty(&mine).unwrap_or_default()).map_err(|e| e.to_string())?;
        }
    }
    if !run("reg.exe", &["import", &src.join("windhawk.reg").display().to_string()]) {
        return Err("couldn't import Windhawk's settings".into());
    }
    Ok(format!("Windhawk: {} mod(s) copied with their settings", m.windhawk_mods.len()))
}

fn windhawk_undo(dir: &Path) -> Result<String, String> {
    let bk = backup_dir(dir).join("windhawk");
    let before: serde_json::Value = serde_json::from_slice(&std::fs::read(bk.join("before.json")).map_err(|_| "no Windhawk backup found".to_string())?).map_err(|e| e.to_string())?;
    let data = Path::new(WH_DATA);
    let list = |k: &str| -> Vec<String> { before[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default() };
    // remove what the snapshot added
    for id in windhawk_mod_ids() {
        if !list("mods").contains(&id) {
            run("reg.exe", &["delete", &format!(r"{WH_KEY}\Engine\Mods\{id}"), "/f"]);
        }
    }
    for (sub, key) in [("ModsSource", "sources"), ("Engine/Mods/32", "dll32"), ("Engine/Mods/64", "dll64")] {
        let had = list(key);
        for e in std::fs::read_dir(data.join(sub)).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !had.contains(&n) && e.path().is_file() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = copy_dir(&bk.join("ModsSource"), &data.join("ModsSource"), false);
    let _ = std::fs::copy(bk.join("userprofile.json"), data.join("userprofile.json"));
    if !run("reg.exe", &["import", &bk.join("windhawk.reg").display().to_string()]) {
        return Err("couldn't restore Windhawk's settings".into());
    }
    let _ = std::fs::remove_dir_all(&bk);
    Ok("Windhawk restored to how it was".into())
}

/// Windhawk keeps its engine inside other programs, so it needs a restart to pick up new mods
pub fn restart_windhawk() {
    let _ = Command::new("taskkill.exe").args(["/F", "/IM", "windhawk.exe"]).creation_flags(NO_WINDOW).status();
    std::thread::sleep(std::time::Duration::from_millis(600));
    // through explorer so it doesn't inherit our rights
    let _ = Command::new("explorer.exe").arg(WH_EXE).spawn();
}
