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
    let t = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
        Err(e) => {
            crate::log::warn(format!("deaths: {} couldn't be read: {e}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
            return vec![];
        }
    };
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    let events: Vec<Event> = lines.iter().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect();
    if events.len() < lines.len() {
        crate::log::warn(format!("deaths: {} lines of {} didn't parse and were skipped", lines.len() - events.len(), path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    }
    events
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

/// `kind`: "taught" (death screens) or "won" (victory screens).
fn prints_path(key: &str, kind: &str) -> PathBuf {
    dir().join(file_name(key).replace(".jsonl", &format!(".{kind}.json")))
}

/// Each game's taught screens once read (the page asks every refresh; only Deck writes them):
/// "kind:key" -> prints.
static TAUGHT: parking_lot::Mutex<Option<std::collections::HashMap<String, Vec<super::screen::teach::Print>>>> = parking_lot::Mutex::new(None);

/// The death screens taught for a game (`deaths\<key>.taught.json`).
pub fn load_taught(key: &str) -> Vec<super::screen::teach::Print> {
    load_prints(key, "taught")
}

/// The victory screens taught for a game (`deaths\<key>.won.json`).
pub fn load_won(key: &str) -> Vec<super::screen::teach::Print> {
    load_prints(key, "won")
}

fn load_prints(key: &str, kind: &str) -> Vec<super::screen::teach::Print> {
    let mut cache = TAUGHT.lock();
    let cache = cache.get_or_insert_with(Default::default);
    let ck = format!("{kind}:{key}");
    if let Some(p) = cache.get(&ck) {
        return p.clone();
    }
    let prints: Vec<super::screen::teach::Print> = std::fs::read_to_string(prints_path(key, kind)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    cache.insert(ck, prints.clone());
    prints
}

/// The picture a screen was taught on, small (`deaths\<key>.<kind>.<made>.png`): it shows which
/// screen each learned one is. Only the picture picked is kept.
fn shot_path(key: &str, kind: &str, made: u64) -> PathBuf {
    dir().join(file_name(key).replace(".jsonl", &format!(".{kind}.{made}.png")))
}

pub fn save_shot(key: &str, kind: &str, made: u64, png: &[u8]) {
    let _ = std::fs::create_dir_all(dir());
    if let Err(e) = crate::util::write_atomic(&shot_path(key, kind, made), png) {
        crate::log::warn(format!("deaths: the picture of a learned {kind} screen in {key} couldn't be saved: {e}"));
    }
}

pub fn load_shot(key: &str, kind: &str, made: u64) -> Option<Vec<u8>> {
    std::fs::read(shot_path(key, kind, made)).ok()
}

pub fn remove_shot(key: &str, kind: &str, made: u64) {
    let _ = std::fs::remove_file(shot_path(key, kind, made));
}

pub fn save_taught(key: &str, prints: &[super::screen::teach::Print]) -> Result<(), String> {
    save_prints(key, "taught", prints)
}

pub fn save_won(key: &str, prints: &[super::screen::teach::Print]) -> Result<(), String> {
    save_prints(key, "won", prints)
}

fn save_prints(key: &str, kind: &str, prints: &[super::screen::teach::Print]) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir());
    let p = prints_path(key, kind);
    // read again next time, whatever happens below
    if let Some(c) = TAUGHT.lock().as_mut() {
        c.remove(&format!("{kind}:{key}"));
    }
    if prints.is_empty() {
        let _ = std::fs::remove_file(&p);
        return Ok(());
    }
    crate::util::write_atomic(&p, serde_json::to_string_pretty(prints).unwrap_or_default().as_bytes()).map_err(|e| format!("Couldn't save it: {e}"))
}

/// The files a reset clears: each game's history and boss notes, and the game list. Learned
/// death screens and the library's files stay.
fn cleared_by_reset(name: &str) -> bool {
    name == "games.json" || name.ends_with(".notes.json") || name.ends_with(".jsonl")
}

/// Moves the history aside to `dir\reset-<stamp>` (a way back, by hand) and returns that folder.
pub fn reset_in(dir: &Path, stamp: u64) -> std::io::Result<PathBuf> {
    let to = dir.join(format!("reset-{stamp}"));
    std::fs::create_dir_all(&to)?;
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if e.path().is_file() && cleared_by_reset(&name) {
            std::fs::rename(e.path(), to.join(&name))?;
        }
    }
    Ok(to)
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
    fn reset_moves_the_history_aside_and_keeps_learned_screens() {
        let d = std::env::temp_dir().join(format!("deck-deaths-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in ["exe-bbq.jsonl", "exe-bbq.notes.json", "games.json", "exe-bbq.taught.json", "library.meta.json"] {
            std::fs::write(d.join(f), f).unwrap();
        }
        let to = reset_in(&d, 1234).unwrap();
        assert_eq!(to, d.join("reset-1234"));
        for f in ["exe-bbq.jsonl", "exe-bbq.notes.json", "games.json"] {
            assert!(!d.join(f).exists(), "{f} cleared");
            assert_eq!(std::fs::read_to_string(to.join(f)).unwrap(), f, "{f} kept aside");
        }
        assert!(d.join("exe-bbq.taught.json").exists(), "learned screens stay");
        assert!(d.join("library.meta.json").exists());
        let again = reset_in(&d, 5678).unwrap();
        assert!(std::fs::read_dir(&again).unwrap().next().is_none(), "nothing left to move");
        let _ = std::fs::remove_dir_all(d);
    }

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
