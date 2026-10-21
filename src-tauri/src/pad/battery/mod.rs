//! The universal controller battery. Several sources are asked every 2 s, the first useful
//! answer per controller wins, and the result is smoothed like a phone's battery:
//!
//! | # | Source | Covers |
//! |---|---|---|
//! | 1 | vendor HID protocols (`gamesir`) | dongles that hide the battery from Windows |
//! | 2 | Sony input reports (`sony`) | DualShock 4, DualSense (read passively) |
//! | 3 | Windows.Gaming.Input (`wgi`) | Xbox controllers and others Windows knows |
//! | 4 | Bluetooth battery property (`bt`) | Bluetooth pads that report it |
//! | 5 | XInput (`xinput`) | older Xbox-style pads, coarse levels only |
//!
//! Nothing here runs while the tray icons and the overlay are both off.

pub mod bt;
pub mod gamesir;
pub mod hid;
pub mod sony;
pub mod wgi;
pub mod xinput;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Reading {
    /// the level, when the source gives one (not while a GameSir charges)
    pub percent: Option<u8>,
    /// a coarse level ("empty", "low", "medium", "full") from sources without a percent
    pub level: Option<&'static str>,
    pub charging: bool,
}

impl Reading {
    /// Low or empty, on battery: drives the red colour and the warning.
    pub fn low(&self) -> bool {
        !self.charging && (self.percent.is_some_and(|p| p <= 15) || matches!(self.level, Some("low" | "empty")))
    }
    /// 0.0-1.0 for drawing a fill.
    pub fn fill(&self) -> f32 {
        match (self.percent, self.level) {
            (Some(p), _) => p as f32 / 100.0,
            (None, Some("empty")) => 0.05,
            (None, Some("low")) => 0.25,
            (None, Some("medium")) => 0.6,
            _ => 1.0,
        }
    }
    /// "64%", "charging · 64%", "charging", "battery low".
    pub fn text(&self) -> String {
        match (self.percent, self.charging) {
            (Some(p), true) => format!("charging · {p}%"),
            (Some(p), false) => format!("{p}%"),
            (None, true) => "charging".into(),
            (None, false) => format!("battery {}", self.level.unwrap_or("unknown")),
        }
    }
}

/// One controller with a battery reading.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    /// stable per controller: the MAC, the HID path or Windows' id, so two identical pads stay two
    pub id: String,
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub source: &'static str,
    pub reading: Reading,
    /// the HID interface it was read from (lower case), when there is one
    pub path: String,
    /// made by a tool like DS4Windows (ViGEm): never gets its own icon or battery
    pub virtual_pad: bool,
}

/// Rank of a source: lower wins.
fn rank(source: &str) -> u8 {
    match source {
        "GameSir" => 1,
        "PlayStation" => 2,
        "Windows" => 3,
        "Bluetooth" => 4,
        _ => 5,
    }
}

/// The real name of a controller Deck knows by its VID/PID, for when SDL only has a generic
/// one ("XInput Controller #1" for a GameSir dongle).
pub fn known_name(vid: u16, pid: u16) -> Option<&'static str> {
    if (vid, pid) == (gamesir::VID, gamesir::PID) {
        Some(gamesir::NAME)
    } else if sony::is_sony_pad(vid, pid) {
        Some(sony::name(pid))
    } else {
        None
    }
}

/// One entry per controller: virtual pads dropped, a lower source dropped when a higher one
/// already covers a controller with the same VID/PID, XInput only when Windows.Gaming.Input
/// found nothing (it sees the same pads with a real percent).
pub fn merge(mut all: Vec<Entry>) -> Vec<Entry> {
    all.retain(|e| !e.virtual_pad);
    all.sort_by_key(|e| rank(e.source));
    let wgi_found = all.iter().any(|e| e.source == "Windows");
    let mut out: Vec<Entry> = Vec::new();
    for e in all {
        if out.iter().any(|o| o.id == e.id) {
            continue;
        }
        if e.source == "XInput" && wgi_found {
            continue;
        }
        let covered = e.vid != 0 && out.iter().any(|o| rank(o.source) < rank(e.source) && (o.vid, o.pid) == (e.vid, e.pid));
        if !covered {
            out.push(e);
        }
    }
    out
}

/// Phone-style smoothing. The level most pads report comes from the battery voltage, which sags
/// while you play and recovers at rest, so turning a pad off and on reads 10-15 points higher
/// than a minute before. On battery the shown level never rises: the lowest since the last charge
/// is kept (on disk, across power cycles and restarts) until it charges, or jumps so far that it
/// must have been charged elsewhere, or until the pad has been away for `AWAY_MS` (it may have
/// been charged while off, which Deck can't see, or rested and recovered for real).
#[derive(Default)]
pub struct Floors {
    pub map: HashMap<String, Floor>,
    /// a floor changed: save now
    pub dirty: bool,
    /// a pad was seen since the last save: save its last-seen time within `SEEN_SAVE_MS`
    touched: bool,
    saved_at: u64,
}

/// The lowest level since the last charge, and when the pad last reported (ms since 1970).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub pct: u8,
    pub seen: u64,
}

pub const RECHARGED: u8 = 20;
/// Away this long and the floor is forgotten.
pub const AWAY_MS: u64 = 60 * 60_000;
/// How often last-seen times reach the disk while nothing else changes.
pub const SEEN_SAVE_MS: u64 = 5 * 60_000;

impl Floors {
    pub fn steady(&mut self, id: &str, r: Reading, now: u64) -> Reading {
        let Some(p) = r.percent.filter(|_| !r.charging) else {
            if self.map.remove(id).is_some() {
                self.dirty = true;
            }
            return r;
        };
        let old = self.map.get(id).filter(|f| now.saturating_sub(f.seen) <= AWAY_MS).map(|f| f.pct);
        let pct = match old {
            Some(f) if p >= f && p - f < RECHARGED => f,
            _ => p,
        };
        if old != Some(pct) {
            self.dirty = true;
        }
        self.touched = true;
        self.map.insert(id.to_string(), Floor { pct, seen: now });
        Reading { percent: Some(pct), ..r }
    }

    /// Whether `save_floors` should write now.
    pub fn due(&self, now: u64) -> bool {
        self.dirty || (self.touched && now.saturating_sub(self.saved_at) >= SEEN_SAVE_MS)
    }

    pub fn saved(&mut self, now: u64) {
        self.dirty = false;
        self.touched = false;
        self.saved_at = now;
    }
}

/// A controller counts as off after this many silent polls in a row (one dropped reply doesn't hide it).
pub const MISSES: u32 = 2;

#[derive(Default)]
pub struct Misses {
    last: HashMap<String, (Entry, u32)>,
}

impl Misses {
    /// What to show after a poll that found `polled`: those, plus anything missing only once.
    pub fn update(&mut self, polled: Vec<Entry>) -> Vec<Entry> {
        let ids: HashSet<String> = polled.iter().map(|e| e.id.clone()).collect();
        for (_, (_, miss)) in self.last.iter_mut().filter(|(id, _)| !ids.contains(*id)) {
            *miss += 1;
        }
        self.last.retain(|_, (_, miss)| *miss < MISSES);
        for e in polled {
            self.last.insert(e.id.clone(), (e, 0));
        }
        let mut out: Vec<Entry> = self.last.values().map(|(e, _)| e.clone()).collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

fn floors_path() -> PathBuf {
    crate::settings::Settings::dir().join("pad-battery.json")
}

/// The saved floors; a bare number (the format before last-seen times) counts as seen long ago.
pub fn parse_floors(b: &[u8]) -> Floors {
    let map = serde_json::from_slice::<HashMap<String, serde_json::Value>>(b)
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| {
                    let f = match v.as_u64() {
                        Some(n) => Floor { pct: u8::try_from(n).ok()?, seen: 0 },
                        None => serde_json::from_value::<Floor>(v).ok()?,
                    };
                    (f.pct <= 100).then_some((k, f))
                })
                .collect()
        })
        .unwrap_or_default();
    Floors { map, ..Floors::default() }
}

pub fn load_floors() -> Floors {
    std::fs::read(floors_path()).map(|b| parse_floors(&b)).unwrap_or_default()
}

pub fn save_floors(f: &mut Floors) {
    let now = crate::util::now_ms();
    if f.due(now) {
        f.saved(now);
        if let Ok(b) = serde_json::to_vec(&f.map) {
            if let Err(e) = crate::util::write_atomic(&floors_path(), &b) {
                crate::log::warn(format!("controller battery: {e}"));
            }
        }
    }
}

/// The battery thread: polls while `wanted()`, publishes through `publish` when anything changed.
pub fn run(wanted: impl Fn() -> bool, mut publish: impl FnMut(Vec<Entry>)) {
    crate::util::com();
    let mut gs = gamesir::GameSir::default();
    let mut sony = sony::Sony::default();
    let mut misses = Misses::default();
    let mut floors = load_floors();
    let mut last: Vec<Entry> = Vec::new();
    loop {
        if !wanted() {
            // close every device and forget what was seen
            gs = gamesir::GameSir::default();
            sony = sony::Sony::default();
            misses = Misses::default();
            if !last.is_empty() {
                last.clear();
                publish(Vec::new());
            }
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let ifaces = hid::list();
        let mut all = gs.poll(&ifaces);
        all.extend(sony.poll(&ifaces));
        all.extend(wgi::poll());
        all.extend(bt::poll(&ifaces));
        all.extend(xinput::poll());
        let mut polled = merge(all);
        let now = crate::util::now_ms();
        for e in &mut polled {
            e.reading = floors.steady(&e.id, e.reading, now);
        }
        save_floors(&mut floors);
        let shown = misses.update(polled);
        if shown != last {
            last = shown.clone();
            publish(shown);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Starts the battery thread once.
pub fn start(wanted: impl Fn() -> bool + Send + 'static, publish: impl FnMut(Vec<Entry>) + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name("pad-battery".into()).spawn(move || run(wanted, publish)) {
        crate::log::error(format!("controller battery: its thread couldn't be started: {e}"));
    }
}

/// The low-battery warning: once per discharge, when a controller first reads low.
#[derive(Default)]
pub struct LowWarn {
    warned: HashSet<String>,
}

impl LowWarn {
    /// The entries that just went low.
    pub fn check<'a>(&mut self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        let mut out = Vec::new();
        for e in entries {
            if e.reading.low() {
                if self.warned.insert(e.id.clone()) {
                    out.push(e);
                }
            } else if e.reading.charging || e.reading.percent.is_some_and(|p| p >= 20) || e.reading.level.is_some_and(|l| l != "low" && l != "empty") {
                self.warned.remove(&e.id);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(p: u8) -> Reading {
        Reading { percent: Some(p), level: None, charging: false }
    }
    fn entry(id: &str, source: &'static str, vid: u16, pid: u16) -> Entry {
        Entry { id: id.into(), name: id.into(), vid, pid, source, reading: r(50), path: String::new(), virtual_pad: false }
    }

    #[test]
    fn known_names() {
        assert_eq!(known_name(0x3537, 0x1098), Some("GameSir Nova 2 Lite"));
        assert_eq!(known_name(0x054C, 0x0CE6), Some("DualSense"));
        assert_eq!(known_name(0x045E, 0x028E), None);
    }

    const T: u64 = 1_800_000_000_000;

    #[test]
    fn smoothing_never_rises_and_resets_at_20() {
        let mut f = Floors::default();
        assert_eq!(f.steady("a", r(60), T).percent, Some(60));
        assert_eq!(f.steady("a", r(73), T).percent, Some(60));
        assert_eq!(f.steady("a", r(55), T).percent, Some(55));
        assert_eq!(f.steady("a", r(74), T).percent, Some(55));
        assert_eq!(f.steady("a", r(75), T).percent, Some(75));
        assert!(f.dirty);
    }

    #[test]
    fn charging_clears_floor() {
        let mut f = Floors::default();
        f.steady("a", r(40), T);
        let c = f.steady("a", Reading { percent: Some(45), level: None, charging: true }, T);
        assert_eq!(c.percent, Some(45));
        assert!(!f.map.contains_key("a"));
        assert_eq!(f.steady("a", r(48), T).percent, Some(48));
    }

    #[test]
    fn floor_is_forgotten_after_the_pad_was_away() {
        // stuck at 57 for a day: charged while off (Deck never saw it charging), back at 64
        let mut f = Floors::default();
        assert_eq!(f.steady("a", r(57), T).percent, Some(57));
        assert_eq!(f.steady("a", r(64), T + 10 * 60_000).percent, Some(57), "a quick power cycle still rebounds");
        assert_eq!(f.steady("a", r(64), T + 10 * 60_000 + AWAY_MS + 1).percent, Some(64));
    }

    #[test]
    fn seen_keeps_the_floor_alive_while_playing() {
        let mut f = Floors::default();
        f.steady("a", r(57), T);
        let mut t = T;
        while t < T + 3 * AWAY_MS {
            t += 2_000;
            assert_eq!(f.steady("a", r(64), t).percent, Some(57));
        }
    }

    #[test]
    fn floors_round_trip_and_old_numbers_are_stale() {
        let mut f = Floors::default();
        f.steady("a", r(57), T);
        let back = parse_floors(&serde_json::to_vec(&f.map).unwrap());
        assert_eq!(back.map.get("a"), Some(&Floor { pct: 57, seen: T }));
        // the format before last-seen times: a bare number, taken as seen long ago
        let mut old = parse_floors(br#"{"gamesir:4c4ac9f61901":57,"bad":300}"#);
        assert_eq!(old.map.len(), 1);
        assert_eq!(old.steady("gamesir:4c4ac9f61901", r(64), T).percent, Some(64));
    }

    #[test]
    fn last_seen_is_saved_now_and_then() {
        let mut f = Floors::default();
        f.steady("a", r(57), T);
        assert!(f.due(T));
        f.saved(T);
        f.steady("a", r(57), T + 2_000);
        assert!(!f.due(T + 2_000), "not every poll");
        f.steady("a", r(57), T + SEEN_SAVE_MS);
        assert!(f.due(T + SEEN_SAVE_MS));
        f.saved(T + SEEN_SAVE_MS);
        assert!(!f.due(T + 2 * SEEN_SAVE_MS), "nothing seen since: nothing to save");
    }

    #[test]
    fn two_misses_hide() {
        let mut m = Misses::default();
        assert_eq!(m.update(vec![entry("a", "GameSir", 1, 1)]).len(), 1);
        assert_eq!(m.update(vec![]).len(), 1, "one dropped reply keeps it");
        assert_eq!(m.update(vec![]).len(), 0, "two in a row: off");
        assert_eq!(m.update(vec![entry("a", "GameSir", 1, 1)]).len(), 1);
    }

    #[test]
    fn two_pads_two_entries() {
        let out = merge(vec![entry("hid:a", "PlayStation", 0x54c, 0x9cc), entry("hid:b", "PlayStation", 0x54c, 0x9cc)]);
        assert_eq!(out.len(), 2);
        let mut m = Misses::default();
        assert_eq!(m.update(out).len(), 2);
    }

    #[test]
    fn higher_source_wins_per_controller() {
        let out = merge(vec![entry("wgi:x", "Windows", 0x54c, 0x9cc), entry("hid:a", "PlayStation", 0x54c, 0x9cc), entry("wgi:y", "Windows", 0x45e, 0xb13)]);
        let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["hid:a", "wgi:y"]);
    }

    #[test]
    fn virtual_pad_deduped() {
        let mut v = entry("hid:virt", "PlayStation", 0x54c, 0x5c4);
        v.virtual_pad = true;
        let out = merge(vec![v, entry("gamesir:1", "GameSir", 0x3537, 0x1098)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "gamesir:1");
    }

    #[test]
    fn xinput_only_without_wgi() {
        assert_eq!(merge(vec![entry("xinput:0", "XInput", 0, 0)]).len(), 1);
        assert_eq!(merge(vec![entry("xinput:0", "XInput", 0, 0), entry("wgi:y", "Windows", 0x45e, 0xb13)]).len(), 1);
    }

    #[test]
    fn low_warns_once_per_discharge() {
        let mut w = LowWarn::default();
        let mut e = entry("a", "GameSir", 1, 1);
        e.reading = r(15);
        assert_eq!(w.check(std::slice::from_ref(&e)).len(), 1);
        assert_eq!(w.check(std::slice::from_ref(&e)).len(), 0);
        e.reading = r(18);
        assert_eq!(w.check(std::slice::from_ref(&e)).len(), 0, "not re-armed below 20");
        e.reading = Reading { charging: true, ..r(30) };
        w.check(std::slice::from_ref(&e));
        e.reading = r(12);
        assert_eq!(w.check(std::slice::from_ref(&e)).len(), 1);
    }

    #[test]
    fn reading_text() {
        assert_eq!(r(64).text(), "64%");
        assert_eq!(Reading { percent: None, level: None, charging: true }.text(), "charging");
        assert_eq!(Reading { percent: Some(70), level: None, charging: true }.text(), "charging · 70%");
        assert!(Reading { percent: None, level: Some("low"), charging: false }.low());
    }
}
