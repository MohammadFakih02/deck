//! Which game is in front, by its program alone (no store lists or numbers): anything that covers
//! its monitor, or a program the catalogue lists. The catalogue only adds extras on top.

use super::catalog::{self, Entry};
use parking_lot::Mutex;
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
    /// the exe's folder, for save readers
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

use catalog::slug;

/// Folders that are a library or a store's, not a game's: never a game's name.
const LIBRARY_DIRS: &[&str] = &["common", "steamapps", "steamlibrary", "steam", "games", "game", "programfiles", "programfilesx86", "xboxgames", "content", "epicgames", "goggames", "gog", "itch", "engine", "binaries", "users", "downloads", "desktop"];

/// What a game is called, from its program alone: its description ("ELDEN RING™"), else the folder
/// it is installed in ("The First Berserker - Khazan" for BBQ\Binaries\Win64\BBQ-Win64-Shipping.exe),
/// else the window title, else the exe's name.
fn game_name(exe_path: &str, file: &str, title: &str, describe: &dyn Fn(&str) -> Option<String>) -> String {
    let stem = catalog::stem_key(file);
    let plain = |s: &str| !s.trim().is_empty() && s.chars().count() <= 60 && !generic_exe(&slug(s));
    if let Some(d) = describe(exe_path).filter(|d| plain(d)) {
        return d.trim().to_string();
    }
    let folders: Vec<&str> = exe_path
        .rsplit(['\\', '/'])
        .skip(1)
        .take_while(|f| !f.ends_with(':'))
        .filter(|f| {
            let s = slug(f);
            !s.is_empty() && !BUILD_DIRS.contains(&s.as_str()) && !LIBRARY_DIRS.contains(&s.as_str()) && plain(f)
        })
        .collect();
    // a folder named like the exe ("BBQ" for BBQ-Win64-Shipping.exe) only when there's no other
    let folder = folders.iter().find(|f| slug(f) != stem).or(folders.first());
    if let Some(f) = folder {
        return f.trim().to_string();
    }
    if plain(title) {
        return title.trim().to_string();
    }
    crate::util::exe_stem(file)
}

/// Names a game from its program and its window title. The key is the exe (the same game in any
/// store); the catalogue adds its extras when it lists the exe. None: the catalogue doesn't know it
/// and `fullscreen` is false (an ordinary app in a window).
pub fn identify_with(exe_path: &str, title: &str, fullscreen: bool, describe: &dyn Fn(&str) -> Option<String>) -> Option<Game> {
    let file = exe_path.rsplit(['\\', '/']).next().unwrap_or(exe_path);
    let dir = exe_path[..exe_path.len() - file.len()].trim_end_matches(['\\', '/']).to_string();
    let entry = catalog::by_exe(file, title);
    if let Some(e) = entry.filter(|e| !e.title.is_empty()) {
        // an emulator: the game is in its window title
        return Some(Game { key: format!("emu:{}", slug(&e.name)), name: e.name.clone(), entry: Some(e), dir });
    }
    if entry.is_none() && !fullscreen {
        return None;
    }
    let name = entry.map_or_else(|| game_name(exe_path, file, title, describe), |e| e.name.clone());
    Some(Game { key: exe_key(exe_path, file), name, entry, dir })
}

/// The program's description, once per exe.
fn describe(exe_path: &str) -> Option<String> {
    static SEEN: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());
    let mut s = SEEN.lock();
    if let Some((_, d)) = s.iter().find(|(p, _)| p == exe_path) {
        return d.clone();
    }
    let d = crate::boost::file_description(exe_path);
    if s.len() >= 100 {
        s.remove(0);
    }
    s.push((exe_path.to_string(), d.clone()));
    d
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
        let game = identify_with(&path, &title, full, &describe)?;
        Some(Front { game, hwnd: w as isize, monitor: (m.left, m.top, m.right, m.bottom) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    fn id(path: &str, title: &str, full: bool) -> Option<Game> {
        identify_with(path, title, full, &none)
    }

    #[test]
    fn a_listed_game_is_known_by_its_exe_from_any_store() {
        for p in ["D:\\Steam\\steamapps\\common\\ELDEN RING\\Game\\eldenring.exe", "C:\\GOG Games\\ELDEN RING\\Game\\eldenring.exe"] {
            let g = id(p, "ELDEN RING™", false).unwrap();
            assert_eq!((g.key.as_str(), g.name.as_str()), ("exe:eldenring", "ELDEN RING"), "{p}");
            assert!(g.entry.is_some_and(|e| e.words == ["YOU DIED"]), "and the catalogue's extras come with it");
        }
        let w = id("C:\\XboxGames\\Black Myth Wukong\\Content\\b1\\Binaries\\WinGDK\\b1-WinGDK-Shipping.exe", "Black Myth: Wukong", true).unwrap();
        assert_eq!(w.key, "exe:b1");
    }

    #[test]
    fn an_unlisted_game_is_named_from_its_program() {
        // like Khazan outside Steam: no description, a window title of "KZ"; its folder names it
        let k = id("C:\\The First Berserker - Khazan\\FOO\\Binaries\\Win64\\FOO-Win64-Shipping.exe", "KZ", true).unwrap();
        assert_eq!((k.key.as_str(), k.name.as_str()), ("exe:foo", "The First Berserker - Khazan"));
        let d = identify_with("E:\\Games\\x\\Hades.exe", "Hades", true, &|_| Some("Hades II".into())).unwrap();
        assert_eq!(d.name, "Hades II", "the program's description first");
        let s = id("D:\\SteamLibrary\\steamapps\\common\\Hollow Knight\\hollow_knight.exe", "Hollow Knight", true).unwrap();
        assert_eq!(s.name, "Hollow Knight");
        let t = id("D:\\Games\\nk.exe", "Nine Sols", true).unwrap();
        assert_eq!(t.name, "Nine Sols", "the library folder isn't a name; the title is");
    }

    #[test]
    fn any_fullscreen_program_counts_but_a_windowed_app_does_not() {
        let g = id("E:\\Itch\\Some Game\\SomeGame.exe", "Some Game", true).unwrap();
        assert_eq!((g.key.as_str(), g.name.as_str()), ("exe:somegame", "Some Game"));
        assert!(id("C:\\Program Files\\App\\app.exe", "Notes", false).is_none());
    }

    #[test]
    fn games_that_share_an_exe_name_are_told_apart_by_their_folder() {
        let a = id("E:\\Itch\\To the Moon\\Game.exe", "To the Moon", true).unwrap();
        let b = id("E:\\Itch\\OFF\\Game.exe", "OFF", true).unwrap();
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
        let g = id("C:\\Emu\\shadPS4\\shadPS4.exe", "shadPS4 v0.9 | Bloodborne | CUSA00900", false).unwrap();
        assert_eq!((g.key.as_str(), g.name.as_str()), ("emu:bloodborne", "Bloodborne"));
    }
}
