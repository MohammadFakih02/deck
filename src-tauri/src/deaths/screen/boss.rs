//! Boss fights from the screen: when a boss's health bar comes up (`bar.rs`), its name is read
//! and its fight opens at once; a death ends the try; the bar emptying and going away, or a
//! victory banner, is a win. Pure decisions: frames come in as what `bar.rs` saw, names as text
//! the OCR read.
//!
//! Names: the first readable read opens the fight; a read like a boss already fought in this game
//! takes that boss's spelling. A few more reads follow while the fight goes on, and when most of
//! them agree on another spelling the fight is renamed (the OCR misreads stylised letters).
//!
//! Bosses with phases: a try lasts until a death or a win, through cutscenes (the bar gone for up
//! to PHASE_GAP), other names (the second phase's, a duo's other half) and other bars (it
//! follows whichever boss bar is up). Each new bar in a try is the next phase when its name
//! differs or the last one had emptied; a bar that empties and fills up again is one too. A bar
//! that emptied and went is a win after WIN_MS; a bar coming back within PHASE_GAP takes it back.

use super::bar::{self, Bar};
use super::capture::Frame;
use crate::deaths::book::{like_boss, same_boss};

#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// a boss bar has been up since `since` with `name` on it ("" unread). Phase 1: a try starts;
    /// 2 and up: the same try goes on with another bar (the fight's next phase or its other boss).
    Engage { since: u64, name: String, phase: u8 },
    /// more reads of the try's (`since`) first bar agree on another spelling of its name
    Rename { since: u64, name: String },
    /// the bar went away a while ago with no death: out of the arena (or a long cutscene)
    Leave,
    /// beaten: the bar emptied and went, or a victory banner was read
    Won { last_seen: u64 },
}

/// frames in a row a bar must be in the same place before it counts
const STREAK: u32 = 3;
const READ_EVERY_MS: u64 = 400;
/// reads of a name before giving up on it
const MAX_READS: u32 = 6;
/// reads after the fight opened, to vote on its name
const VOTES: u8 = 4;
/// A bar with no name to read (a HUD or loading bar that flickers in and out, an enemy's bar
/// that moves with it) makes naming quiet outside a fight: the next bar anywhere is read only
/// after a wait, doubling each time a read fails up to QUIET_MAX_MS, and only QUIET_READS times.
/// The wait counts from the bar's first sighting, so a boss's fight still starts when its bar
/// showed up.
const QUIET_FIRST_MS: u64 = 2_000;
const QUIET_MAX_MS: u64 = 16_000;
const QUIET_READS: u32 = 2;
/// quiet naming is forgotten this long after its wait ended
const QUIET_FORGET_MS: u64 = 120_000;
/// frames without a bar that drop a bar not yet followed
const MISSES: u32 = 3;
/// the bar gone this long without a death: out of the fight (the clock stops)
pub const LEAVE_MS: u64 = 15_000;
/// a try lasts this long with no bar at all: a cutscene between phases, then the next bar
pub const PHASE_GAP_MS: u64 = 120_000;
/// an emptied bar gone this long with no death is a win (a bar coming back within PHASE_GAP
/// turns it back into a fight: the next phase)
pub const WIN_MS: u64 = 10_000;
/// a bar this empty (%) that goes away is a boss down
pub const LOW: u8 = 5;
/// a bar that was this low and fills up past FULL again is the next phase
const REFILL_FROM: u8 = 10;
const FULL: u8 = 80;

/// Victory banners: the bar can also go away with health left (a last hit takes a chunk).
pub const WIN_WORDS: [&str; 12] = [
    "ENEMY FELLED",
    "GREAT ENEMY FELLED",
    "DEMIGOD FELLED",
    "LEGEND FELLED",
    "GOD SLAIN",
    "VICTORY ACHIEVED",
    "PREY SLAUGHTERED",
    "NIGHTMARE SLAIN",
    "HEIR OF FIRE DESTROYED",
    "LORD OF CINDER FALLEN",
    "BOSS DEFEATED",
    "TARGET DEFEATED",
];

/// One frame as seen: the followed bar still there (and how full), or a bar found anywhere.
#[derive(Clone, Copy, Debug, Default)]
pub struct Look {
    pub tracked: bool,
    pub fill: u8,
    pub found: Option<(Bar, [u8; 3])>,
}

/// The try going on: from the first bar to a death or a win, through every phase.
#[derive(Clone, Debug)]
struct Run {
    since: u64,
    phase: u8,
    name: String,
    last_seen: u64,
    fill: u8,
    /// the bar got down to REFILL_FROM in this phase (a refill past FULL is the next phase)
    dipped: bool,
    left: bool,
    /// told as won (its bar emptied and went); a bar coming back is the next phase
    won: bool,
    /// reads of the first bar's name after the fight opened, and how many are still to come
    votes: Vec<String>,
    to_vote: u8,
    next_read: u64,
}

#[derive(Clone, Debug)]
enum S {
    Idle,
    Seeing { bar: Bar, colour: [u8; 3], first: u64, n: u32 },
    Naming { bar: Bar, colour: [u8; 3], since: u64, tries: u32, next: u64, missed: u32 },
    /// a bar with no name to read (and no try going on): left alone until it goes
    Nameless { bar: Bar, missed: u32 },
    Fighting { bar: Bar, colour: [u8; 3], gone: Option<u64> },
}

/// after a bar with no name to read: no bar is read again until `until`
#[derive(Clone, Copy, Debug)]
struct Quiet {
    until: u64,
    gap: u64,
}

pub struct BossWatch {
    s: S,
    run: Option<Run>,
    w: usize,
    quiet: Option<Quiet>,
}

impl Default for BossWatch {
    fn default() -> Self {
        BossWatch { s: S::Idle, run: None, w: 0, quiet: None }
    }
}

impl BossWatch {
    /// Quiet naming, while it is remembered and no fight is open (a fight's next bar is read
    /// at once).
    fn quiet_now(&self, now: u64) -> Option<Quiet> {
        self.quiet.filter(|q| self.run.is_none() && now < q.until + QUIET_FORGET_MS)
    }
}

/// The spelling the reads agree on, letter by letter. The read most like the others is the
/// frame (then the one with no digits read for letters, "0men", then the longer: a read can drop
/// the end), and each of its letters becomes the one most reads have there: "Ma1enia, Blade of
/// Miquella" and "Malenia, Blade of Miquel1a" make "Malenia, Blade of Miquella". A tie goes to a
/// letter over a digit, then to the frame's own.
fn vote(reads: &[String]) -> Option<String> {
    let like = |a: &str, b: &str| {
        let n = a.chars().count() + b.chars().count();
        (crate::deaths::book::common(a, b) * 2000).checked_div(n).unwrap_or(0)
    };
    let rank = |s: &String| {
        let agree = reads.iter().filter(|r| same_boss(r, s)).count();
        let near: usize = reads.iter().map(|r| like(r, s)).sum();
        (agree, near, std::cmp::Reverse(s.chars().filter(char::is_ascii_digit).count()), s.chars().count())
    };
    let frame: Vec<char> = reads.iter().max_by_key(|s| rank(s))?.chars().collect();
    let mut tally: Vec<Vec<(char, usize)>> = vec![Vec::new(); frame.len()];
    for r in reads {
        for (i, c) in aligned(&frame, &r.chars().collect::<Vec<_>>()) {
            match tally[i].iter_mut().find(|(k, _)| *k == c) {
                Some(t) => t.1 += 1,
                None => tally[i].push((c, 1)),
            }
        }
    }
    Some(frame.iter().zip(&tally).map(|(own, t)| t.iter().max_by_key(|(c, n)| (*n, c.is_alphabetic(), c == own)).map_or(*own, |(c, _)| *c)).collect())
}

/// Which letter of `read` lines up with each letter of `frame` (fewest edits between them;
/// letters one has and the other doesn't line up with nothing).
fn aligned(frame: &[char], read: &[char]) -> Vec<(usize, char)> {
    let (n, m) = (frame.len(), read.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, v) in d[0].iter_mut().enumerate() {
        *v = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub = d[i - 1][j - 1] + usize::from(frame[i - 1] != read[j - 1]);
            d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
        }
    }
    let (mut i, mut j, mut out) = (n, m, Vec::new());
    while i > 0 && j > 0 {
        if d[i][j] == d[i - 1][j - 1] + usize::from(frame[i - 1] != read[j - 1]) {
            out.push((i - 1, read[j - 1]));
            i -= 1;
            j -= 1;
        } else if d[i][j] == d[i - 1][j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out
}

impl BossWatch {
    /// What this frame shows: the followed bar, or (none followed, or it's gone) a boss bar at
    /// one of the game's `spots`, else a centred one.
    pub fn look(&mut self, f: &Frame, spots: &[bar::Spot]) -> Look {
        self.w = f.w;
        let mut l = Look::default();
        if let S::Fighting { bar, colour, .. } = &self.s {
            if bar::still_there(f, bar, *colour) {
                l.tracked = true;
                l.fill = bar::fill_pct(f, bar, *colour);
                return l;
            }
        }
        l.found = spots.iter().find_map(|s| bar::find_at(f, s)).or_else(|| bar::find(f)).map(|b| (b, bar::fill_colour(f, &b)));
        if let (S::Fighting { bar, colour, .. }, Some((b, _))) = (&self.s, l.found) {
            if b.same_place(bar, f.w) {
                // the same bar, its edges blurred for a frame: still followed
                return Look { tracked: true, fill: bar::fill_pct(f, bar, *colour), found: None };
            }
        }
        l
    }

    /// The frame's look at `now` (ms): what to record, if anything.
    pub fn step(&mut self, l: Look, now: u64) -> Option<Out> {
        let w = self.w.max(1);
        if let Some(r) = &mut self.run {
            if l.tracked || l.found.is_some() {
                r.last_seen = now;
            }
        }
        // no boss bar for PHASE_GAP: the try is over
        if !l.tracked && l.found.is_none() && self.run.as_ref().is_some_and(|r| now.saturating_sub(r.last_seen) >= PHASE_GAP_MS) {
            self.run = None;
            self.s = S::Idle;
            return None;
        }
        match &mut self.s {
            S::Fighting { gone, .. } => {
                if l.tracked {
                    *gone = None;
                    if let Some(r) = &mut self.run {
                        r.left = false;
                        if l.fill <= REFILL_FROM {
                            r.dipped = true;
                        } else if r.dipped && l.fill >= FULL {
                            r.phase = r.phase.saturating_add(1);
                            r.dipped = false;
                        }
                        r.fill = l.fill;
                    }
                    return None;
                }
                let gone_ms = now.saturating_sub(*gone.get_or_insert(now));
                if let Some((b, c)) = l.found {
                    // another boss bar while this one is gone: the duo's other half, the next phase
                    self.s = S::Seeing { bar: b, colour: c, first: now, n: 1 };
                } else if let Some(r) = self.run.as_mut().filter(|r| !r.won && r.fill <= LOW && gone_ms >= WIN_MS) {
                    // emptied and gone, nobody died: beaten (until a bar comes back)
                    r.won = true;
                    self.s = S::Idle;
                    return Some(Out::Won { last_seen: r.last_seen });
                } else if gone_ms >= LEAVE_MS {
                    self.s = S::Idle;
                    if let Some(r) = self.run.as_mut().filter(|r| !r.left && !r.won) {
                        r.left = true;
                        return Some(Out::Leave);
                    }
                }
                None
            }
            S::Idle => {
                if let Some((b, c)) = l.found {
                    self.s = S::Seeing { bar: b, colour: c, first: now, n: 1 };
                }
                None
            }
            S::Seeing { bar, colour, first, n } => {
                match l.found {
                    Some((b, _)) if b.same_place(bar, w) => {
                        *n += 1;
                        if *n >= STREAK {
                            let (bar, colour, first) = (*bar, *colour, *first);
                            match self.quiet_now(now) {
                                // after a bar with no name: waits, still seeing the bar
                                Some(q) if now < q.until => {}
                                q => {
                                    let tries = if q.is_some() { MAX_READS - QUIET_READS } else { 0 };
                                    self.s = S::Naming { bar, colour, since: first, tries, next: now, missed: 0 };
                                }
                            }
                        }
                    }
                    Some((b, c)) => self.s = S::Seeing { bar: b, colour: c, first: now, n: 1 },
                    None => self.s = S::Idle,
                }
                None
            }
            S::Naming { bar, missed, .. } | S::Nameless { bar, missed } => {
                match l.found {
                    Some((b, _)) if b.same_place(bar, w) => *missed = 0,
                    _ => {
                        *missed += 1;
                        if *missed >= MISSES {
                            self.s = S::Idle;
                        }
                    }
                }
                None
            }
        }
    }

    /// A bar whose name should be read now (the caller reads around it and calls `named`): a new
    /// bar, or the try's first bar while its name is still being voted on.
    pub fn wants_name(&self, now: u64) -> Option<Bar> {
        match (&self.s, &self.run) {
            (S::Naming { bar, next, .. }, _) if now >= *next => Some(*bar),
            (S::Fighting { bar, gone: None, .. }, Some(r)) if r.to_vote > 0 && now >= r.next_read => Some(*bar),
            _ => None,
        }
    }

    /// The name read on the bar `wants_name` gave (None: nothing readable). `known`: bosses
    /// already fought in this game; a read like one of them takes its spelling.
    pub fn named(&mut self, name: Option<String>, now: u64, known: &[String]) -> Option<Out> {
        let name = name.map(|n| known.iter().find(|k| same_boss(k, &n) || like_boss(k, &n)).cloned().map_or((n, false), |k| (k, true)));
        if let (S::Fighting { .. }, Some(r)) = (&self.s, &mut self.run) {
            // a vote on the name of the fight that just opened
            r.to_vote = r.to_vote.saturating_sub(1);
            r.next_read = now + READ_EVERY_MS;
            if let Some((n, _)) = name {
                r.votes.push(n);
            }
            if r.to_vote > 0 {
                return None;
            }
            let best = vote(&r.votes)?;
            if same_boss(&best, &r.name) {
                return None;
            }
            r.name = best.clone();
            return Some(Out::Rename { since: r.since, name: best });
        }
        let S::Naming { tries, next, .. } = &mut self.s else { return None };
        *tries += 1;
        *next = now + READ_EVERY_MS;
        match name {
            Some((n, sure)) => {
                self.quiet = None;
                self.engage(n, sure, now)
            }
            None if *tries >= MAX_READS => {
                if self.run.is_some() {
                    // the next phase's bar without a readable name: still the same try
                    return self.engage(String::new(), true, now);
                }
                let S::Naming { bar, .. } = self.s else { return None };
                self.s = S::Nameless { bar, missed: 0 };
                let gap = self.quiet_now(now).map_or(QUIET_FIRST_MS, |q| (q.gap * 2).min(QUIET_MAX_MS));
                self.quiet = Some(Quiet { until: now + gap, gap });
                None
            }
            None => None,
        }
    }

    /// The bar has a name (`sure`: a boss already fought): followed from now on.
    fn engage(&mut self, name: String, sure: bool, now: u64) -> Option<Out> {
        let S::Naming { bar, colour, since, .. } = self.s else { return None };
        self.s = S::Fighting { bar, colour, gone: None };
        let phase = match &mut self.run {
            Some(r) => {
                // a new bar in the same try: the next phase if it's another name or the last one emptied
                if (!name.is_empty() && !r.name.is_empty() && !same_boss(&r.name, &name)) || r.fill <= LOW {
                    r.phase = r.phase.saturating_add(1);
                }
                if !name.is_empty() {
                    r.name = name.clone();
                }
                r.fill = 100;
                r.dipped = false;
                r.left = false;
                r.won = false;
                r.last_seen = now;
                r.phase
            }
            None => {
                let to_vote = if sure { 0 } else { VOTES };
                self.run = Some(Run { since, phase: 1, name: name.clone(), last_seen: now, fill: 100, dipped: false, left: false, won: false, votes: vec![name.clone()], to_vote, next_read: now + READ_EVERY_MS });
                1
            }
        };
        Some(Out::Engage { since, name, phase })
    }

    /// A death was counted: the try is over.
    pub fn death(&mut self) {
        self.run = None;
        self.s = S::Idle;
    }

    /// A try is going on (a victory banner now is a win).
    pub fn in_try(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.won)
    }

    /// A victory banner was read.
    pub fn victory(&mut self) -> Option<Out> {
        let r = self.run.take()?;
        self.s = S::Idle;
        (!r.won).then_some(Out::Won { last_seen: r.last_seen })
    }

    /// The try going on: (when its bar was last seen, how full, which phase).
    pub fn now(&self) -> Option<(u64, u8, u8)> {
        self.run.as_ref().filter(|r| !r.won).map(|r| (r.last_seen, r.fill, r.phase))
    }
}

/// A boss's name from the lines read next to its bar (`above`: the bar is under them). The name
/// is the biggest writing there: lines much smaller than the biggest (a tip, a status, a damage
/// number) go, and of the rest the one nearest the bar is the name.
pub fn pick_name(lines: &[super::ocr::Line], above: bool) -> Option<String> {
    let named: Vec<(&super::ocr::Line, String)> = lines.iter().filter_map(|l| Some((l, clean_name(&l.text, above)?))).collect();
    let tallest = named.iter().map(|(l, _)| l.y1 - l.y0).fold(0f32, f32::max);
    let near = |l: &super::ocr::Line| if above { l.y1 } else { -l.y0 };
    named.into_iter().filter(|(l, _)| (l.y1 - l.y0) * 10.0 >= tallest * 6.0).max_by(|a, b| near(a.0).total_cmp(&near(b.0))).map(|(_, n)| n)
}

/// A boss's name from the text read next to its bar (`above`: the line nearest the bar is the
/// last). Damage numbers and stray marks are dropped; it needs three letters and at most seven
/// words (a loading screen's tip isn't a name).
pub fn clean_name(text: &str, above: bool) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let pick = |l: &&str| {
        // words with a letter in them ("0men" is a misread "Omen"; "1,203" is damage)
        let words: Vec<String> = l
            .split_whitespace()
            .filter(|w| w.chars().any(char::is_alphabetic))
            .map(|w| w.chars().filter(|c| c.is_alphanumeric() || matches!(c, '\'' | '’' | ',' | '-' | '.')).collect::<String>())
            .filter(|w| !w.is_empty())
            .collect();
        let s = words.join(" ");
        let s = plain_accent(s.trim_matches(|c: char| !c.is_alphanumeric()).to_string());
        let letters = s.chars().filter(|c| c.is_alphabetic()).count();
        (letters >= 3 && words.len() <= 7 && s.chars().count() <= 48 && !words.iter().any(|w| garbled(w)) && !hud_text(&s)).then_some(s)
    };
    if above {
        lines.iter().rev().find_map(pick)
    } else {
        lines.iter().find_map(pick)
    }
}

/// Words of a game's own messages, never in a boss's name ("Bot Drop Detected" over a
/// Helldivers 2 event's bar).
const HUD_WORDS: &[&str] = &["detected", "objective", "objectives", "mission", "loading", "incoming", "extraction", "reinforcements", "press", "completed", "failed"];

fn hud_text(s: &str) -> bool {
    s.split(|c: char| !c.is_alphabetic()).any(|w| HUD_WORDS.iter().any(|h| w.eq_ignore_ascii_case(h)))
}

/// A word the reader made up from noise: a dot between letters ("Ei.atL1"), or a capital after
/// a small letter in a word that also has a digit.
fn garbled(w: &str) -> bool {
    let c: Vec<char> = w.chars().collect();
    let dot_inside = c.windows(3).any(|x| x[1] == '.' && x[0].is_alphanumeric() && x[2].is_alphanumeric());
    let case_flip = c.windows(2).any(|x| x[0].is_lowercase() && x[1].is_uppercase());
    dot_inside || (case_flip && c.iter().any(char::is_ascii_digit))
}

/// One accented letter in an otherwise plain name is the reader's mistake ("Mutatéd"); names
/// with more than one keep theirs.
fn plain_accent(s: String) -> String {
    let odd: Vec<char> = s.chars().filter(|c| c.is_alphabetic() && !c.is_ascii()).collect();
    let &[c] = odd.as_slice() else { return s };
    let base = match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'À' | 'Á' | 'Â' | 'Ä' | 'Ã' => 'A',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' => 'O',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        _ => return s,
    };
    s.replace(c, &base.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: Bar = Bar { x0: 290, x1: 990, y0: 620, y1: 632 };
    const B2: Bar = Bar { x0: 290, x1: 990, y0: 590, y1: 602 };
    const RED: [u8; 3] = [30, 30, 170];

    fn found(b: Bar) -> Look {
        Look { found: Some((b, RED)), ..Look::default() }
    }
    fn tracked(fill: u8) -> Look {
        Look { tracked: true, fill, found: None }
    }
    fn none() -> Look {
        Look::default()
    }

    /// Frames every 250 ms from `t` until a name is wanted, then reads `names` until one engages.
    fn bring_up(w: &mut BossWatch, b: Bar, t: &mut u64, names: &[&str]) -> Option<Out> {
        w.w = 1280;
        for _ in 0..STREAK {
            assert_eq!(w.step(found(b), *t), None);
            *t += 250;
        }
        for n in names {
            assert!(w.wants_name(*t).is_some(), "a name is wanted");
            let out = w.named(clean_name(n, true), *t, &[]);
            assert_eq!(w.step(if out.is_some() { tracked(100) } else { found(b) }, *t), None);
            *t += 500;
            if out.is_some() {
                return out;
            }
        }
        None
    }

    /// The fight's remaining name votes, each read as `n`; what they decide.
    fn votes(w: &mut BossWatch, t: &mut u64, names: &[&str]) -> Vec<Out> {
        let mut outs = vec![];
        for n in names {
            assert!(w.wants_name(*t).is_some(), "a vote is wanted");
            outs.extend(w.named(clean_name(n, true), *t, &[]));
            w.step(tracked(100), *t);
            *t += 500;
        }
        assert!(w.wants_name(*t).is_none(), "no more votes");
        outs
    }

    #[test]
    fn the_fight_opens_on_the_first_name_read_from_when_the_bar_showed_up() {
        let mut w = BossWatch::default();
        let mut t = 10_000;
        let out = bring_up(&mut w, B, &mut t, &["Margit, the Fell Omen"]);
        assert_eq!(out, Some(Out::Engage { since: 10_000, name: "Margit, the Fell Omen".into(), phase: 1 }));
        assert_eq!(votes(&mut w, &mut t, &["Margit, the Fell 0men", "Margit, the Fell Omen", "", "Margit, the Fell Omen"]), vec![], "the votes agree");
        assert_eq!(w.step(tracked(70), t), None);
        assert_eq!(w.now(), Some((t, 70, 1)));
        w.death();
        assert!(!w.in_try());
    }

    /// Khazan, 2026-10-21: "Xaldin" read as "aldii". The first read opens the fight; most of the
    /// next reads agree on another spelling, so it's renamed.
    #[test]
    fn a_misread_first_name_is_voted_out() {
        let mut w = BossWatch::default();
        let mut t = 0;
        let out = bring_up(&mut w, B, &mut t, &["aldii"]);
        assert!(matches!(out, Some(Out::Engage { ref name, .. }) if name == "aldii"));
        let outs = votes(&mut w, &mut t, &["Xaldin", "Xaldin", "Xa1din", "aldii"]);
        assert_eq!(outs, vec![Out::Rename { since: 0, name: "Xaldin".into() }]);
    }

    #[test]
    fn a_boss_fought_before_takes_its_spelling_at_once() {
        let mut w = BossWatch::default();
        w.w = 1280;
        let mut t = 0;
        for _ in 0..STREAK {
            w.step(found(B), t);
            t += 250;
        }
        let known = vec!["Xaldin".to_string()];
        let out = w.named(clean_name("aldii", true), t, &known);
        assert_eq!(out, Some(Out::Engage { since: 0, name: "Xaldin".into(), phase: 1 }));
        w.step(tracked(100), t);
        assert!(w.wants_name(t + 5_000).is_none(), "a known boss isn't voted on");
    }

    #[test]
    fn a_bar_seen_once_or_with_no_name_starts_nothing() {
        let mut w = BossWatch::default();
        w.w = 1280;
        assert_eq!(w.step(found(B), 0), None);
        assert_eq!(w.step(none(), 250), None);
        assert_eq!(w.step(found(B), 500), None);
        assert!(w.wants_name(600).is_none(), "one frame isn't a bar");
        let mut t = 1_000;
        let out = bring_up(&mut w, B, &mut t, &["", "1234", "", "", "", ""]);
        assert_eq!(out, None, "a loading bar with nothing to read isn't a boss");
        assert!(!w.in_try());
    }

    /// Frames of `b` every 250 ms from `t` to `until`, each read wanted read as nothing: when
    /// the reads were.
    fn unreadable(w: &mut BossWatch, b: Bar, t: &mut u64, until: u64) -> Vec<u64> {
        let mut at = vec![];
        while *t < until {
            w.step(found(b), *t);
            if w.wants_name(*t).is_some() {
                at.push(*t);
                w.named(None, *t, &[]);
            }
            *t += 250;
        }
        at
    }

    /// No bar for MISSES frames: the bar went.
    fn gone(w: &mut BossWatch, t: &mut u64) {
        for _ in 0..MISSES {
            w.step(none(), *t);
            *t += 250;
        }
    }

    /// Performance run, 2026-10-21: a bar with nothing to read that kept flickering kept Deck's
    /// text reader busy for most of a minute after a game started. After its first six reads
    /// fail, its spot is read twice at most each time, after a wait that doubles.
    #[test]
    fn a_flickering_bar_with_no_name_is_read_less_and_less() {
        let mut w = BossWatch::default();
        w.w = 1280;
        let mut t = 0;
        assert_eq!(unreadable(&mut w, B, &mut t, 10_000), vec![500, 1_000, 1_500, 2_000, 2_500, 3_000]);
        gone(&mut w, &mut t);
        assert_eq!(unreadable(&mut w, B, &mut t, 12_000), vec![11_250, 11_750], "back after the wait: two reads");
        gone(&mut w, &mut t);
        assert_eq!(unreadable(&mut w, B, &mut t, 20_000), vec![15_750, 16_250], "back within the wait: read when it ends");
        assert_eq!(w.quiet.map(|q| q.gap), Some(QUIET_FIRST_MS * 4));
        assert!(!w.in_try());
    }

    #[test]
    fn a_boss_in_a_quiet_spot_still_opens_its_fight_from_when_its_bar_showed_up() {
        let mut w = BossWatch::default();
        let mut t = 0;
        assert_eq!(bring_up(&mut w, B, &mut t, &["", "", "", "", "", ""]), None);
        gone(&mut w, &mut t);
        let showed = t;
        let mut out = None;
        while out.is_none() && t < showed + 10_000 {
            assert_eq!(w.step(found(B), t), None);
            if w.wants_name(t).is_some() {
                out = w.named(clean_name("Margit, the Fell Omen", true), t, &[]);
            }
            t += 250;
        }
        assert!(t > showed + 750, "it waited");
        assert_eq!(out, Some(Out::Engage { since: showed, name: "Margit, the Fell Omen".into(), phase: 1 }));
        assert!(w.quiet.is_none(), "a named bar ends the quiet");
    }

    /// An enemy's bar that moves with it is a new spot each time it stops: the wait still holds.
    #[test]
    fn a_bar_with_no_name_that_moves_is_waited_for_too() {
        let mut w = BossWatch::default();
        w.w = 1280;
        let mut t = 0;
        assert_eq!(unreadable(&mut w, B, &mut t, 3_250).len(), 6);
        gone(&mut w, &mut t);
        assert_eq!(unreadable(&mut w, B2, &mut t, 8_000), vec![5_000, 5_500], "somewhere else: after the wait, two reads");
    }

    /// The bar empties, a cutscene hides it: a win after WIN_MS. The second phase comes up under
    /// another name within PHASE_GAP: the same try, phase 2 (the win is taken back).
    #[test]
    fn a_cutscene_and_a_second_name_are_the_same_try() {
        let mut w = BossWatch::default();
        let mut t = 0;
        bring_up(&mut w, B, &mut t, &["Radagon of the Golden Order"]).unwrap();
        votes(&mut w, &mut t, &["Radagon of the Golden Order"; 4]);
        w.step(tracked(3), t);
        let emptied = t;
        t += 250;
        let mut outs = vec![];
        for _ in 0..160 {
            outs.extend(w.step(none(), t));
            t += 250;
        }
        assert_eq!(outs, vec![Out::Won { last_seen: emptied }]);
        assert!(!w.in_try(), "won, for now");
        let out = bring_up(&mut w, B2, &mut t, &["Elden Beast"]);
        assert!(matches!(out, Some(Out::Engage { phase: 2, ref name, .. }) if name == "Elden Beast"), "the bar came back: phase 2 ({out:?})");
        w.step(tracked(2), t);
        let last = t;
        let mut outs = vec![];
        for _ in 0..(PHASE_GAP_MS / 250 + 4) {
            t += 250;
            outs.extend(w.step(none(), t));
        }
        assert_eq!(outs, vec![Out::Won { last_seen: last }], "won after WIN_MS, nothing else");
    }

    #[test]
    fn a_bar_that_refills_is_the_next_phase_and_a_death_ends_the_try() {
        let mut w = BossWatch::default();
        let mut t = 0;
        bring_up(&mut w, B, &mut t, &["Malenia, Blade of Miquella"]).unwrap();
        for fill in [60, 20, 8, 40, 95, 50] {
            w.step(tracked(fill), t);
            t += 250;
        }
        assert_eq!(w.now().map(|n| (n.1, n.2)), Some((50, 2)));
        w.death();
        let out = bring_up(&mut w, B, &mut t, &["Malenia, Blade of Miquella"]);
        assert!(matches!(out, Some(Out::Engage { phase: 1, .. })), "after a death, a new try");
    }

    /// Two bars (a duo): when the followed one goes, the other is followed in the same try; no
    /// win while a boss bar is up.
    #[test]
    fn a_second_bar_keeps_the_try_going() {
        let mut w = BossWatch::default();
        let mut t = 0;
        bring_up(&mut w, B, &mut t, &["Valiant Gargoyle"]).unwrap();
        votes(&mut w, &mut t, &["Valiant Gargoyle"; 4]);
        w.step(tracked(1), t);
        t += 250;
        let out = bring_up(&mut w, B2, &mut t, &["Valiant Gargoyle (Twinblade)"]);
        assert!(matches!(out, Some(Out::Engage { phase: 2, .. })), "{out:?}");
        for _ in 0..(PHASE_GAP_MS / 250 + 4) {
            assert_eq!(w.step(tracked(40), t), None, "the other bar is still up");
            t += 250;
        }
    }

    #[test]
    fn leaving_the_arena_with_health_left_is_no_win() {
        let mut w = BossWatch::default();
        let mut t = 0;
        bring_up(&mut w, B, &mut t, &["Tree Sentinel"]).unwrap();
        votes(&mut w, &mut t, &["Tree Sentinel"; 4]);
        w.step(tracked(60), t);
        let mut outs = vec![];
        for _ in 0..(PHASE_GAP_MS / 250 + 4) {
            t += 250;
            outs.extend(w.step(none(), t));
        }
        assert_eq!(outs, vec![Out::Leave]);
        assert!(!w.in_try());
        // coming back to it: the same boss, a new try (phase 1: nothing emptied, same name)
        let out = bring_up(&mut w, B, &mut t, &["Tree Sentinel"]);
        assert!(matches!(out, Some(Out::Engage { phase: 1, .. })));
        assert_eq!(w.victory(), Some(Out::Won { last_seen: t - 500 }), "a victory banner wins at once");
    }

    #[test]
    fn the_name_is_the_big_writing_nearest_the_bar() {
        let l = |text: &str, y0: f32, y1: f32| crate::deaths::screen::ocr::Line { text: text.into(), y0, y1 };
        // above the bar: a small tip at the top, the name, a damage number by the bar
        let lines = [l("Hold Y to lock on", 0.0, 14.0), l("Xaldin", 20.0, 52.0), l("1,203", 54.0, 70.0)];
        assert_eq!(pick_name(&lines, true).as_deref(), Some("Xaldin"));
        // the scenery read as small words nearer the bar loses to the big name
        let lines = [l("Xaldin", 10.0, 42.0), l("Ei.atL1", 46.0, 58.0)];
        assert_eq!(pick_name(&lines, true).as_deref(), Some("Xaldin"));
        // two names as big: the one nearest the bar (below it: the top one)
        let lines = [l("Gwyn, Lord of Cinder", 4.0, 30.0), l("Soul of Cinder", 40.0, 66.0)];
        assert_eq!(pick_name(&lines, false).as_deref(), Some("Gwyn, Lord of Cinder"));
        assert_eq!(pick_name(&lines, true).as_deref(), Some("Soul of Cinder"));
        assert_eq!(pick_name(&[], true), None);
    }

    #[test]
    fn letters_misread_in_different_reads_are_outvoted() {
        let reads: Vec<String> = ["Ma1enia, Blade of Miquella", "Malenia, Blade of Miquel1a", "Malenia, Blade 0f Miquella"].map(String::from).to_vec();
        assert_eq!(vote(&reads).as_deref(), Some("Malenia, Blade of Miquella"));
        let reads: Vec<String> = ["Xa1din", "Xaldin"].map(String::from).to_vec();
        assert_eq!(vote(&reads).as_deref(), Some("Xaldin"), "a tie goes to the letter");
        let reads: Vec<String> = ["Xaldin", "aldii", "Xaldin"].map(String::from).to_vec();
        assert_eq!(vote(&reads).as_deref(), Some("Xaldin"));
    }

    #[test]
    fn names_are_cleaned_of_numbers_and_tips() {
        assert_eq!(clean_name("2417\nMargit, the Fell Omen", true).as_deref(), Some("Margit, the Fell Omen"));
        assert_eq!(clean_name("Godrick the Grafted\n1,203", false).as_deref(), Some("Godrick the Grafted"));
        assert_eq!(clean_name("12 345", true), None);
        assert_eq!(clean_name("Tip: rest at a site of grace to restore your flasks and health.", true), None);
        assert_eq!(clean_name("— Ab —", true), None);
    }

    /// From a play session, 2026-10-21: a Helldivers 2 event bar, reader noise and a stray accent.
    #[test]
    fn hud_messages_noise_and_stray_accents_are_not_names() {
        assert_eq!(clean_name("Bot Drop Detected", true), None);
        assert_eq!(clean_name("Ei.atL1", true), None);
        assert_eq!(clean_name("Mutatéd Xilence", true).as_deref(), Some("Mutated Xilence"));
        assert_eq!(clean_name("Séléné, the Moon", true).as_deref(), Some("Séléné, the Moon"), "a name's own accents stay");
        assert_eq!(clean_name("St. Trina", true).as_deref(), Some("St. Trina"));
        assert_eq!(clean_name("Malenia, Blade of Miquel1a", true).as_deref(), Some("Malenia, Blade of Miquel1a"), "a misread letter is left to the vote");
    }
}
