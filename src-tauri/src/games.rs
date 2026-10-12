//! Installed games from Steam and Epic, with launch links.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize, Clone, Debug)]
pub struct Game {
    pub id: String,
    pub name: String,
    /// "steam" | "epic"
    pub store: String,
    pub launch: String,
    pub dir: String,
    pub size: u64,
    /// unix seconds of the last update/play Steam recorded
    pub last: u64,
}

fn steam_root() -> Option<PathBuf> {
    let hk = winreg_read("Software\\Valve\\Steam", "SteamPath")?;
    let p = PathBuf::from(hk.replace('/', "\\"));
    p.is_dir().then_some(p)
}

/// Reads a string value from HKCU.
pub fn winreg_read(key: &str, value: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    };
    r.is_ok().then(|| crate::util::from_wide(&buf))
}

/// Every Steam library folder (the Steam install plus extra drives).
pub fn steam_libraries() -> Vec<PathBuf> {
    let Some(root) = steam_root() else { return Vec::new() };
    let mut out = vec![root.clone()];
    if let Ok(t) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for line in t.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("\"path\"") {
                let p = rest.trim().trim_matches('"').replace("\\\\", "\\");
                let pb = PathBuf::from(p);
                if pb.is_dir() && !out.iter().any(|x| x.to_string_lossy().eq_ignore_ascii_case(&pb.to_string_lossy())) {
                    out.push(pb);
                }
            }
        }
    }
    out
}

fn vdf_value(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix(&needle) {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Steam tools and runtimes that aren't games.
fn not_a_game(name: &str) -> bool {
    let n = name.to_lowercase();
    ["steamworks common", "redistributable", "proton", "steam linux runtime", "wallpaper engine", "blender", "soundtrack", "dedicated server", "sdk"]
        .iter()
        .any(|w| n.contains(w))
}

pub fn installed() -> Vec<Game> {
    let mut out = Vec::new();
    for lib in steam_libraries() {
        let apps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if !fname.starts_with("appmanifest_") {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(e.path()) else { continue };
            let (Some(id), Some(name)) = (vdf_value(&t, "appid"), vdf_value(&t, "name")) else { continue };
            if not_a_game(&name) {
                continue;
            }
            let dir = vdf_value(&t, "installdir").map(|d| apps.join("common").join(d).to_string_lossy().to_string()).unwrap_or_default();
            out.push(Game {
                launch: format!("steam://rungameid/{id}"),
                id: format!("steam:{id}"),
                name,
                store: "steam".into(),
                dir,
                size: vdf_value(&t, "SizeOnDisk").and_then(|s| s.parse().ok()).unwrap_or(0),
                last: vdf_value(&t, "LastPlayed").or_else(|| vdf_value(&t, "LastUpdated")).and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
    }
    // Epic: one JSON manifest per installed game.
    let epic = PathBuf::from("C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests");
    if let Ok(rd) = std::fs::read_dir(epic) {
        for e in rd.flatten() {
            if e.path().extension().map(|x| x != "item").unwrap_or(true) {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(e.path()) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { continue };
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let app = s("AppName");
            if app.is_empty() || v.get("bIsApplication").and_then(|b| b.as_bool()) == Some(false) {
                continue;
            }
            out.push(Game {
                id: format!("epic:{app}"),
                name: s("DisplayName"),
                store: "epic".into(),
                launch: format!("com.epicgames.launcher://apps/{app}?action=launch&silent=true"),
                dir: s("InstallLocation"),
                size: v.get("InstallSize").and_then(|x| x.as_u64()).unwrap_or(0),
                last: 0,
            });
        }
    }
    out.sort_by(|a, b| b.last.cmp(&a.last).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}
