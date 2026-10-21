//! Games Deck knows: extras for counting in them (their own death-screen words, a save reader,
//! the wording). Not a whitelist: every game can be counted; these just count better.
//! The list is `catalog.json`, kept by hand: each game by the exe names it runs as (in any store),
//! with its death-screen words, a save reader or the wording. A game is never looked up by a
//! store's number (user, 2026-10-20: "forgo all Steam ids, stick to the process").

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub name: String,
    /// exe names (lower case, no path): the game runs as one of these, from any store
    pub exe: Vec<String>,
    /// an emulator's window title contains this (with `exe` being the emulator)
    pub title: String,
    /// "boss" (fights have bosses), "run" (roguelikes), "level" (platformers): only wording
    pub kind: String,
    /// words on the game's own death screen, upper case ("YOU DIED")
    pub words: Vec<String>,
    /// words on its banner when a boss is beaten, upper case ("ENEMY FELLED")
    pub win: Vec<String>,
    /// where its boss bar is when that isn't centred at the top or the bottom (Sekiro: top left)
    pub bar: Option<super::screen::bar::Spot>,
    /// a save reader Deck has for it ("eldenring", …), "" for none
    pub save: String,
    /// the words or reader were checked in the real game
    pub verified: bool,
}

pub fn all() -> &'static [Entry] {
    static C: OnceLock<Vec<Entry>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("catalog.json")).unwrap_or_default())
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

/// The entry for a game key ("exe:eldenring", "emu:bloodborne").
pub fn by_key(key: &str) -> Option<&'static Entry> {
    match key.split_once(':') {
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
        assert!(c.len() >= 15, "only {} games", c.len());
        assert!(c.iter().all(|e| !e.name.is_empty() && !e.exe.is_empty() && ["boss", "run", "level"].contains(&e.kind.as_str())));
        assert!(c.iter().all(|e| e.save.is_empty() || super::super::save::known(&e.save)), "a save reader Deck doesn't have");
        // one game per exe (an emulator's are told apart by the window title)
        let mut keys: Vec<(String, &str)> = c.iter().flat_map(|e| e.exe.iter().map(|x| (stem_key(x), e.title.as_str()))).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "an exe is listed for two games");
        assert!(c.iter().all(|e| e.exe.iter().all(|x| !super::super::ident::generic_exe(&stem_key(x)) && !x.starts_with("start_protected"))), "an exe many games share");
    }

    #[test]
    fn games_are_found_by_exe_and_by_emulator_title() {
        assert_eq!(by_exe("eldenring.exe", "ELDEN RING").map(|e| e.name.as_str()), Some("ELDEN RING"));
        assert!(by_exe("start_protected_game.exe", "").is_none(), "Easy Anti-Cheat's launcher is many games");
        assert_eq!(by_exe("shadPS4.exe", "shadPS4 | Bloodborne CUSA00900").map(|e| e.name.as_str()), Some("Bloodborne"));
        assert!(by_exe("shadps4.exe", "shadPS4 | Some other game").is_none());
    }

    #[test]
    fn an_exe_is_known_the_same_in_every_store() {
        assert_eq!(stem_key("B1-Win64-Shipping.exe"), "b1");
        assert_eq!(stem_key("C:\\XboxGames\\Black Myth\\b1-WinGDK-Shipping.exe"), "b1");
        assert_eq!(stem_key("eldenring.exe"), "eldenring");
        assert_eq!(by_exe("b1-WinGDK-Shipping.exe", "").map(|e| e.name.as_str()), Some("Black Myth: Wukong"), "a Game Pass copy");
        assert_eq!(by_key("exe:eldenring").map(|e| e.name.as_str()), Some("ELDEN RING"));
        assert_eq!(by_key("exe:celeste").map(|e| e.save.as_str()), Some("celeste"));
        assert_eq!(by_key("emu:bloodborne").map(|e| e.name.as_str()), Some("Bloodborne"));
        assert!(by_key("steam:1245620").is_none(), "no store numbers");
        assert!(by_key("exe:notagame").is_none());
        let k = by_exe("BBQ-Win64-Shipping.exe", "KZ").expect("Khazan");
        assert_eq!((k.name.as_str(), k.words.clone()), ("The First Berserker: Khazan", vec!["KHAZAN HAS FALLEN".to_string()]));
    }
}
