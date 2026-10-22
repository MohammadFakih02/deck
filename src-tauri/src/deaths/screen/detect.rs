//! Screen detection's decisions, apart from the capture so they can be tested: when a frame is
//! worth reading (the gate), whether the text read is a death screen (the words), and when a
//! death screen counts (the matcher).
//!
//! Frames come in as 64x36 grey thumbnails (4 a second). Windows' OCR is the expensive part, so
//! it only runs on a frame whose middle has settled (a banner that faded in and stays), at most
//! once a second, and an unchanged picture is read again only every 3 s.

use std::time::{Duration, Instant};

pub const TW: usize = 64;
pub const TH: usize = 36;

/// Death screens most games use, read line by line. Single words ("DEATH", "RETRY") are left
/// out: menus and item names have them.
pub const COMMON_WORDS: [&str; 9] = ["YOU DIED", "YOU ARE DEAD", "YOU HAVE DIED", "YOU WERE KILLED", "GAME OVER", "WASTED", "MISSION FAILED", "YOU HAVE BEEN DEFEATED", "YOU WERE DEFEATED"];

const OCR_EVERY: Duration = Duration::from_secs(1);
const SAME_SCENE_EVERY: Duration = Duration::from_secs(3);
const CONFIRM_AFTER: Duration = Duration::from_millis(500);
/// mean change per cell (0–255) below which two frames are "the same picture"
const STABLE: u32 = 6;
const NEW_SCENE: u32 = 10;
/// a frame this dark and flat is a loading screen or a capture that sees nothing
const DARK: u32 = 6;
const FLAT: u32 = 3;
const BLIND_AFTER: Duration = Duration::from_secs(20);
/// The screen going this much darker within DIM_WINDOW (a death fade, a pause) is worth reading
/// for BURST_FOR even while the picture is still changing: a death banner fades in over a moving,
/// darkening scene that never "settles" (Dark Souls II), so waiting for calm would miss it.
const DIM_WINDOW: Duration = Duration::from_millis(1500);
const DIM_TO_PERCENT: u32 = 70;
/// brighter than this before the dimming (a dark scene getting darker isn't a fade)
const DIM_FROM: u32 = 24;
const BURST_FOR: Duration = Duration::from_secs(6);
const BURST_EVERY: Duration = Duration::from_millis(700);

/// The middle of the picture, where death banners are: the middle 3/4 across, middle 2/3 down.
fn middle(t: &[u8]) -> Vec<u8> {
    let (x0, x1, y0, y1) = (TW / 8, TW - TW / 8, TH / 6, TH - TH / 6);
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| t[y * TW + x])).collect()
}

fn diff(a: &[u8], b: &[u8]) -> u32 {
    if a.len() != b.len() || a.is_empty() {
        return 255;
    }
    (a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u32).sum::<u32>()) / a.len() as u32
}

/// Near black and flat: nothing to read.
pub fn empty(t: &[u8]) -> bool {
    if t.is_empty() {
        return true;
    }
    let mean = t.iter().map(|v| *v as u32).sum::<u32>() / t.len() as u32;
    let spread = t.iter().map(|v| (*v as u32).abs_diff(mean)).sum::<u32>() / t.len() as u32;
    mean < DARK && spread < FLAT
}

#[derive(Default)]
pub struct Gate {
    prev: Option<Vec<u8>>,
    read: Option<(Instant, Vec<u8>)>,
    /// since when every frame has been empty
    empty_since: Option<Instant>,
    /// the last frame's middle differed from the one before (or it was black)
    moving: bool,
    /// the whole picture's brightness lately: (when, mean)
    means: std::collections::VecDeque<(Instant, u32)>,
    /// until when a dimming makes every 700 ms frame worth reading
    burst_until: Option<Instant>,
}

impl Gate {
    /// A new thumbnail; true = read this frame now. `confirm`: the last read matched and wants
    /// a second look soon.
    pub fn feed(&mut self, thumb: &[u8], now: Instant, confirm: bool) -> bool {
        if thumb.len() != TW * TH {
            return false;
        }
        if empty(thumb) {
            self.empty_since.get_or_insert(now);
            self.prev = None;
            self.moving = true;
            return false;
        }
        self.empty_since = None;
        // a sudden dimming (a death fade): read for a few seconds, settled or not
        let mean = thumb.iter().map(|v| *v as u32).sum::<u32>() / thumb.len() as u32;
        self.means.push_back((now, mean));
        while self.means.front().is_some_and(|(t, _)| now.duration_since(*t) > DIM_WINDOW) {
            self.means.pop_front();
        }
        let before = self.means.iter().map(|(_, m)| *m).max().unwrap_or(mean);
        if before >= DIM_FROM && mean * 100 <= before * DIM_TO_PERCENT {
            self.burst_until = Some(now + BURST_FOR);
            self.means.clear();
        }
        let bursting = self.burst_until.is_some_and(|u| now < u);
        let mid = middle(thumb);
        let stable = self.prev.as_ref().is_some_and(|p| diff(p, &mid) <= STABLE);
        self.moving = !stable;
        self.prev = Some(mid.clone());
        let go = match &self.read {
            None => stable || bursting,
            Some((at, _)) if confirm => now.duration_since(*at) >= CONFIRM_AFTER,
            Some((at, _)) if bursting => now.duration_since(*at) >= BURST_EVERY,
            Some((at, scene)) => {
                let since = now.duration_since(*at);
                stable && since >= OCR_EVERY && (diff(scene, &mid) >= NEW_SCENE || since >= SAME_SCENE_EVERY)
            }
        };
        if go {
            self.read = Some((now, mid));
        }
        go
    }

    /// The picture is changing (or black): the game moved on from whatever was on screen.
    pub fn moving(&self) -> bool {
        self.moving
    }

    /// Every frame has been black for a while: the capture can't see this game.
    pub fn blind(&self, now: Instant) -> bool {
        self.empty_since.is_some_and(|t| now.duration_since(t) >= BLIND_AFTER)
    }
}

/// Upper case letters and single spaces: "You  Died!" -> "YOU DIED".
pub fn normalize(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_uppercase());
        } else if c.is_whitespace() && !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

pub fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (prev + usize::from(a[i - 1] != b[j - 1])).min(row[j] + 1).min(row[j - 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// A long phrase is long enough to tell apart from anything else with a letter or two dropped or
/// misread (a stylised K read as "(" is dropped); a short one isn't ("YOU DID" vs "YOU DIED").
const LONG_PHRASE: usize = 12;

/// `line` is `w` with at most `slack` letters read wrong (none dropped or added).
fn misread(line: &str, w: &str, slack: usize) -> bool {
    line.chars().count() == w.chars().count() && line.chars().zip(w.chars()).filter(|(a, b)| a != b).count() <= slack
}

/// OCR's usual misreads of big stylised letters (I/l/1, O/0).
fn fold(s: &str) -> String {
    s.chars().map(|c| match c { '0' => 'O', '1' | 'L' | '|' => 'I', '5' => 'S', c => c }).collect()
}

/// A line of the text read is one of `words` on its own (a banner). A phrase of several words
/// may have a letter read wrong (one per 8 letters); a long one (12+ letters) may also have letters
/// dropped or added ("(HAZAN HAS FALLEh"). A single word must be exact ("WASTED" is one letter
/// from "WANTED"), and a short phrase never passes with a letter dropped ("YOU DID" isn't
/// "YOU DIED"). Subtitles and menus that only contain the words don't count.
pub fn is_death_text(text: &str, words: &[String]) -> bool {
    let wanted: Vec<String> = words.iter().map(|w| fold(&normalize(w))).filter(|w| !w.is_empty()).collect();
    text.lines().map(|l| fold(&normalize(l))).filter(|l| !l.is_empty()).any(|line| {
        wanted.iter().any(|w| {
            // "YOUDIED": OCR often drops the gap in spaced-out letters
            if line == *w || line.replace(' ', "") == w.replace(' ', "") {
                return true;
            }
            let slack = (w.chars().count() / 8).max(1);
            w.contains(' ') && (misread(&line, w, slack) || (w.chars().count() >= LONG_PHRASE && edits(&line, w) <= slack))
        })
    })
}

/// When death screens count: twice in a row (two reads, within 3 s), then not again until the
/// screen has gone and 8 s have passed.
#[derive(Default)]
pub struct Matcher {
    hit_at: Option<Instant>,
    counted_at: Option<Instant>,
    /// a read without the death screen since the last count
    cleared: bool,
}

const PAIR_WITHIN: Duration = Duration::from_secs(3);
const COOLDOWN: Duration = Duration::from_secs(8);
/// a death banner fades in for a moment after it is counted: that isn't it going away
const MOVED_AFTER: Duration = Duration::from_secs(3);

impl Matcher {
    /// The result of one read; true = count a death now.
    pub fn feed(&mut self, death_text: bool, now: Instant) -> bool {
        if !death_text {
            self.hit_at = None;
            self.cleared = true;
            return false;
        }
        if let Some(c) = self.counted_at {
            if !self.cleared || now.duration_since(c) < COOLDOWN {
                return false;
            }
        }
        match self.hit_at {
            Some(h) if now.duration_since(h) <= PAIR_WITHIN => {
                self.hit_at = None;
                self.counted_at = Some(now);
                self.cleared = false;
                true
            }
            _ => {
                self.hit_at = Some(now);
                false
            }
        }
    }

    /// The picture changed. Once the death screen that was counted has been up a few seconds, a
    /// change means it is gone, so the next death screen counts (action games rarely show a calm
    /// frame between deaths, so waiting for one to be read would miss it).
    pub fn moved(&mut self, now: Instant) {
        if self.counted_at.is_some_and(|c| now.duration_since(c) >= MOVED_AFTER) {
            self.cleared = true;
        }
    }

    /// Another way of seeing deaths just counted one: the same cooldown applies here.
    pub fn counted(&mut self, now: Instant) {
        self.hit_at = None;
        self.counted_at = Some(now);
        self.cleared = false;
    }

    /// One match is waiting for its second.
    pub fn pending(&self) -> bool {
        self.hit_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(v: u8) -> Vec<u8> {
        vec![v; TW * TH]
    }

    fn with_banner(base: u8, banner: u8) -> Vec<u8> {
        let mut f = frame(base);
        for y in 15..21 {
            for x in 12..52 {
                f[y * TW + x] = banner;
            }
        }
        f
    }

    #[test]
    fn the_gate_reads_a_settled_new_picture_at_most_once_a_second() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut g = Gate::default();
        assert!(!g.feed(&frame(80), t, false), "first frame: nothing to compare with");
        assert!(g.feed(&frame(80), ms(250), false), "settled: read it");
        assert!(!g.feed(&frame(80), ms(500), false), "read a moment ago");
        assert!(!g.feed(&with_banner(80, 200), ms(1500), false), "changing: wait for it to settle");
        assert!(g.feed(&with_banner(80, 200), ms(1750), false), "a new settled picture");
        assert!(!g.feed(&with_banner(80, 200), ms(2750), false), "the same picture: not again yet");
        assert!(g.feed(&with_banner(80, 200), ms(4800), false), "the same picture after 3 s");
    }

    /// Dark Souls II, 2026-10-23: the death banner fades in over a darkening scene that keeps moving,
    /// so the picture never settled and the death was never read.
    #[test]
    fn a_sudden_dimming_is_read_even_while_the_picture_keeps_changing() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut g = Gate::default();
        // playing: bright and always moving: never read
        for i in 0..12u64 {
            assert!(!g.feed(&frame(if i % 2 == 0 { 120 } else { 150 }), ms(250 * i), false), "gameplay isn't read while it moves");
        }
        // the fade: darker, and still changing from frame to frame
        let mut read_at = vec![];
        for (i, v) in [60u8, 52, 44, 40, 36, 33, 30, 28, 27, 26, 25, 24, 23, 22, 21].iter().enumerate() {
            let now = ms(3000 + 250 * i as u64);
            if g.feed(&with_banner(*v, v.saturating_add(70 + (i as u8 % 3) * 9)), now, false) {
                read_at.push(i);
            }
        }
        assert!(read_at.len() >= 3 && read_at[0] <= 1, "read soon after the dimming, about every 0.7 s: {read_at:?}");
        // the burst ends: moving again is not read
        g.feed(&with_banner(18, 150), ms(11_750), false); // the first frame after the gap compares with the fade's last
        for i in 0..8u64 {
            assert!(!g.feed(&with_banner(18, if i % 2 == 0 { 90 } else { 150 }), ms(12_000 + 250 * i), false), "after the burst, moving frames wait again (frame {i})");
        }
    }

    #[test]
    fn a_dark_scene_getting_darker_or_a_small_change_is_no_burst() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut g = Gate::default();
        for i in 0..6u64 {
            g.feed(&frame(if i % 2 == 0 { 14 } else { 20 }), ms(250 * i), false);
        }
        for i in 0..8u64 {
            assert!(!g.feed(&frame(if i % 2 == 0 { 12 } else { 20 }), ms(2000 + 250 * i), false), "from a dark scene: no burst (frame {i})");
        }
        let mut g = Gate::default();
        for i in 0..8u64 {
            assert!(!g.feed(&frame(if i % 2 == 0 { 100 } else { 120 }), ms(250 * i), false), "brightness wobbling a little: no burst");
        }
    }

    #[test]
    fn a_pending_match_gets_its_second_look_quickly_even_while_moving() {
        let t = Instant::now();
        let mut g = Gate::default();
        g.feed(&frame(80), t, false);
        assert!(g.feed(&frame(80), t + Duration::from_millis(250), false));
        assert!(g.feed(&frame(140), t + Duration::from_millis(750), true));
    }

    #[test]
    fn black_frames_are_never_read_and_long_black_means_blind() {
        let t = Instant::now();
        let mut g = Gate::default();
        for i in 0..100 {
            assert!(!g.feed(&frame(1), t + Duration::from_millis(250 * i), false));
        }
        assert!(g.blind(t + Duration::from_secs(25)));
        g.feed(&frame(80), t + Duration::from_secs(26), false);
        assert!(!g.blind(t + Duration::from_secs(26)));
    }

    #[test]
    fn death_text_is_a_banner_line_with_room_for_ocr_slips() {
        let w = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let common = w(&COMMON_WORDS);
        assert!(is_death_text("YOU DIED", &common));
        assert!(is_death_text("You  Died!", &common));
        assert!(is_death_text("Y0U DlED", &common), "O/0 and I/l misreads");
        assert!(is_death_text("YOUDIED", &common));
        assert!(is_death_text("Souls 12,345\nYOU DIED", &common), "another line on screen is fine");
        assert!(is_death_text("GAME OVER", &common));
        assert!(!is_death_text("Nobody knows how you died out there", &common), "a subtitle that only contains it");
        assert!(!is_death_text("CONTINUE\nQUIT GAME", &common));
        assert!(!is_death_text("", &common));
        assert!(is_death_text("YOU DIEO", &common), "one letter read wrong in a phrase");
        assert!(!is_death_text("YOU DID", &common), "a dropped letter is another phrase");
        assert!(is_death_text("WASTED", &common));
        assert!(!is_death_text("WANTED", &common), "one word: exactly");
        assert!(!is_death_text("LASTED", &common));
        let khazan = w(&["KHAZAN HAS FALLEN"]);
        assert!(is_death_text("(HAZAN HAS FALLEh", &khazan), "a stylised K dropped and an N misread, as Windows OCR read it");
        assert!(!is_death_text("HAS FALLEN", &khazan), "half of it isn't it");
        let nioh = w(&["FREED FROM THIS MORTAL COIL"]);
        assert!(is_death_text("Freed from this mortal coil", &nioh));
        assert!(!is_death_text("FREED", &nioh), "a part of the words isn't them");
    }

    #[test]
    fn the_matcher_counts_two_reads_in_a_row_once() {
        let t = Instant::now();
        let s = |n| t + Duration::from_millis(n);
        let mut m = Matcher::default();
        assert!(!m.feed(true, s(0)));
        assert!(m.pending());
        assert!(m.feed(true, s(600)), "second read: a death");
        assert!(!m.feed(true, s(1600)), "the same death screen still up");
        assert!(!m.feed(true, s(9000)), "still up after the cooldown: not a new death");
        assert!(!m.feed(false, s(10000)));
        assert!(!m.feed(true, s(20000)));
        assert!(m.feed(true, s(20500)), "the next death");
    }

    #[test]
    fn the_next_death_counts_after_the_picture_moved_even_if_no_calm_frame_was_read() {
        let t = Instant::now();
        let s = |n| t + Duration::from_millis(n);
        let mut m = Matcher::default();
        m.feed(true, s(0));
        assert!(m.feed(true, s(600)));
        m.moved(s(1500));
        assert!(!m.feed(true, s(9000)), "the banner fading in isn't it going away: still the same death screen");
        m.moved(s(4000));
        m.moved(s(4250));
        assert!(!m.feed(true, s(5000)), "moved, but still inside the cooldown");
        assert!(!m.feed(true, s(12000)), "first sight of the next death screen");
        assert!(m.feed(true, s(12500)), "the next death counts: no read ever came back without the death text");
    }

    #[test]
    fn a_death_screen_that_never_goes_away_counts_once() {
        let t = Instant::now();
        let mut m = Matcher::default();
        m.feed(true, t);
        assert!(m.feed(true, t + Duration::from_millis(600)));
        for i in 0..40 {
            let now = t + Duration::from_secs(1 + i);
            assert!(!m.feed(true, now), "the picture is still: nothing moved");
        }
    }

    #[test]
    fn the_gate_knows_when_the_picture_moves() {
        let t = Instant::now();
        let mut g = Gate::default();
        g.feed(&frame(80), t, false);
        g.feed(&frame(80), t + Duration::from_millis(250), false);
        assert!(!g.moving());
        g.feed(&with_banner(80, 200), t + Duration::from_millis(500), false);
        assert!(g.moving());
        g.feed(&frame(1), t + Duration::from_millis(750), false);
        assert!(g.moving(), "black counts as moving");
    }

    #[test]
    fn a_single_glimpse_never_counts() {
        let t = Instant::now();
        let mut m = Matcher::default();
        assert!(!m.feed(true, t));
        assert!(!m.feed(false, t + Duration::from_millis(500)));
        assert!(!m.feed(true, t + Duration::from_secs(1)));
        assert!(!m.feed(true, t + Duration::from_secs(10)), "too far apart to be a pair");
    }
}
