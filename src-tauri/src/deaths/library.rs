//! The shared death-screen library: death screens people taught Deck, collected in the public
//! repo (`data/death-screens/library.json`) so a game only has to be taught once, by anyone.
//!
//! Every Deck build carries a copy (`EMBEDDED`). While the death counter is on and
//! `community_library` is on, Deck also looks for a newer copy on GitHub at most once a day (one
//! plain HTTPS GET of a public file, nothing sent but the request itself) and keeps it in
//! `deaths\library.json`. A copy is only used when it is newer, small, and every print in it
//! passes the same checks as one you taught yourself. Library prints are matched exactly like
//! your own (`teach::Taught`), never edited, and never written to your own taught list.
//!
//! Contributing: the page copies your share code and opens a prefilled issue; a workflow in the
//! repo turns it into a pull request that the maintainer reviews (`tools/deaths-library`).

use super::screen::teach::{Print, Taught, MAX_PER_GAME};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const URL: &str = "https://raw.githubusercontent.com/MohammadFakih02/deck/main/data/death-screens/library.json";
pub const ISSUE_URL: &str = "https://github.com/MohammadFakih02/deck/issues/new?template=death_screen.yml";
static EMBEDDED: &str = include_str!("../../../data/death-screens/library.json");

/// refuse anything bigger than this (the real file is far smaller)
const MAX_BYTES: usize = 3 * 1024 * 1024;
const MAX_GAMES: usize = 5000;
const REFRESH_EVERY_SECS: u64 = 24 * 3600;
/// retry after a failed look this soon
const RETRY: Duration = Duration::from_secs(3600);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Game {
    pub name: String,
    pub prints: Vec<Print>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Library {
    /// bumped on every change to the file; a copy is only replaced by a higher one
    pub version: u32,
    pub games: BTreeMap<String, Game>,
}

/// A game key the library accepts (the same on every PC).
pub fn good_key(k: &str) -> bool {
    let ok = |rest: &str| !rest.is_empty() && rest.len() <= 64 && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    match k.split_once(':') {
        // "exe:game" is every RPG Maker game: not the same game on every PC
        Some(("exe", r)) => ok(r) && !super::ident::generic_exe(r),
        Some(("emu", r)) => ok(r),
        // no store numbers ("steam:…", "epic:…"): a game is its program
        _ => false,
    }
}

/// A game name that is safe to show and store: letters, digits and a few marks, at most 100
/// characters (no @mentions, brackets or markup). None when nothing is left.
pub fn clean_name(name: &str) -> Option<String> {
    let n: String = name.chars().filter(|c| c.is_alphanumeric() || " .:'&()+-_".contains(*c)).take(100).collect();
    let n = n.trim().to_string();
    (!n.is_empty()).then_some(n)
}

/// Parses a library file and keeps only what passes the checks; None when it isn't one. A bad
/// game or print is skipped, not fatal: one odd entry (a number too big for its field, say) must
/// not turn every Deck's library into an empty one.
pub fn parse(text: &str) -> Option<Library> {
    if text.len() > MAX_BYTES {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let games = v["games"].as_object()?;
    if games.len() > MAX_GAMES {
        return None;
    }
    let mut lib = Library { version: v["version"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0), games: BTreeMap::new() };
    for (k, g) in games {
        if !good_key(k) {
            continue;
        }
        let prints: Vec<Print> = g["prints"].as_array().map(|a| a.iter().filter_map(|p| serde_json::from_value(p.clone()).ok()).collect()).unwrap_or_default();
        lib.games.insert(k.clone(), Game { name: clean_name(g["name"].as_str().unwrap_or(k)).unwrap_or_else(|| k.clone()), prints });
    }
    for g in lib.games.values_mut() {
        g.prints.retain(|p| Taught::new(p).is_some());
        let mut seen: Vec<(String, String)> = Vec::new();
        g.prints.retain(|p| {
            let id = (p.mask.clone(), p.values.clone());
            let new = !seen.contains(&id);
            seen.push(id);
            new
        });
        g.prints.truncate(MAX_PER_GAME * 2);
    }
    lib.games.retain(|_, g| !g.prints.is_empty());
    Some(lib)
}

static ACTIVE: RwLock<Option<Arc<Library>>> = RwLock::new(None);
static ENABLED: AtomicBool = AtomicBool::new(true);
static STATE: Mutex<Refresh> = Mutex::new(Refresh { next: None, busy: false });

struct Refresh {
    next: Option<Instant>,
    busy: bool,
}

fn cache_path() -> std::path::PathBuf {
    super::store::dir().join("library.json")
}

fn meta_path() -> std::path::PathBuf {
    super::store::dir().join("library.meta.json")
}

fn embedded() -> Library {
    parse(EMBEDDED).unwrap_or_default()
}

fn current() -> Arc<Library> {
    if let Some(l) = ACTIVE.read().as_ref() {
        return l.clone();
    }
    let mut lib = embedded();
    if let Some(cached) = std::fs::read_to_string(cache_path()).ok().and_then(|t| parse(&t)) {
        // the copy from GitHub wins a tie: it is main as it was when fetched
        if cached.version >= lib.version {
            lib = cached;
        }
    }
    let lib = Arc::new(lib);
    *ACTIVE.write() = Some(lib.clone());
    lib
}

/// The library's death screens for `game` (empty when the library is off).
pub fn prints(game: &str) -> Vec<Print> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    current().games.get(game).map(|g| g.prints.clone()).unwrap_or_default()
}

/// (games, death screens) in the library, for the page.
pub fn totals() -> (usize, usize) {
    let l = current();
    (l.games.len(), l.games.values().map(|g| g.prints.len()).sum())
}

pub fn count(game: &str) -> usize {
    current().games.get(game).map_or(0, |g| g.prints.len())
}

/// The setting changed; true when that changes what the watchers should match.
pub fn set_enabled(on: bool) -> bool {
    ENABLED.swap(on, Ordering::Relaxed) != on
}

/// Adopts a downloaded copy when it is valid and newer; true when it was. "Newer" includes the same
/// version with other content: two submissions merged from the same starting file both carry it, and
/// the second must still reach everyone (a lower version, a deliberate step back, is never taken).
pub fn adopt(text: &str) -> bool {
    let Some(new) = parse(text) else { return false };
    let have = current();
    if new.version < have.version || (new.version == have.version && new == *have) {
        return false;
    }
    *ACTIVE.write() = Some(Arc::new(new));
    true
}

/// Looks for a newer copy on GitHub when one is due and the setting is on. Cheap to call often:
/// it returns at once unless it's time, and the download runs on its own thread.
pub fn maybe_refresh(on: bool) {
    if !on {
        return;
    }
    {
        let mut s = STATE.lock();
        let now = Instant::now();
        if s.busy || s.next.is_some_and(|n| now < n) {
            return;
        }
        s.busy = true;
        // the first look waits a little, so starting Deck stays quiet
        if s.next.is_none() {
            s.busy = false;
            s.next = Some(now + Duration::from_secs(60));
            return;
        }
    }
    let _ = std::thread::Builder::new().name("deaths-library".into()).spawn(|| {
        let r = fetch();
        let mut s = STATE.lock();
        s.busy = false;
        s.next = Some(Instant::now() + if r.is_ok() { Duration::from_secs(REFRESH_EVERY_SECS) } else { RETRY });
        match r {
            Ok(true) => {
                crate::log::info("death counter: a newer shared death-screen library was downloaded");
                super::screen::taught_changed();
            }
            Ok(false) => {}
            Err(e) => crate::log::warn(format!("death counter: shared library not refreshed: {e}")),
        }
    });
}

fn fetch() -> Result<bool, String> {
    use std::io::Read;
    let meta: serde_json::Value = std::fs::read_to_string(meta_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    // checked within the day already (a restart must not look again)
    let last = meta["fetched"].as_u64().unwrap_or(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if now.saturating_sub(last) < REFRESH_EVERY_SECS {
        return Ok(false);
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(30)).build();
    let mut req = agent.get(URL).set("User-Agent", "Deck");
    if let Some(tag) = meta["etag"].as_str() {
        req = req.set("If-None-Match", tag);
    }
    let save_meta = |etag: Option<String>| {
        let m = serde_json::json!({ "fetched": now, "etag": etag });
        let _ = std::fs::create_dir_all(super::store::dir());
        let _ = crate::util::write_atomic(&meta_path(), m.to_string().as_bytes());
    };
    let resp = match req.call() {
        // ureq only makes 4xx/5xx errors: "not modified" arrives as a response
        Ok(r) if r.status() == 304 => {
            save_meta(meta["etag"].as_str().map(String::from));
            return Ok(false);
        }
        Ok(r) => r,
        Err(e) => return Err(e.to_string()),
    };
    let etag = resp.header("ETag").map(String::from);
    let mut body = Vec::new();
    resp.into_reader().take(MAX_BYTES as u64 + 1).read_to_end(&mut body).map_err(|e| e.to_string())?;
    let text = String::from_utf8(body).map_err(|_| "not text".to_string())?;
    save_meta(etag);
    if !adopt(&text) {
        return Ok(false);
    }
    let _ = std::fs::create_dir_all(super::store::dir());
    crate::util::write_atomic(&cache_path(), text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print() -> Print {
        // 64x36 mask with the first 40 cells set, each with value 100 + i
        let mut bits = vec![0u8; 64 * 36 / 8];
        for i in 0..40 {
            bits[i / 8] |= 1 << (i % 8);
        }
        let hex = |b: &[u8]| b.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let values: Vec<u8> = (0..40).map(|i| (100 + i * 3) as u8).collect();
        Print { mask: hex(&bits), values: hex(&values), made: 1, aspect: 0 }
    }

    #[test]
    fn the_embedded_library_is_valid() {
        let l = embedded();
        assert!(l.version >= 1);
        assert!(l.games.keys().all(|k| good_key(k)));
        assert!(EMBEDDED.len() < MAX_BYTES);
    }

    #[test]
    fn only_good_games_and_prints_survive_parsing() {
        let good = print();
        let bad = Print { mask: "zz".into(), values: "00".into(), made: 0, aspect: 0 };
        let lib = Library {
            version: 3,
            games: BTreeMap::from([
                ("exe:sekiro".into(), Game { name: "Sekiro".into(), prints: vec![good.clone(), good.clone(), bad.clone()] }),
                ("../../etc/passwd".into(), Game { name: "x".into(), prints: vec![good.clone()] }),
                ("exe:game1x".into(), Game { name: "only bad".into(), prints: vec![bad] }),
            ]),
        };
        let out = parse(&serde_json::to_string(&lib).unwrap()).unwrap();
        assert_eq!(out.games.len(), 1, "a bad key and a game with nothing usable are dropped");
        assert_eq!(out.games["exe:sekiro"].prints, vec![good], "duplicates and invalid prints are dropped");
        assert!(parse("not json").is_none());
        assert!(parse(&" ".repeat(MAX_BYTES + 1)).is_none());
    }

    #[test]
    fn one_odd_entry_does_not_empty_everyones_library() {
        let p = print();
        let ok = serde_json::to_value(&p).unwrap();
        // a `made` too big for its number type, a non-text mask, a print that isn't an object
        let text = serde_json::json!({
            "version": 7,
            "games": {
                "exe:game1x": { "name": "Fine", "prints": [ok.clone()] },
                "exe:game2x": { "name": "Mixed", "prints": [ { "mask": p.mask, "values": p.values, "made": 1e30 }, { "mask": 5, "values": [] }, "x", ok.clone() ] },
                "exe:game3x": { "name": "Bad name @[x](y)", "prints": [ok.clone()] },
                "exe:game4x": "not an object"
            }
        })
        .to_string();
        let lib = parse(&text).expect("still a library");
        assert_eq!(lib.version, 7);
        assert_eq!(lib.games.keys().map(String::as_str).collect::<Vec<_>>(), ["exe:game1x", "exe:game2x", "exe:game3x"]);
        assert_eq!(lib.games["exe:game2x"].prints.len(), 1, "only the good print of the mixed game");
        assert!(!lib.games["exe:game3x"].name.contains(['@', '[']), "{}", lib.games["exe:game3x"].name);
        assert_eq!(clean_name(&"é".repeat(300)).map(|n| n.chars().count()), Some(100), "cut on a character, not a byte");
        assert_eq!(clean_name("@[]"), None);
    }

    #[test]
    fn keys_are_the_same_on_every_pc() {
        assert!(good_key("emu:bloodborne") && good_key("exe:somegame"));
        assert!(!good_key("steam:814380") && !good_key("epic:Min") && !good_key("exe:") && !good_key("exe:a b") && !good_key("file:x") && !good_key("exe:a/b"));
        assert!(!good_key("exe:game") && good_key("exe:tothemoon-game"), "a shared exe name only with its folder");
    }

    #[test]
    fn a_newer_copy_replaces_the_active_one_and_an_older_one_does_not() {
        let mk = |v: u32| serde_json::to_string(&Library { version: v, games: BTreeMap::from([("exe:game2x".into(), Game { name: "G".into(), prints: vec![print()] })]) }).unwrap();
        *ACTIVE.write() = Some(Arc::new(Library { version: 10, games: BTreeMap::new() }));
        assert!(!adopt(&mk(9)), "older");
        assert!(adopt(&mk(10)), "the same version with more in it: a second merge from the same base");
        assert!(!adopt(&mk(10)), "the same version and the same content");
        assert!(adopt(&mk(11)));
        assert_eq!(count("exe:game2x"), 1);
        assert!(!set_enabled(true));
        assert!(set_enabled(false));
        assert!(prints("exe:game2x").is_empty(), "off: nothing from the library is matched");
        set_enabled(true);
        *ACTIVE.write() = None;
    }
}
