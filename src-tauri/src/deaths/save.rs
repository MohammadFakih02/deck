//! Deaths read from a game's own save file (stage 2). A reader finds the save files and reads
//! each character's death count out of one; the tracker re-reads a file when it changes and turns
//! a rise in the count into deaths. Read only: Deck never writes a game's save.
//!
//! Readers (catalogue `save` value):
//! - `celeste`: `Saves\N.celeste` in the install folder, plain XML; `<TotalDeaths>` under
//!   `<SaveData>` (format: github.com/frissyn/celeste-saves).
//! - `ds1`: Dark Souls: Prepare to Die Edition, `Documents\NBGI\DarkSouls\*\DRAKS0005.sl2`, a BND4
//!   container with one entry per character; the name at +0x100 (UTF-16), deaths at +0x1F128
//!   (u32), from dsfp's datasheet (MIT).
//!
//! Neither is checked against a real save yet (`verified` stays false in the catalogue).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, PartialEq)]
pub struct Character {
    pub slot: usize,
    pub name: String,
    pub deaths: u32,
}

/// The save files a reader looks at, given the game's install folder ("" when unknown).
fn locate(reader: &str, install: &str) -> Vec<PathBuf> {
    match reader {
        "celeste" => {
            let mut dirs = Vec::new();
            if !install.is_empty() {
                dirs.push(Path::new(install).join("Saves"));
            }
            // other stores keep the saves under the user's local app data
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(Path::new(&l).join("Celeste").join("Saves"));
            }
            dirs.iter().flat_map(|d| (0..10).map(move |i| d.join(format!("{i}.celeste")))).filter(|p| p.is_file()).collect()
        }
        "ds1" => {
            let Some(docs) = dirs_documents() else { return Vec::new() };
            let root = docs.join("NBGI").join("DarkSouls");
            std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path().join("DRAKS0005.sl2")).filter(|p| p.is_file()).collect()
        }
        _ => Vec::new(),
    }
}

fn dirs_documents() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|p| Path::new(&p).join("Documents"))
}

/// Every character in one save file, or None when it isn't a save this reader knows.
pub fn read(reader: &str, bytes: &[u8]) -> Option<Vec<Character>> {
    match reader {
        "celeste" => celeste(bytes),
        "ds1" => ds1(bytes),
        _ => None,
    }
}

pub fn known(reader: &str) -> bool {
    matches!(reader, "celeste" | "ds1")
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let i = s.find(&open)? + open.len();
    let j = s[i..].find(&format!("</{name}>"))?;
    Some(&s[i..i + j])
}

fn celeste(bytes: &[u8]) -> Option<Vec<Character>> {
    let s = std::str::from_utf8(bytes).ok()?;
    let root = s.find("<SaveData")?;
    let s = &s[root..];
    // TotalDeaths sits directly under SaveData, before the per-area stats (which have their own)
    let deaths = tag(s, "TotalDeaths")?.trim().parse().ok()?;
    let name = tag(s, "Name").unwrap_or("Madeline").trim().to_string();
    Some(vec![Character { slot: 0, name, deaths }])
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// A BND4 container's entries as (data offset, size).
pub fn bnd4_entries(b: &[u8]) -> Option<Vec<(usize, usize)>> {
    if b.get(0..4)? != b"BND4" {
        return None;
    }
    let count = u32_at(b, 0x0C)? as usize;
    if count == 0 || count > 64 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let h = 0x40 + i * 0x20;
        let size = u32_at(b, h + 0x08)? as usize;
        let off = u32_at(b, h + 0x10)? as usize;
        if off.checked_add(size)? > b.len() {
            return None;
        }
        out.push((off, size));
    }
    Some(out)
}

const DS1_NAME: usize = 0x100;
const DS1_DEATHS: usize = 0x1F128;

fn ds1(b: &[u8]) -> Option<Vec<Character>> {
    let entries = bnd4_entries(b)?;
    let mut out = Vec::new();
    // the last entry is the menu's own data, not a character
    for (slot, (off, size)) in entries.iter().take(entries.len().saturating_sub(1)).enumerate() {
        if *size < DS1_DEATHS + 4 {
            continue;
        }
        let d = &b[*off..off + size];
        let name: Vec<u16> = d[DS1_NAME..DS1_NAME + 26].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
        if name.is_empty() {
            continue; // an empty slot
        }
        out.push(Character { slot, name: String::from_utf16_lossy(&name), deaths: u32_at(d, DS1_DEATHS)? });
    }
    Some(out)
}

/// More than this in one change isn't deaths: another save was loaded or restored.
const MAX_STEP: u32 = 20;

/// The biggest rise one save can bring. Celeste writes its save only now and then (a hard screen
/// between two saves can be 50 deaths), the Souls games every few seconds.
fn max_step(reader: &str) -> u32 {
    match reader {
        "celeste" => 500,
        _ => MAX_STEP,
    }
}

/// Deaths since the last read: the character whose count rose by at most `max`. A count that fell
/// or jumped, or a character that's new, sets a new baseline instead of counting.
pub fn deaths_between(before: &[Character], after: &[Character], max: u32) -> u32 {
    let mut n = 0;
    for a in after {
        if let Some(b) = before.iter().find(|b| b.slot == a.slot && b.name == a.name) {
            if a.deaths > b.deaths && a.deaths - b.deaths <= max {
                n += a.deaths - b.deaths;
            }
        }
    }
    n
}

const CHECK: Duration = Duration::from_secs(2);

struct File {
    path: PathBuf,
    modified: Option<SystemTime>,
    chars: Vec<Character>,
}

/// Watches one game's save files while it's in front.
pub struct Tracker {
    pub game: String,
    reader: String,
    install: String,
    files: Vec<File>,
    next: Instant,
    next_locate: Instant,
    /// the last read worked (for the page: "counting from the save file")
    pub healthy: bool,
    /// when a read last worked; a save caught mid-write for a moment isn't "not working"
    last_ok: Option<Instant>,
    /// the reader was checked against the real game (the catalogue's `verified`)
    pub trusted: bool,
    /// it has counted a death from the save this session
    counted: bool,
}

/// A save file that worked this recently is still the way this game is counted.
const GRACE: Duration = Duration::from_secs(15);

impl Tracker {
    pub fn new(game: &str, reader: &str, install: &str) -> Tracker {
        let now = Instant::now();
        Tracker { game: game.into(), reader: reader.into(), install: install.into(), files: Vec::new(), next: now, next_locate: now, healthy: false, last_ok: None, trusted: false, counted: false }
    }

    /// New deaths since the last call (checks at most every 2 s).
    pub fn poll(&mut self) -> u32 {
        let now = Instant::now();
        if now < self.next {
            return 0;
        }
        self.next = now + CHECK;
        if now >= self.next_locate {
            // a save made after the game started (a new slot) shows up within a minute
            self.next_locate = now + Duration::from_secs(60);
            for path in locate(&self.reader, &self.install) {
                if !self.files.iter().any(|f| f.path == path) {
                    let chars = std::fs::read(&path).ok().and_then(|b| read(&self.reader, &b)).unwrap_or_default();
                    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                    self.files.push(File { path, modified, chars });
                }
            }
        }
        let mut n = 0;
        let mut ok = !self.files.is_empty();
        for f in &mut self.files {
            let modified = std::fs::metadata(&f.path).and_then(|m| m.modified()).ok();
            if modified == f.modified {
                continue;
            }
            // std opens files shared, so the game can keep writing while Deck reads
            match std::fs::read(&f.path).ok().and_then(|b| read(&self.reader, &b)) {
                Some(chars) => {
                    n += deaths_between(&f.chars, &chars, max_step(&self.reader));
                    f.chars = chars;
                    f.modified = modified;
                }
                None => ok = false, // caught mid-write: try again next time
            }
        }
        self.healthy = ok;
        if ok {
            self.last_ok = Some(now);
        }
        self.counted |= n > 0;
        n
    }

    /// The save file is the way this game is counted: it works, and the reader was checked in the
    /// real game or has counted a death already.
    pub fn proven(&self) -> bool {
        self.working() && (self.trusted || self.counted)
    }

    /// The save file is counting this game: it read fine a moment ago. A screen watcher must not
    /// start (and count the same death twice) because one read hit a half-written file.
    pub fn working(&self) -> bool {
        self.last_ok.is_some_and(|t| t.elapsed() < GRACE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn celeste_reads_the_save_wide_total_not_an_area_count() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<SaveData xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <Version>1.4.0.0</Version>
  <Name>Madeline</Name>
  <Time>123456789</Time>
  <TotalDeaths>1337</TotalDeaths>
  <TotalStrawberries>12</TotalStrawberries>
  <Areas><AreaStats ID="1"><Modes><AreaModeStats Deaths="40" /></Modes></AreaStats></Areas>
</SaveData>"#;
        assert_eq!(read("celeste", xml.as_bytes()), Some(vec![Character { slot: 0, name: "Madeline".into(), deaths: 1337 }]));
        assert_eq!(read("celeste", b"<Settings></Settings>"), None);
    }

    /// A DS1-shaped container: `slots` characters (name, deaths) plus the menu entry.
    fn ds1_file(slots: &[(&str, u32)]) -> Vec<u8> {
        let n = slots.len() + 1;
        let size = DS1_DEATHS + 0x100;
        let first = 0x40 + n * 0x20;
        let mut b = vec![0u8; first + n * size];
        b[0..4].copy_from_slice(b"BND4");
        b[0x0C..0x10].copy_from_slice(&(n as u32).to_le_bytes());
        for i in 0..n {
            let h = 0x40 + i * 0x20;
            let off = first + i * size;
            b[h + 0x08..h + 0x0C].copy_from_slice(&(size as u32).to_le_bytes());
            b[h + 0x10..h + 0x14].copy_from_slice(&(off as u32).to_le_bytes());
            if let Some((name, deaths)) = slots.get(i) {
                for (k, c) in name.encode_utf16().enumerate() {
                    b[off + DS1_NAME + k * 2..off + DS1_NAME + k * 2 + 2].copy_from_slice(&c.to_le_bytes());
                }
                b[off + DS1_DEATHS..off + DS1_DEATHS + 4].copy_from_slice(&deaths.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn ds1_reads_every_character_and_skips_empty_slots() {
        let b = ds1_file(&[("Solaire", 3), ("", 0), ("Nick", 155)]);
        let c = read("ds1", &b).unwrap();
        assert_eq!(c, vec![Character { slot: 0, name: "Solaire".into(), deaths: 3 }, Character { slot: 2, name: "Nick".into(), deaths: 155 }]);
        assert_eq!(read("ds1", b"BND4 but too short"), None);
        assert_eq!(read("ds1", b"not a save"), None);
    }

    #[test]
    fn only_a_small_rise_on_the_same_character_counts() {
        let c = |slot, name: &str, deaths| Character { slot, name: name.into(), deaths };
        let m = MAX_STEP;
        assert_eq!(deaths_between(&[c(0, "A", 10)], &[c(0, "A", 11)], m), 1);
        assert_eq!(deaths_between(&[c(0, "A", 10), c(1, "B", 5)], &[c(0, "A", 10), c(1, "B", 7)], m), 2);
        assert_eq!(deaths_between(&[c(0, "A", 10)], &[c(0, "A", 9)], m), 0, "a restored backup");
        assert_eq!(deaths_between(&[c(0, "A", 10)], &[c(0, "A", 500)], m), 0, "another save copied in");
        assert_eq!(deaths_between(&[c(0, "A", 10)], &[c(0, "New", 11)], m), 0, "a new character in the slot");
        assert_eq!(deaths_between(&[], &[c(0, "A", 11)], m), 0, "the first read is the baseline");
        assert_eq!(deaths_between(&[c(0, "M", 10)], &[c(0, "M", 60)], max_step("celeste")), 50, "Celeste saves now and then: a hard screen at once");
    }

    #[test]
    fn the_tracker_counts_a_save_that_changes_on_disk() {
        let dir = std::env::temp_dir().join(format!("deck-save-test-{}", std::process::id()));
        let saves = dir.join("Saves");
        std::fs::create_dir_all(&saves).unwrap();
        let write = |n: u32| std::fs::write(saves.join("0.celeste"), format!("<SaveData><Name>M</Name><TotalDeaths>{n}</TotalDeaths></SaveData>")).unwrap();
        write(5);
        let mut t = Tracker::new("exe:celeste", "celeste", dir.to_str().unwrap());
        assert_eq!(t.poll(), 0, "the first look is the baseline");
        assert!(t.healthy);
        std::thread::sleep(Duration::from_millis(30));
        write(7);
        t.next = Instant::now();
        // file times can be coarse: make sure this read sees a new time
        t.files[0].modified = None;
        assert!(!t.proven(), "an unchecked reader that hasn't counted yet leaves the screen on");
        assert_eq!(t.poll(), 2);
        assert!(t.proven(), "once it has counted, it is the way this game is counted");
        // a read that catches the file half-written: not healthy for a moment, but still working
        std::fs::write(saves.join("0.celeste"), "<SaveData><Name>M</Name><TotalDe").unwrap();
        t.next = Instant::now();
        t.files[0].modified = None;
        assert_eq!(t.poll(), 0);
        assert!(!t.healthy && t.working(), "one bad read must not hand the game to the screen watcher");
        t.last_ok = Some(Instant::now() - GRACE * 2);
        assert!(!t.working(), "but a save that stays unreadable does");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
