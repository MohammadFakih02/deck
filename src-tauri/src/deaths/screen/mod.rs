//! Screen detection (stage 3): while a counted game is in front, its window is captured 4 times
//! a second and Windows' OCR reads the middle when the picture settles; the game's own death
//! words (catalogue) and the common ones, seen twice in a row, count a death. Death screens you
//! taught Deck (`teach.rs`) are matched on every frame without OCR.
//! Started and stopped by the deaths thread; one watcher at a time.
//!
//! Boss fights (`boss.rs`): the same frames are searched for a boss's health bar; its name is read
//! with the same OCR, only next to the bar and only while the name is still unknown.
//!
//! The last 10 s of small frames stay in memory (`RECENT`) so a death you counted yourself can be
//! taught from them; they're never written anywhere.

pub mod bar;
pub mod boss;
pub mod capture;
pub mod detect;
pub mod ocr;
pub mod teach;

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// frames a second for a game Deck knows (its own death words, or taught death screens)
const RATE: f64 = 4.0;
/// for any other game: half the work, still two looks at a banner that stays up a second or more
const SLOW_RATE: f64 = 2.0;
/// frames kept for teaching: the last 20 s, 2 a second (people count a death a while after it)
const KEEP: usize = 40;
const KEEP_MS: u64 = 20_000;
const KEEP_EVERY_MS: u64 = 450;
/// the teaching strip's pictures (grey)
pub const PW: usize = 160;
pub const PH: usize = 90;

/// What the watcher sees, for the page.
pub const STARTING: u8 = 0;
pub const WATCHING: u8 = 1;
pub const BLIND: u8 = 2;
pub const FAILED: u8 = 3;
/// watching, but Windows' text recognition has no English, so English death words may be missed
pub const NO_ENGLISH: u8 = 4;

#[derive(Clone)]
pub struct Shot {
    /// when (ms since 1970)
    pub t: u64,
    pub thumb: Vec<u8>,
    pub preview: Vec<u8>,
    /// the picture's width / height x 1000 (a fingerprint fits pictures of the same shape)
    pub aspect: u32,
}

/// The last picture of a game with a bar-like strip on it, big enough to show where its boss bar
/// and the boss's name are (RGB, `w` x `h`), with its strips. Memory only, one at a time.
#[derive(Clone)]
pub struct BigShot {
    pub game: String,
    pub t: u64,
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
    pub bars: Vec<bar::Spot>,
}

/// width of `BigShot`
pub const BIG_W: usize = 960;
static BIG: Mutex<Option<BigShot>> = Mutex::new(None);

/// The last picture of `game` with a bar-like strip on it.
pub fn big(game: &str) -> Option<BigShot> {
    BIG.lock().as_ref().filter(|b| b.game == game).cloned()
}

/// width / height x 1000
pub fn aspect_of(w: usize, h: usize) -> u32 {
    (w * 1000).checked_div(h).map_or(0, |a| a as u32)
}

/// The game being watched and its last frames.
static RECENT: Mutex<(String, VecDeque<Shot>)> = Mutex::new((String::new(), VecDeque::new()));
/// bumped when a game's taught death screens change, so a running watcher reloads them
static TAUGHT: AtomicU32 = AtomicU32::new(0);
/// look for boss bars (Settings: "Find boss fights by themselves")
static BOSSES: AtomicBool = AtomicBool::new(true);
/// where the game in front shows its boss bar, when that isn't centred: (game, spot)
static SPOT: Mutex<Option<(String, bar::Spot)>> = Mutex::new(None);
/// where the game in front shows the boss's name, when you showed it: (game, spot)
static NAME_SPOT: Mutex<Option<(String, bar::Spot)>> = Mutex::new(None);
/// bosses already fought in the game in front: (game, names); a name read like one takes its spelling
static KNOWN: Mutex<(String, Vec<String>)> = Mutex::new((String::new(), Vec::new()));
/// the boss bar of the try going on: (game, when last seen, how full %, phase)
static BAR_NOW: Mutex<Option<(String, u64, u8, u8)>> = Mutex::new(None);
/// a death this long after the boss bar was last seen still happened in its fight (the bar fades
/// before the death screen shows)
const BAR_FRESH_MS: u64 = 15_000;

pub fn set_bosses(on: bool) {
    BOSSES.store(on, Ordering::Relaxed);
}

/// Where `game` shows its boss bar (taught, or from the catalogue); None: centred.
pub fn set_spot(game: &str, spot: Option<bar::Spot>) {
    *SPOT.lock() = spot.map(|s| (game.to_string(), s));
}

/// The bosses already fought in `game`.
pub fn set_known(game: &str, names: Vec<String>) {
    *KNOWN.lock() = (game.to_string(), names);
}

fn known_of(game: &str) -> Vec<String> {
    let k = KNOWN.lock();
    if k.0 == game {
        k.1.clone()
    } else {
        Vec::new()
    }
}

/// Where `game` shows the boss's name (you showed it); None: next to the bar.
pub fn set_name_spot(game: &str, spot: Option<bar::Spot>) {
    *NAME_SPOT.lock() = spot.map(|s| (game.to_string(), s));
}

fn name_spot_of(game: &str) -> Option<bar::Spot> {
    NAME_SPOT.lock().as_ref().filter(|s| s.0 == game).map(|s| s.1)
}

fn spot_of(game: &str) -> Option<bar::Spot> {
    SPOT.lock().as_ref().filter(|s| s.0 == game).map(|s| s.1)
}

/// The boss's health (%) and the fight's phase for a death in `game` at `t`, when its bar was up.
pub fn bar_now(game: &str, t: u64) -> Option<(u8, u8)> {
    BAR_NOW.lock().as_ref().filter(|b| b.0 == game && t.saturating_sub(b.1) <= BAR_FRESH_MS).map(|b| (b.2, b.3))
}

/// What the watcher saw.
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    /// a death screen; what saw it ("words", or the taught screen's `by`)
    Death(String),
    Boss(boss::Out),
    /// a victory screen you taught: the open fight is won
    Victory,
}

/// The last frames of `game` (oldest first), empty when it wasn't the game watched last.
pub fn recent(game: &str) -> Vec<Shot> {
    let r = RECENT.lock();
    if r.0 == game {
        r.1.iter().cloned().collect()
    } else {
        Vec::new()
    }
}

pub fn taught_changed() {
    TAUGHT.fetch_add(1, Ordering::Relaxed);
}

pub struct Watch {
    pub game: String,
    pub hwnd: isize,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
    paused: Arc<AtomicBool>,
}

/// paused this long (another window in front), the capture is closed until the game is back
const PAUSE_RELEASE: Duration = Duration::from_secs(30);

impl Watch {
    /// Watches `hwnd` for the game's `words` and the common death words, for the death screens
    /// taught for `game`, and for boss bars (`win`: the game's victory banners on top of the common
    /// ones); `on_seen` runs on the watcher's thread for every death and boss event seen.
    pub fn start(game: &str, hwnd: isize, words: &[String], win: &[String], on_seen: impl Fn(Seen) + Send + 'static) -> Watch {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(AtomicU8::new(STARTING));
        // the game's own words on top of the common ones: an unchecked preset never hides "YOU DIED"
        let known = !words.is_empty();
        let words: Vec<String> = words.iter().cloned().chain(detect::COMMON_WORDS.iter().map(|w| w.to_string())).collect();
        let win: Vec<String> = win.iter().cloned().chain(boss::WIN_WORDS.iter().map(|w| w.to_string())).collect();
        let paused = Arc::new(AtomicBool::new(false));
        let (s2, st2, g2, p2) = (stop.clone(), status.clone(), game.to_string(), paused.clone());
        {
            let mut r = RECENT.lock();
            if r.0 != game {
                *r = (game.to_string(), VecDeque::new());
            }
        }
        let _ = std::thread::Builder::new().name("deaths-screen".into()).spawn(move || {
            crate::util::com();
            watch(&g2, hwnd, &words, &win, known, &s2, &p2, &st2, &on_seen);
        });
        Watch { game: game.into(), hwnd, stop, status, paused }
    }

    pub fn status(&self) -> u8 {
        self.status.load(Ordering::Relaxed)
    }

    /// Another window is in front (an alt-tab): stop looking, keep everything for when the game
    /// is back (the capture itself is closed after PAUSE_RELEASE).
    pub fn pause(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn load_won(game: &str) -> Vec<teach::Taught> {
    super::store::load_won(game).iter().filter_map(teach::Taught::new).collect()
}

fn load_taught(game: &str) -> Vec<teach::Taught> {
    let mine = super::store::load_taught(game).into_iter().map(|p| (format!("taught:{}", p.made), p));
    let shared = super::library::prints(game).into_iter().map(|p| ("library".to_string(), p));
    mine.chain(shared)
        .filter_map(|(by, p)| {
            let mut t = teach::Taught::new(&p)?;
            t.by = by;
            Some(t)
        })
        .collect()
}

/// Frames in a row that may fail before the capture is opened again.
const BAD_FRAMES: u32 = 20;
/// how long to wait before trying to open a capture that failed
const REOPEN: Duration = Duration::from_secs(5);

/// The watcher's loop. It never gives up on a game that is still in front: a capture that can't
/// open (the window isn't ready, a mode switch) or breaks (the window shrank, the resolution
/// changed) is opened again every few seconds, until the deaths thread stops it.
#[allow(clippy::too_many_arguments)]
fn watch(game: &str, hwnd: isize, words: &[String], win: &[String], known: bool, stop: &AtomicBool, paused: &AtomicBool, status: &AtomicU8, on_seen: &dyn Fn(Seen)) {
    // no OCR language: taught death screens still work
    let ocr = ocr::Ocr::new();
    match &ocr {
        None => crate::log::warn("death counter: Windows OCR has no language installed; only taught death screens are seen"),
        Some(o) if !o.english => crate::log::info("death counter: Windows OCR has no English; English death screens may be missed"),
        _ => {}
    }
    let mut cap: Option<capture::Capture> = None;
    let mut bad = 0u32;
    let mut reopen_at = Instant::now();
    let mut logged = false;
    let mut gate = detect::Gate::default();
    let mut matcher = detect::Matcher::default();
    let mut taught_matcher = detect::Matcher::default();
    let mut gen = TAUGHT.load(Ordering::Relaxed);
    let mut taught = load_taught(game);
    // the watch from start to end in the log (the game by Deck's key for it, never what is on screen)
    struct Watching<'a>(&'a str, Instant);
    impl Drop for Watching<'_> {
        fn drop(&mut self) {
            crate::log::info(format!("death counter: stopped watching {} after {}", self.0, crate::log::took(self.1.elapsed())));
        }
    }
    let _watching = Watching(game, Instant::now());
    crate::log::info(format!("death counter: watching {game} ({} death words, {} taught screens, OCR {})", words.len(), taught.len(), if ocr.is_some() { "on" } else { "off" }));
    let mut won = load_won(game);
    let mut won_matcher = detect::Matcher::default();
    let mut boss = boss::BossWatch::default();
    // a death: recorded first (it reads the bar's health), then the try is over
    let died = |by: &str, boss: &mut boss::BossWatch| {
        on_seen(Seen::Death(by.to_string()));
        boss.death();
        BAR_NOW.lock().take();
    };
    let mut next = Instant::now();
    let mut paused_at: Option<Instant> = None;
    // text reads per minute, in the debug log: (boss names, each up to six reads; death text)
    let mut reads = (0u32, 0u32);
    let mut reads_since = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if paused.load(Ordering::Relaxed) {
            let at = *paused_at.get_or_insert_with(Instant::now);
            if cap.is_some() && at.elapsed() >= PAUSE_RELEASE {
                cap = None;
            }
            std::thread::sleep(Duration::from_millis(250));
            next = Instant::now();
            continue;
        }
        paused_at = None;
        let rate = if known || !taught.is_empty() { RATE } else { SLOW_RATE };
        next += Duration::from_secs_f64(1.0 / rate);
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now; // fell behind: no burst
        }
        if TAUGHT.load(Ordering::Relaxed) != gen {
            gen = TAUGHT.load(Ordering::Relaxed);
            taught = load_taught(game);
            won = load_won(game);
        }
        let now = Instant::now();
        if cap.is_none() {
            if now < reopen_at {
                continue;
            }
            match capture::Capture::new(hwnd, RATE) {
                Ok(c) => {
                    if logged {
                        crate::log::info("death counter: screen capture works again");
                    }
                    cap = Some(c);
                    bad = 0;
                    logged = false;
                }
                Err(e) => {
                    if !logged {
                        crate::log::warn(format!("death counter: screen capture didn't start: {e}"));
                        logged = true;
                    }
                    status.store(FAILED, Ordering::Relaxed);
                    reopen_at = now + REOPEN;
                    continue;
                }
            }
        }
        let Some(frame) = cap.as_mut().map(|c| c.grab()) else { continue };
        let frame = match frame {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                bad += 1;
                if bad >= BAD_FRAMES {
                    if !logged {
                        crate::log::warn(format!("death counter: screen capture stopped, opening it again: {e}"));
                        logged = true;
                    }
                    cap = None;
                    reopen_at = now + REOPEN;
                    status.store(FAILED, Ordering::Relaxed);
                }
                continue;
            }
        };
        bad = 0;
        let now = Instant::now();
        let thumb = capture::thumb(&frame);
        let aspect = aspect_of(frame.w, frame.h);
        {
            let mut r = RECENT.lock();
            let t = super::now_ms();
            if r.0 == game && r.1.back().is_none_or(|s| t.saturating_sub(s.t) >= KEEP_EVERY_MS) {
                while r.1.len() >= KEEP || r.1.front().is_some_and(|s| t.saturating_sub(s.t) > KEEP_MS) {
                    r.1.pop_front();
                }
                r.1.push_back(Shot { t, thumb: thumb.clone(), preview: capture::colour(&frame, PW, PH), aspect });
                drop(r);
                // the big picture to show where the boss's bar and name are: the last one with a bar
                if BOSSES.load(Ordering::Relaxed) {
                    let bars: Vec<bar::Spot> = bar::candidates(&frame).iter().map(|b| bar::Spot::of(b, frame.w, frame.h)).collect();
                    if !bars.is_empty() {
                        let h = (BIG_W * frame.h / frame.w.max(1)).max(1);
                        *BIG.lock() = Some(BigShot { game: game.to_string(), t, w: BIG_W, h, rgb: capture::colour(&frame, BIG_W, h), bars });
                    }
                }
            }
        }
        if !taught.is_empty() {
            let hit = taught.iter().find(|t| t.fits(aspect) && t.matches(&thumb));
            if taught_matcher.feed(hit.is_some(), now) {
                matcher.counted(now);
                died(hit.map_or("", |t| t.by.as_str()), &mut boss);
            }
        }
        if !won.is_empty() {
            let hit = won.iter().any(|t| t.fits(aspect) && t.matches(&thumb));
            if won_matcher.feed(hit, now) {
                boss.victory();
                BAR_NOW.lock().take();
                on_seen(Seen::Victory);
            }
        }
        if let Some(ocr) = ocr.as_ref().filter(|_| BOSSES.load(Ordering::Relaxed)) {
            let t = super::now_ms();
            let look = boss.look(&frame, spot_of(game).as_slice());
            let mut outs: Vec<boss::Out> = boss.step(look, t).into_iter().collect();
            if let Some(b) = boss.wants_name(t) {
                reads.0 += 1;
                let name = read_name(ocr, cap.as_mut(), &frame, &b, name_spot_of(game), &known_of(game));
                outs.extend(boss.named(name, t, &known_of(game)));
            }
            *BAR_NOW.lock() = boss.now().map(|(seen, fill, phase)| (game.to_string(), seen, fill, phase));
            for o in outs {
                on_seen(Seen::Boss(o));
            }
        }
        let read = if ocr.is_some() {
            gate.feed(&thumb, now, matcher.pending())
        } else {
            gate.feed(&thumb, now, false); // still notices a black capture
            false
        };
        if gate.moving() {
            matcher.moved(now);
        }
        if read {
            if let Some(ocr) = &ocr {
                reads.1 += 1;
                let (w, h, px) = capture::middle_for_ocr(&frame);
                let text = ocr.read(w, h, &px).unwrap_or_default();
                if matcher.feed(detect::is_death_text(&text, words), now) {
                    taught_matcher.counted(now);
                    died("words", &mut boss);
                } else if boss.in_try() && detect::is_death_text(&text, win) {
                    if let Some(o) = boss.victory() {
                        BAR_NOW.lock().take();
                        on_seen(Seen::Boss(o));
                    }
                }
            }
        }
        let state = if gate.blind(now) {
            BLIND
        } else if ocr.is_none() && taught.is_empty() {
            FAILED
        } else if ocr.as_ref().is_some_and(|o| !o.english) && taught.is_empty() {
            NO_ENGLISH
        } else {
            WATCHING
        };
        status.store(state, Ordering::Relaxed);
        if reads_since.elapsed() >= Duration::from_secs(60) {
            if reads != (0, 0) {
                crate::log::debug_with(|| format!("death counter: in the last minute, {} boss name reads and {} death text reads", reads.0, reads.1));
            }
            reads = (0, 0);
            reads_since = Instant::now();
        }
    }
}

/// A boss's name: where you showed it, else next to its bar (above it in most games, else below).
/// From the window at its full resolution when the capture has it (the frame watched is shrunk to
/// 1280 wide). Each place is read three times: as it is, cleaned to black letters on white with
/// one threshold, and with a threshold for each spot (letters over a bright sky and a dark wall
/// at once); the read like a known boss wins, else the one with the most letters.
fn read_name(ocr: &ocr::Ocr, mut cap: Option<&mut capture::Capture>, f: &capture::Frame, b: &bar::Bar, taught: Option<bar::Spot>, known: &[String]) -> Option<String> {
    let tall = ((b.y1 - b.y0) * 4).max(f.h * 6 / 100);
    let x0 = b.x0.saturating_sub(f.w / 50);
    let px = |v: u16, of: usize| v as usize * of / 1000;
    let places: Vec<((usize, usize, usize, usize), bool)> = match taught {
        Some(s) => vec![((px(s.x0, f.w), px(s.y0, f.h), px(s.x1, f.w), px(s.y1, f.h)), true)],
        None => vec![((x0, b.y0.saturating_sub(tall), b.x1, b.y0), true), ((x0, b.y1, b.x1, (b.y1 + tall).min(f.h)), false)],
    };
    let letters = |n: &String| n.chars().filter(|c| c.is_alphabetic()).count();
    let is_known = |n: &String| known.iter().any(|k| crate::deaths::book::same_boss(k, n) || crate::deaths::book::like_boss(k, n));
    places.into_iter().find_map(|(r, up)| {
        let share = |v: usize, of: usize| v as f32 / of.max(1) as f32;
        let full = cap.as_deref_mut().and_then(|c| c.area((share(r.0, f.w), share(r.1, f.h), share(r.2, f.w), share(r.3, f.h))).ok());
        let part = full.unwrap_or_else(|| capture::crop(f, r));
        let copy = capture::Frame { w: part.w, h: part.h, px: part.px.clone() };
        let copy2 = capture::Frame { w: part.w, h: part.h, px: part.px.clone() };
        let reads: Vec<String> = [capture::clean_text_for_ocr(copy, 96), capture::local_text_for_ocr(copy2, 96), capture::text_for_ocr(part, 96)]
            .into_iter()
            .filter_map(|(w, h, px)| ocr.lines(w, h, &px).ok())
            .filter_map(|lines| boss::pick_name(&lines, up))
            .collect();
        reads.into_iter().max_by_key(|n| (is_known(n), letters(n)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const PLAY: u8 = 0;
    const YOU_DIED: u8 = 1;
    /// a death screen without words: a red disc and a pale bar on black
    const SYMBOL: u8 = 2;
    /// a boss fight: a red health bar at the bottom, the boss's name above it
    const BOSS: u8 = 3;

    /// Deck's own test window (never the real screen), drawing what `mode` says. On screen
    /// (Windows composes nothing off every monitor) but behind everything, 1/255 opaque and
    /// click-through, so it can't be seen or clicked.
    fn test_window(mode: &'static AtomicU8) -> isize {
        let (tx, rx) = std::sync::mpsc::channel::<isize>();
        std::thread::spawn(move || unsafe {
            unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
                match m {
                    WM_TIMER => {
                        let _ = InvalidateRect(Some(h), None, false);
                        LRESULT(0)
                    }
                    WM_PAINT => {
                        let mut ps = PAINTSTRUCT::default();
                        let dc = BeginPaint(h, &mut ps);
                        let mut r = RECT::default();
                        let _ = GetClientRect(h, &mut r);
                        let mode = (GetWindowLongPtrW(h, GWLP_USERDATA) as *const AtomicU8).as_ref().map_or(PLAY, |m| m.load(Ordering::Relaxed));
                        let brush = CreateSolidBrush(COLORREF(if mode == PLAY { 0x0050_6070 } else { 0x0008_0808 }));
                        FillRect(dc, &r, brush);
                        let _ = DeleteObject(brush.into());
                        if mode == SYMBOL {
                            let red = CreateSolidBrush(COLORREF(0x0010_10C0));
                            let old = SelectObject(dc, red.into());
                            let _ = Ellipse(dc, 360, 150, 600, 390);
                            SelectObject(dc, old);
                            let _ = DeleteObject(red.into());
                            let pale = CreateSolidBrush(COLORREF(0x00D0_D0D0));
                            FillRect(dc, &RECT { left: 200, top: 420, right: 760, bottom: 450 }, pale);
                            let _ = DeleteObject(pale.into());
                        }
                        if mode == BOSS {
                            for (r, c) in [((200, 470, 760, 482), 0x0010_0E0E), ((202, 472, 600, 480), 0x0020_20B0), ((600, 472, 758, 480), 0x002C_2828)] {
                                let b = CreateSolidBrush(COLORREF(c));
                                FillRect(dc, &RECT { left: r.0, top: r.1, right: r.2, bottom: r.3 }, b);
                                let _ = DeleteObject(b.into());
                            }
                            let face = crate::util::wide("Segoe UI");
                            let font = CreateFontW(-22, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, PCWSTR(face.as_ptr()));
                            let old = SelectObject(dc, font.into());
                            SetBkMode(dc, TRANSPARENT);
                            SetTextColor(dc, COLORREF(0x00F0_F0F0));
                            let mut t: Vec<u16> = "Margit, the Fell Omen".encode_utf16().collect();
                            let mut at = RECT { left: 204, top: 440, right: 760, bottom: 468 };
                            DrawTextW(dc, &mut t, &mut at, DT_LEFT | DT_BOTTOM | DT_SINGLELINE);
                            SelectObject(dc, old);
                            let _ = DeleteObject(font.into());
                        }
                        if mode == YOU_DIED {
                            let face = crate::util::wide("Times New Roman");
                            let font = CreateFontW(-90, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, PCWSTR(face.as_ptr()));
                            let old = SelectObject(dc, font.into());
                            SetBkMode(dc, TRANSPARENT);
                            SetTextColor(dc, COLORREF(0x0000_10B0));
                            let mut t: Vec<u16> = "YOU DIED".encode_utf16().collect();
                            DrawTextW(dc, &mut t, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                            SelectObject(dc, old);
                            let _ = DeleteObject(font.into());
                        }
                        let _ = EndPaint(h, &ps);
                        LRESULT(0)
                    }
                    _ => DefWindowProcW(h, m, w, l),
                }
            }
            let inst = GetModuleHandleW(None).unwrap();
            let class = HSTRING::from("DeckDeathsScreenTest");
            let wc = WNDCLASSW { lpfnWndProc: Some(proc_), hInstance: inst.into(), lpszClassName: PCWSTR(class.as_ptr()), ..Default::default() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT, PCWSTR(class.as_ptr()), PCWSTR(HSTRING::from("deaths screen test").as_ptr()), WS_POPUP, 0, 0, 960, 540, None, None, Some(inst.into()), None).unwrap();
            SetWindowLongPtrW(h, GWLP_USERDATA, mode as *const AtomicU8 as isize);
            let _ = SetLayeredWindowAttributes(h, COLORREF(0), 1, LWA_ALPHA);
            let _ = SetWindowPos(h, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            SetTimer(Some(h), 1, 100, None);
            tx.send(h.0 as isize).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        });
        rx.recv().unwrap()
    }

    /// A window that can't be captured (gone, or not capturable) is reported and retried; the
    /// watcher thread stays up instead of ending for good.
    #[test]
    fn a_capture_that_cannot_open_is_reported_and_retried() {
        let w = Watch::start("test-gone", 0x7fff_fff0, &[], &[], |_| {});
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(w.status(), FAILED, "reported");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(w.status(), FAILED, "still reported, no crash");
        drop(w); // stops it
    }

    /// Opening and closing captures again and again (each switch back to the game) gives back
    /// what each one took: the driver's threads and the handles. Run it alone:
    /// `cargo test -- --ignored captures_opened_and_closed`
    #[test]
    #[ignore]
    fn captures_opened_and_closed_give_their_threads_back() {
        static MODE: AtomicU8 = AtomicU8::new(PLAY);
        let hwnd = test_window(&MODE);
        let threads = || {
            use windows::Win32::System::Diagnostics::ToolHelp::*;
            let me = std::process::id();
            let mut n = 0;
            unsafe {
                let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).unwrap();
                let mut e = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
                let mut ok = Thread32First(snap, &mut e).is_ok();
                while ok {
                    n += (e.th32OwnerProcessID == me) as u32;
                    ok = Thread32Next(snap, &mut e).is_ok();
                }
                let _ = windows::Win32::Foundation::CloseHandle(snap);
            }
            n
        };
        crate::util::com();
        // the first capture loads the driver and Windows' capture service: count after it
        drop(capture::Capture::new(hwnd, 4.0).expect("capture"));
        std::thread::sleep(Duration::from_millis(500));
        let before = threads();
        for _ in 0..8 {
            let mut c = capture::Capture::new(hwnd, 4.0).expect("capture");
            std::thread::sleep(Duration::from_millis(300));
            let _ = c.grab();
            drop(c);
        }
        std::thread::sleep(Duration::from_secs(2));
        let after = threads();
        assert!(after <= before + 4, "threads {before} -> {after}: each capture keeps some");
    }

    /// The whole path on a window of Deck's own: capture, gate, OCR, matcher. One death screen
    /// held for 4 s counts once. Run it alone (the frame buffer is shared):
    /// `cargo test -- --ignored a_drawn_death_screen`
    #[test]
    #[ignore]
    fn a_drawn_death_screen_counts_once() {
        static MODE: AtomicU8 = AtomicU8::new(PLAY);
        let hwnd = test_window(&MODE);
        let deaths = Arc::new(AtomicU32::new(0));
        let d2 = deaths.clone();
        let w = Watch::start("test", hwnd, &[], &[], move |s| {
            if matches!(s, Seen::Death(_)) {
                d2.fetch_add(1, Ordering::Relaxed);
            }
        });
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(deaths.load(Ordering::Relaxed), 0, "gameplay isn't a death");
        MODE.store(YOU_DIED, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(4));
        MODE.store(PLAY, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(2));
        assert_ne!(w.status(), FAILED, "capture failed");
        assert_eq!(deaths.load(Ordering::Relaxed), 1);
    }

    /// Boss fights on a window of Deck's own: a boss bar with a name opens the fight (the name read
    /// by Windows' OCR), and the death screen after it is counted with the boss's health. Run it
    /// alone: `cargo test -- --ignored a_drawn_boss_bar`
    #[test]
    #[ignore]
    fn a_drawn_boss_bar_starts_a_fight_and_a_death_ends_it() {
        static MODE: AtomicU8 = AtomicU8::new(PLAY);
        let hwnd = test_window(&MODE);
        let seen = Arc::new(Mutex::new(Vec::<Seen>::new()));
        let s2 = seen.clone();
        let w = Watch::start("boss-test", hwnd, &[], &[], move |s| {
            if let Seen::Death(_) = &s {
                // what the death would be recorded with
                s2.lock().push(Seen::Death(format!("{:?}", bar_now("boss-test", super::super::now_ms()))));
            } else {
                s2.lock().push(s);
            }
        });
        std::thread::sleep(Duration::from_secs(2));
        MODE.store(BOSS, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(5));
        assert_ne!(w.status(), FAILED, "capture failed");
        let got = seen.lock().clone();
        assert!(matches!(got.first(), Some(Seen::Boss(boss::Out::Engage { name, phase: 1, .. })) if same_name(name)), "the fight opens when the bar shows: {got:?}");
        MODE.store(YOU_DIED, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(4));
        MODE.store(PLAY, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(1));
        let got = seen.lock().clone();
        assert!(got.iter().any(|s| matches!(s, Seen::Death(hp) if hp.starts_with("Some((7"))), "the death has the boss at about 7x%: {got:?}");
        assert_eq!(got.iter().filter(|s| matches!(s, Seen::Boss(_))).count(), 1, "one engage, no rename (the reads agree): {got:?}");
    }

    fn same_name(n: &str) -> bool {
        crate::deaths::book::same_boss(n, "Margit, the Fell Omen")
    }

    /// Teaching on real captured frames: a death screen with no words isn't counted, is learned
    /// from the last frames, and the fingerprint then matches it and not normal play. Run it
    /// alone: `cargo test -- --ignored a_wordless_death_screen`
    #[test]
    #[ignore]
    fn a_wordless_death_screen_is_learned_from_captured_frames() {
        static MODE: AtomicU8 = AtomicU8::new(PLAY);
        let hwnd = test_window(&MODE);
        let deaths = Arc::new(AtomicU32::new(0));
        let d2 = deaths.clone();
        let _w = Watch::start("teach-test", hwnd, &[], &[], move |s| {
            if matches!(s, Seen::Death(_)) {
                d2.fetch_add(1, Ordering::Relaxed);
            }
        });
        std::thread::sleep(Duration::from_secs(4));
        MODE.store(SYMBOL, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(deaths.load(Ordering::Relaxed), 0, "no words: OCR can't see it");
        let shots = recent("teach-test");
        assert!(shots.len() >= 20, "only {} frames kept", shots.len());
        let thumbs: Vec<Vec<u8>> = shots.iter().map(|s| s.thumb.clone()).collect();
        let print = teach::learn(&thumbs, thumbs.len() - 2, 1).expect("learned");
        let t = teach::Taught::new(&print).unwrap();
        assert!(t.matches(thumbs.last().unwrap()), "the death screen");
        assert!(!t.matches(&thumbs[2]), "normal play");
        MODE.store(PLAY, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(1));
        assert!(!t.matches(&recent("teach-test").last().unwrap().thumb), "normal play again");
        MODE.store(SYMBOL, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(1));
        assert!(t.matches(&recent("teach-test").last().unwrap().thumb), "the next death");
    }
}
