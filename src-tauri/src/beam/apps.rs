//! Installed-apps list and "send apps, install them there" without any installer files:
//! this PC's apps are listed through winget (which knows their package ids); only the list of ids is sent,
//! and the other PC installs each one itself through winget, skipping what it already has.
use crate::beam::config::Shared;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub const MARK: &str = "AppBeamApps";
const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct AppList {
    pub app: String,
    pub from: String,
    pub apps: Vec<App>,
}

pub fn read_list(dir: &Path) -> Option<AppList> {
    let l: AppList = serde_json::from_slice(&std::fs::read(dir.join("apps.json")).ok()?).ok()?;
    (l.app == MARK).then_some(l)
}

/// runtimes, frameworks and system pieces nobody picks by hand
pub fn is_runtime(id: &str) -> bool {
    ["Microsoft.VCRedist", "Microsoft.VCLibs", "Microsoft.DotNet", "Microsoft.UI.Xaml", "Microsoft.DirectX", "Microsoft.AppInstaller", "Microsoft.WindowsAppRuntime", "Microsoft.GameInput", "Microsoft.XNA"]
        .iter()
        .any(|p| id.starts_with(p))
}

fn winget(args: &[&str]) -> Option<(i32, String)> {
    let o = Command::new("winget.exe").args(args).creation_flags(NO_WINDOW).output().ok()?;
    Some((o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string()))
}

pub fn winget_available() -> bool {
    winget(&["--version"]).map(|(c, _)| c == 0).unwrap_or(false)
}

/// apps on this PC that winget can reinstall, sorted by name
pub fn list_installed() -> Result<Vec<App>, String> {
    let (_, out) = winget(&["list", "--source", "winget", "--accept-source-agreements", "--disable-interactivity"]).ok_or("winget isn't available on this PC (it comes with App Installer from the Microsoft Store)")?;
    let lines: Vec<&str> = out.lines().collect();
    let h = lines.iter().position(|l| l.starts_with("Name") && l.contains("Id")).ok_or("couldn't read winget's list")?;
    let header: Vec<char> = lines[h].chars().collect();
    let find = |w: &str| -> Option<usize> { lines[h].find(w).map(|b| lines[h][..b].chars().count()) };
    let (id_at, ver_at) = (find("Id").ok_or("winget list format")?, find("Version").ok_or("winget list format")?);
    let _ = header;
    let cut = |l: &str, a: usize, b: Option<usize>| -> String {
        let c: Vec<char> = l.chars().collect();
        let end = b.unwrap_or(c.len()).min(c.len());
        if a >= end { String::new() } else { c[a..end].iter().collect::<String>().trim().to_string() }
    };
    let avail_at = find("Available");
    let mut apps: Vec<App> = Vec::new();
    for l in lines.iter().skip(h + 1) {
        if l.trim().is_empty() || l.starts_with("---") {
            continue;
        }
        let (name, id, ver) = (cut(l, 0, Some(id_at)), cut(l, id_at, Some(ver_at)), cut(l, ver_at, avail_at));
        if id.is_empty() || id.contains(' ') || apps.iter().any(|a| a.id == id) {
            continue;
        }
        apps.push(App { id, name, version: ver });
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok(apps)
}

/// folder to send: `apps.json` only; the other PC reads the ids from it
pub fn build(out_root: &Path, machine: &str, apps: &[App]) -> Result<PathBuf, String> {
    let dir = out_root.join(format!("{} app{} from {machine}", apps.len(), if apps.len() == 1 { "" } else { "s" }));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let l = AppList { app: MARK.into(), from: machine.into(), apps: apps.to_vec() };
    std::fs::write(dir.join("apps.json"), serde_json::to_vec_pretty(&l).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// installs every app of a received list on a background thread, one after another
pub fn install_async(sh: Arc<Shared>, dir: String) {
    std::thread::spawn(move || {
        let Some(list) = read_list(Path::new(&dir)) else {
            crate::beam::setup::set_note(&sh, &dir, "This isn't an app list".into());
            return;
        };
        if !winget_available() {
            crate::beam::setup::set_note(&sh, &dir, "winget isn't available on this PC - install \"App Installer\" from the Microsoft Store, then try again".into());
            return;
        }
        let n = list.apps.len();
        let (mut ok, mut had, mut failed) = (0, 0, Vec::new());
        for (i, a) in list.apps.iter().enumerate() {
            crate::beam::setup::set_note(&sh, &dir, format!("Installing {}/{n}: {}...", i + 1, a.name));
            // already here?
            let there = winget(&["list", "--id", &a.id, "--exact", "--accept-source-agreements", "--disable-interactivity"]).map(|(c, o)| c == 0 && o.contains(&a.id)).unwrap_or(false);
            if there {
                had += 1;
                continue;
            }
            // network hiccups (a big download over Wi-Fi) are retried; anything else fails straight away
            let mut last = None;
            for attempt in 1..=3 {
                if attempt > 1 {
                    crate::beam::setup::set_note(&sh, &dir, format!("Installing {}/{n}: {} - retrying ({attempt}/3)...", i + 1, a.name));
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
                last = winget(&["install", "--id", &a.id, "--exact", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"]);
                match &last {
                    Some((0, _)) => break,
                    Some((c, _)) if is_network_error(*c) => continue,
                    _ => break,
                }
            }
            match last {
                Some((0, _)) => ok += 1,
                Some((c, out)) => {
                    let tail: Vec<&str> = out.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with(['-', '\\', '|', '/'])).collect();
                    sh.log(format!("winget install {} failed (code {c}): {}", a.id, tail.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" / ")));
                    failed.push(format!("{} ({})", a.name, explain(c)));
                }
                None => failed.push(format!("{} (winget couldn't start)", a.name)),
            }
        }
        let mut note = format!("{ok} installed");
        if had > 0 {
            note += &format!(", {had} already there");
        }
        if !failed.is_empty() {
            note += &format!(", {} failed: {}. Click \"Retry failed\" to try just those again.", failed.len(), failed.join(", "));
        }
        sh.log(format!("Apps from {}: {note}", list.from));
        crate::beam::setup::set_note(&sh, &dir, note);
    });
}

/// winget exit codes are HRESULTs; these are the "the network let us down" ones
fn is_network_error(code: i32) -> bool {
    matches!(code as u32, 0x80072EE2 | 0x80072EFD | 0x80072EE7 | 0x80072F8F | 0x80072EFE | 0x80072EFF | 0x80072EE4)
}

fn explain(code: i32) -> String {
    match code as u32 {
        0x80072EE2 => "the download timed out - check the connection".into(),
        0x80072EFD | 0x80072EE7 => "couldn't reach the download server".into(),
        0x80072EFE | 0x80072EFF => "the connection was interrupted".into(),
        0x80072F8F => "secure connection failed - check the PC's date and time".into(),
        0x8A150011 => "no installer for this PC".into(),
        0x8A150014 => "winget doesn't have this app".into(),
        0x8A15002B => "no newer version applies".into(),
        0x8A150010 => "the download didn't match its checksum".into(),
        0x80070005 => "permission was denied".into(),
        0x800704C7 => "cancelled".into(),
        c => format!("winget code {c:#010X}"),
    }
}
