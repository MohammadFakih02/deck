//! The death counter's files: one history per game in `deaths\<key>.jsonl` (one event per
//! line) and `deaths\games.json` (each game's name and when it was last counted). Every change
//! rewrites the file through `write_atomic`, so a crash leaves the old or the new file, never half.

use super::book::Event;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn dir() -> PathBuf {
    crate::settings::Settings::dir().join("deaths")
}

/// A game key ("exe:eldenring", "emu:bloodborne") as a file name.
pub fn file_name(key: &str) -> String {
    let mut s: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    s.truncate(120);
    format!("{s}.jsonl")
}

/// Reads a history; lines that don't parse (a hand edit gone wrong) are skipped, not fatal.
pub fn load_from(path: &Path) -> Vec<Event> {
    let Ok(t) = std::fs::read_to_string(path) else { return vec![] };
    t.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect()
}

pub fn save_to(path: &Path, events: &[Event]) -> std::io::Result<()> {
    let mut out = String::new();
    for e in events {
        out.push_str(&serde_json::to_string(e).unwrap_or_default());
        out.push('\n');
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    crate::util::write_atomic(path, out.as_bytes())
}

pub fn load(key: &str) -> Vec<Event> {
    load_from(&dir().join(file_name(key)))
}

pub fn save(key: &str, events: &[Event]) {
    if let Err(e) = save_to(&dir().join(file_name(key)), events) {
        crate::log::warn(format!("deaths: saving {key}: {e}"));
    }
}

fn notes_path(key: &str) -> PathBuf {
    dir().join(file_name(key).replace(".jsonl", ".notes.json"))
}

/// Notes per boss for a game (`deaths\<key>.notes.json`), by boss key.
pub fn load_notes(key: &str) -> BTreeMap<String, String> {
    std::fs::read_to_string(notes_path(key)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_notes(key: &str, notes: &BTreeMap<String, String>) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir());
    crate::util::write_atomic(&notes_path(key), serde_json::to_string_pretty(notes).unwrap_or_default().as_bytes()).map_err(|e| format!("Couldn't save the note: {e}"))
}

fn taught_path(key: &str) -> PathBuf {
    dir().join(file_name(key).replace(".jsonl", ".taught.json"))
}

/// Each game's taught death screens once read (the page asks every refresh; only Deck writes them).
static TAUGHT: parking_lot::Mutex<Option<std::collections::HashMap<String, Vec<super::screen::teach::Print>>>> = parking_lot::Mutex::new(None);

/// The death screens taught for a game (`deaths\<key>.taught.json`).
pub fn load_taught(key: &str) -> Vec<super::screen::teach::Print> {
    let mut cache = TAUGHT.lock();
    let cache = cache.get_or_insert_with(Default::default);
    if let Some(p) = cache.get(key) {
        return p.clone();
    }
    let prints: Vec<super::screen::teach::Print> = std::fs::read_to_string(taught_path(key)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    cache.insert(key.to_string(), prints.clone());
    prints
}

pub fn save_taught(key: &str, prints: &[super::screen::teach::Print]) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir());
    let p = taught_path(key);
    // read again next time, whatever happens below
    if let Some(c) = TAUGHT.lock().as_mut() {
        c.remove(key);
    }
    if prints.is_empty() {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    crate::util::write_atomic(&p, serde_json::to_string_pretty(prints).unwrap_or_default().as_bytes()).map_err(|e| format!("Couldn't save it: {e}"))
}

/// What Deck remembers about each counted game.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct GameInfo {
    pub name: String,
    /// unix ms of the last event
    pub last: u64,
}

pub fn load_games() -> BTreeMap<String, GameInfo> {
    std::fs::read_to_string(dir().join("games.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_games(g: &BTreeMap<String, GameInfo>) {
    let _ = std::fs::create_dir_all(dir());
    if let Err(e) = crate::util::write_atomic(&dir().join("games.json"), serde_json::to_string_pretty(g).unwrap_or_default().as_bytes()) {
        crate::log::warn(format!("deaths: saving games.json: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::super::book::Ev;
    use super::*;

    #[test]
    fn keys_make_safe_file_names() {
        assert_eq!(file_name("exe:eldenring"), "exe-eldenring.jsonl");
        assert_eq!(file_name("exe:..\\evil"), "exe-..-evil.jsonl");
    }

    #[test]
    fn a_history_survives_a_round_trip_and_bad_lines() {
        let d = std::env::temp_dir().join(format!("deck-deaths-test-{}", std::process::id()));
        let p = d.join("x.jsonl");
        let ev = vec![Event::new(1, Ev::Death), Event { boss: "A".into(), fight: "f1".into(), ..Event::new(2, Ev::FightStart) }];
        save_to(&p, &ev).unwrap();
        let mut t = std::fs::read_to_string(&p).unwrap();
        t.push_str("not json\n");
        std::fs::write(&p, t).unwrap();
        assert_eq!(load_from(&p), ev);
        let _ = std::fs::remove_dir_all(d);
    }
}
