//! Games Deck knows: extras for counting in them (their own death-screen words, a save reader,
//! the wording). Not a whitelist: every game can be counted; these just count better.
//! The list is `catalog.json`, built from Steam's Souls-like, Boss Rush, Precision Platformer,
//! Action Roguelike, Metroidvania and Hack and Slash tags plus hand-picked games; about a dozen
//! carry presets (death-screen words, a save reader), all still unchecked in the real games.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Entry {
    /// Steam app id, 0 for a game that isn't on Steam (emulated consoles)
    pub steam: u32,
    pub name: String,
    /// exe names (lower case, no path) for copies Deck can't place in a Steam library
    pub exe: Vec<String>,
    /// an emulator's window title contains this (with `exe` being the emulator)
    pub title: String,
    /// "boss" (fights have bosses), "run" (roguelikes), "level" (platformers): only wording
    pub kind: String,
    /// words on the game's own death screen, upper case ("YOU DIED")
    pub words: Vec<String>,
    /// a save reader Deck has for it ("eldenring", …), "" for none
    pub save: String,
    /// the words or reader were checked in the real game
    pub verified: bool,
}

pub fn all() -> &'static [Entry] {
    static C: OnceLock<Vec<Entry>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("catalog.json")).unwrap_or_default())
}

pub fn by_steam(id: u32) -> Option<&'static Entry> {
    (id != 0).then(|| all().iter().find(|e| e.steam == id)).flatten()
}

/// Lower case letters and digits only.
pub fn slug(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// What an exe is known by, the same in every store: the name without ".exe" and without the
/// build's suffix (Steam's "b1-Win64-Shipping.exe" and Game Pass' "b1-WinGDK-Shipping.exe" are
/// both "b1").
pub fn stem_key(file: &str) -> String {
    let name = file.rsplit(['\\', '/']).next().unwrap_or(file).to_lowercase();
    let mut stem = name.strip_suffix(".exe").unwrap_or(&name).to_string();
    for s in ["-win64-shipping", "-wingdk-shipping", "-winstore-shipping", "-shipping", "-wingdk", "-win64"] {
        if let Some(x) = stem.strip_suffix(s) {
            stem = x.to_string();
            break;
        }
    }
    slug(&stem)
}

/// A game Deck found by its exe (and, for an emulator, its window title), whichever store it came
/// from.
pub fn by_exe(exe: &str, title: &str) -> Option<&'static Entry> {
    let k = stem_key(exe);
    let title = title.to_lowercase();
    all().iter().find(|e| e.exe.iter().any(|x| stem_key(x) == k) && (e.title.is_empty() || title.contains(&e.title.to_lowercase())))
}

/// The entry for a game key ("exe:eldenring", "emu:bloodborne", an older "steam:1245620").
pub fn by_key(key: &str) -> Option<&'static Entry> {
    match key.split_once(':') {
        Some(("steam", n)) => n.parse().ok().and_then(by_steam),
        Some(("exe", s)) => all().iter().find(|e| e.title.is_empty() && e.exe.iter().any(|x| stem_key(x) == s)),
        Some(("emu", s)) => all().iter().find(|e| !e.title.is_empty() && slug(&e.name) == s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_loads_and_has_no_duplicates() {
        let c = all();
        assert!(c.len() >= 100, "only {} games", c.len());
        let mut ids: Vec<u32> = c.iter().map(|e| e.steam).filter(|i| *i != 0).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "a Steam id is listed twice");
        assert!(c.iter().all(|e| !e.name.is_empty() && ["boss", "run", "level"].contains(&e.kind.as_str())));
        assert!(c.iter().all(|e| e.save.is_empty() || super::super::save::known(&e.save)), "a save reader Deck doesn't have");
        assert_eq!(by_steam(504230).map(|e| e.save.as_str()), Some("celeste"));
        assert_eq!(by_steam(2680010).map(|e| e.words.clone()), Some(vec!["KHAZAN HAS FALLEN".to_string()]));
    }

    #[test]
    fn games_are_found_by_steam_id_and_by_emulator_title() {
        assert_eq!(by_steam(1245620).map(|e| e.name.as_str()), Some("ELDEN RING"));
        assert_eq!(by_exe("shadPS4.exe", "shadPS4 | Bloodborne CUSA00900").map(|e| e.name.as_str()), Some("Bloodborne"));
        assert!(by_exe("shadps4.exe", "shadPS4 | Some other game").is_none());
    }

    #[test]
    fn an_exe_is_known_the_same_in_every_store() {
        assert_eq!(stem_key("B1-Win64-Shipping.exe"), "b1");
        assert_eq!(stem_key("C:\\XboxGames\\Black Myth\\b1-WinGDK-Shipping.exe"), "b1");
        assert_eq!(stem_key("eldenring.exe"), "eldenring");
        assert_eq!(by_exe("b1-WinGDK-Shipping.exe", "").map(|e| e.name.as_str()), Some("Black Myth: Wukong"), "a Game Pass copy");
        assert_eq!(by_key("exe:eldenring").map(|e| e.steam), Some(1245620));
        assert_eq!(by_key("exe:celeste").map(|e| e.save.as_str()), Some("celeste"));
        assert_eq!(by_key("emu:bloodborne").map(|e| e.name.as_str()), Some("Bloodborne"));
        assert_eq!(by_key("steam:1245620").map(|e| e.name.as_str()), Some("ELDEN RING"));
        assert!(by_key("exe:notagame").is_none());
    }
}
