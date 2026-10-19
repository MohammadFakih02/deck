//! Which game is in front, by name. Any game counts: an installed Steam or Epic game is known by
//! the folder its exe runs from (so it gets its store name and Steam id), anything else that
//! covers its monitor by its exe. The catalogue only adds extras on top.

use super::catalog::{self, Entry};
use parking_lot::Mutex;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    /// "exe:<stem>" (or "exe:<folder>-<stem>" for a shared exe name) in every store, so a Steam, GOG
    /// or Game Pass copy of a game is the same game; "emu:<name>" for an emulated one
    pub key: String,
    pub name: String,
    pub entry: Option<&'static Entry>,
    /// the install folder (a store game's) or the exe's folder, for save readers
    pub dir: String,
}

/// The game window in front: (game, window, its monitor rect).
#[derive(Clone, Debug)]
pub struct Front {
    pub game: Game,
    /// the window screen detection captures
    pub hwnd: isize,
    pub monitor: (i32, i32, i32, i32),
}

/// (install folder in lower case with a trailing '\', store id, store name)
type Dirs = Vec<(String, String, String)>;

static LIB: Mutex<Option<(Instant, Dirs)>> = Mutex::new(None);

fn installed_dirs(max_age: Duration) -> Dirs {
    let mut lib = LIB.lock();
    if lib.as_ref().is_none_or(|(t, _)| t.elapsed() > max_age) {
        let dirs = crate::games::installed()
            .into_iter()
            .filter(|g| !g.dir.is_empty())
            .map(|g| {
                let mut d = g.dir.to_lowercase().replace('/', "\\");
                if !d.ends_with('\\') {
                    d.push('\\');
                }
                (d, g.id, g.name)
            })
            .collect();
        *lib = Some((Instant::now(), dirs));
    }
    lib.as_ref().map(|l| l.1.clone()).unwrap_or_default()
}

/// The installed game whose folder holds `exe_path` (the deepest folder wins).
pub fn match_dir<'a>(dirs: &'a [(String, String, String)], exe_path: &str) -> Option<&'a (String, String, String)> {
    let p = exe_path.to_lowercase().replace('/', "\\");
    dirs.iter().filter(|(d, _, _)| p.starts_with(d.as_str())).max_by_key(|(d, _, _)| d.len())
}

use catalog::slug;

/// Names a game from where its exe is and its window title. The key is always the exe (the same
/// game in any store); an installed Steam or Epic game adds its store name and, for Steam, the
/// catalogue entry by its number. None: not an installed game, not one the catalogue knows by its
/// exe, and `fullscreen` is false (an ordinary app in a window).
pub fn identify_with(dirs: &[(String, String, String)], exe_path: &str, title: &str, fullscreen: bool) -> Option<Game> {
    let file = exe_path.rsplit(['\\', '/']).next().unwrap_or(exe_path);
    let exe_dir = exe_path[..exe_path.len() - file.len()].trim_end_matches(['\\', '/']).to_string();
    let known = catalog::by_exe(file, title);
    if let Some(e) = known.filter(|e| !e.title.is_empty()) {
        // an emulator: the game is in its window title
        return Some(Game { key: format!("emu:{}", slug(&e.name)), name: e.name.clone(), entry: Some(e), dir: exe_dir });
    }
    let store = match_dir(dirs, exe_path);
    if store.is_none() && known.is_none() && !fullscreen {
        return None;
    }
    let entry = store.and_then(|(_, id, _)| id.strip_prefix("steam:").and_then(|n| n.parse().ok()).and_then(catalog::by_steam)).or(known);
    let name = match (store, entry) {
        (Some((_, _, n)), _) => n.clone(),
        (None, Some(e)) => e.name.clone(),
        _ if !title.trim().is_empty() && title.chars().count() <= 60 => title.trim().to_string(),
        _ => crate::util::exe_stem(file),
    };
    let dir = store.map_or(exe_dir, |(d, _, _)| d.trim_end_matches('\\').to_string());
    Some(Game { key: exe_key(exe_path, file), name, entry, dir })
}

/// Exe names many different games share (RPG Maker's Game.exe, NW.js, LOVE, launchers): such a game
/// is told apart by its folder. Slugs (lower case letters and digits).
pub const GENERIC_EXES: &[&str] = &["game", "nw", "launcher", "start", "main", "play", "app", "client", "run", "player", "application", "index", "electron", "love", "godot", "unityplayer", "game64", "gamex64", "win64", "x64", "bin", "shipping"];
/// Folders that say nothing about the game (build output), skipped when naming it by its folder.
const BUILD_DIRS: &[&str] = &["bin", "bin64", "bin32", "x64", "x86", "win64", "win32", "binaries", "windows", "game", "release", "shipping", "data"];

pub fn generic_exe(stem_slug: &str) -> bool {
    GENERIC_EXES.contains(&stem_slug)
}

/// "exe:<stem>", or "exe:<folder>-<stem>" for an exe name many games share.
pub fn exe_key(exe_path: &str, file: &str) -> String {
    let stem = catalog::stem_key(file);
    if !generic_exe(&stem) {
        return format!("exe:{stem}");
    }
    let folder = exe_path.rsplit(['\\', '/']).skip(1).map(slug).find(|f| !f.is_empty() && !BUILD_DIRS.contains(&f.as_str()) && !f.ends_with(':'));
    match folder {
        Some(f) => format!("exe:{}-{stem}", f.chars().take(48).collect::<String>()),
        None => format!("exe:{stem}"),
    }
}

/// Windows' own programs, Deck itself, browsers and video players are never games (a fullscreen
/// video of a game would otherwise count its deaths).
fn not_a_game(exe: &str) -> bool {
    let e = exe.rsplit('\\').next().unwrap_or(exe).to_ascii_lowercase();
    matches!(e.as_str(), "explorer.exe" | "searchhost.exe" | "startmenuexperiencehost.exe" | "lockapp.exe" | "shellexperiencehost.exe" | "applicationframehost.exe" | "deck.exe" | "taskmgr.exe")
        || crate::boost::not_a_game(exe)
}

/// Programs already looked for in a fresh list of installed games: not looked for again for a while
/// (a fresh list reads every Steam and Epic manifest).
static MISSED: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());
const MISS_FOR: Duration = Duration::from_secs(600);

/// True when `path` was looked for in a fresh list a moment ago; otherwise remembers that it is now.
fn looked_for_lately(path: &str) -> bool {
    let mut m = MISSED.lock();
    m.retain(|(_, t)| t.elapsed() < MISS_FOR);
    if m.iter().any(|(p, _)| p == path) {
        return true;
    }
    if m.len() >= 200 {
        m.remove(0);
    }
    m.push((path.to_string(), Instant::now()));
    false
}

/// The game in front right now, or None (desktop, an app, Deck's own windows).
pub fn front() -> Option<Front> {
    unsafe {
        let w: HWND = GetForegroundWindow();
        if w.is_null() || IsIconic(w) != 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let path = crate::util::process_path(pid)?;
        if not_a_game(&path) {
            return None;
        }
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(w, &mut r);
        let mon = MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let m = mi.rcMonitor;
        let style = GetWindowLongW(w, GWL_STYLE) as u32;
        let full = crate::hw::game::covers_like_fullscreen((r.left, r.top, r.right, r.bottom), (m.left, m.top, m.right, m.bottom), style);
        let mut t = [0u16; 256];
        let n = GetWindowTextW(w, t.as_mut_ptr(), 256).max(0) as usize;
        let title = String::from_utf16_lossy(&t[..n]);
        // a game installed a moment ago isn't in the cached list yet: look again (at most every 30 s,
        // and once per program: an app that isn't a game doesn't make Deck read every manifest again)
        let cached = installed_dirs(Duration::from_secs(600));
        let game = identify_with(&cached, &path, &title, false).or_else(|| {
            let dirs = if looked_for_lately(&path) { cached.clone() } else { installed_dirs(Duration::from_secs(30)) };
            identify_with(&dirs, &path, &title, full)
        })?;
        Some(Front { game, hwnd: w as isize, monitor: (m.left, m.top, m.right, m.bottom) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> Dirs {
        vec![
            ("d:\\steam\\steamapps\\common\\elden ring\\".into(), "steam:1245620".into(), "ELDEN RING".into()),
            ("d:\\steam\\steamapps\\common\\".into(), "steam:1".into(), "Too broad".into()),
            ("c:\\games\\hades\\".into(), "epic:Min".into(), "Hades".into()),
        ]
    }

    #[test]
    fn an_installed_game_is_known_by_its_folder() {
        let g = identify_with(&dirs(), "D:\\Steam\\steamapps\\common\\ELDEN RING\\Game\\eldenring.exe", "ELDEN RING™", false).unwrap();
        assert_eq!((g.key.as_str(), g.name.as_str(), g.dir.as_str()), ("exe:eldenring", "ELDEN RING", "d:\\steam\\steamapps\\common\\elden ring"));
        assert!(g.entry.is_some(), "and the catalogue's extras come with it");
        let h = identify_with(&dirs(), "C:\\Games\\Hades\\x64\\Hades.exe", "Hades", true).unwrap();
        assert_eq!((h.key.as_str(), h.name.as_str(), h.entry), ("exe:hades", "Hades", None));
    }

    #[test]
    fn the_same_game_from_another_store_is_the_same_game() {
        // a GOG / Game Pass copy: not in a Steam or Epic folder, found by its exe, even windowed
        let g = identify_with(&dirs(), "C:\\GOG Games\\ELDEN RING\\Game\\eldenring.exe", "ELDEN RING", false).unwrap();
        assert_eq!((g.key.as_str(), g.name.as_str()), ("exe:eldenring", "ELDEN RING"));
        assert_eq!(g.entry.map(|e| e.steam), Some(1245620), "the catalogue's extras too");
        let w = identify_with(&dirs(), "C:\\XboxGames\\Black Myth Wukong\\Content\\b1\\Binaries\\WinGDK\\b1-WinGDK-Shipping.exe", "Black Myth: Wukong", true).unwrap();
        assert_eq!(w.key, "exe:b1");
    }

    #[test]
    fn any_fullscreen_program_counts_but_a_windowed_app_does_not() {
        let g = identify_with(&dirs(), "E:\\Itch\\Some Game\\SomeGame.exe", "Some Game", true).unwrap();
        assert_eq!((g.key.as_str(), g.name.as_str()), ("exe:somegame", "Some Game"));
        assert!(identify_with(&dirs(), "C:\\Program Files\\App\\app.exe", "Notes", false).is_none());
    }

    #[test]
    fn games_that_share_an_exe_name_are_told_apart_by_their_folder() {
        let a = identify_with(&dirs(), "E:\\Itch\\To the Moon\\Game.exe", "To the Moon", true).unwrap();
        let b = identify_with(&dirs(), "E:\\Itch\\OFF\\Game.exe", "OFF", true).unwrap();
        assert_eq!((a.key.as_str(), b.key.as_str()), ("exe:tothemoon-game", "exe:off-game"));
        assert_eq!(exe_key("E:\\Games\\Hollow\\Binaries\\Win64\\launcher.exe", "launcher.exe"), "exe:hollow-launcher", "build folders are skipped");
        assert_eq!(exe_key("E:\\Itch\\Celeste\\Celeste.exe", "Celeste.exe"), "exe:celeste");
    }

    #[test]
    fn browsers_and_video_players_are_not_games() {
        assert!(not_a_game("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"));
        assert!(not_a_game("C:\\Program Files\\VideoLAN\\VLC\\vlc.exe"));
        assert!(!not_a_game("E:\\Itch\\Some Game\\SomeGame.exe"));
    }

    #[test]
    fn an_emulated_game_is_known_by_the_emulator_and_its_title() {
        let g = identify_with(&dirs(), "C:\\Emu\\shadPS4\\shadPS4.exe", "shadPS4 v0.9 | Bloodborne | CUSA00900", false).unwrap();
        assert_eq!((g.key.as_str(), g.name.as_str()), ("emu:bloodborne", "Bloodborne"));
    }
}
