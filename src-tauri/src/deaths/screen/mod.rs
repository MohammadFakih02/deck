//! Screen detection (stage 3): while a counted game is in front, its window is captured 4 times
//! a second and Windows' OCR reads the middle when the picture settles; the game's own death
//! words (catalogue) and the common ones, seen twice in a row, count a death. Death screens you
//! taught Deck (`teach.rs`) are matched on every frame without OCR.
//! Started and stopped by the deaths thread; one watcher at a time.
//!
//! The last 10 s of small frames stay in memory (`RECENT`) so a death you counted yourself can be
//! taught from them; they're never written anywhere.

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
/// frames kept for teaching: the last 10 s (at most this many)
const KEEP: usize = 40;
const KEEP_MS: u64 = 10_000;
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

/// width / height x 1000
pub fn aspect_of(w: usize, h: usize) -> u32 {
    (w * 1000).checked_div(h).map_or(0, |a| a as u32)
}

/// The game being watched and its last frames.
static RECENT: Mutex<(String, VecDeque<Shot>)> = Mutex::new((String::new(), VecDeque::new()));
/// bumped when a game's taught death screens change, so a running watcher reloads them
static TAUGHT: AtomicU32 = AtomicU32::new(0);

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
}

impl Watch {
    /// Watches `hwnd` for the game's `words` and the common death words, and for the death
    /// screens taught for `game`; `on_death` runs on the watcher's thread for every death seen.
    pub fn start(game: &str, hwnd: isize, words: &[String], on_death: impl Fn() + Send + 'static) -> Watch {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(AtomicU8::new(STARTING));
        // the game's own words on top of the common ones: an unchecked preset never hides "YOU DIED"
        let known = !words.is_empty();
        let words: Vec<String> = words.iter().cloned().chain(detect::COMMON_WORDS.iter().map(|w| w.to_string())).collect();
        let (s2, st2, g2) = (stop.clone(), status.clone(), game.to_string());
        {
            let mut r = RECENT.lock();
            if r.0 != game {
                *r = (game.to_string(), VecDeque::new());
            }
        }
        let _ = std::thread::Builder::new().name("deaths-screen".into()).spawn(move || {
            crate::util::com();
            watch(&g2, hwnd, &words, known, &s2, &st2, &on_death);
        });
        Watch { game: game.into(), hwnd, stop, status }
    }

    pub fn status(&self) -> u8 {
        self.status.load(Ordering::Relaxed)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn load_taught(game: &str) -> Vec<teach::Taught> {
    let mut prints = super::store::load_taught(game);
    prints.extend(super::library::prints(game));
    prints.iter().filter_map(teach::Taught::new).collect()
}

/// Frames in a row that may fail before the capture is opened again.
const BAD_FRAMES: u32 = 20;
/// how long to wait before trying to open a capture that failed
const REOPEN: Duration = Duration::from_secs(5);

/// The watcher's loop. It never gives up on a game that is still in front: a capture that can't
/// open (the window isn't ready, a mode switch) or breaks (the window shrank, the resolution
/// changed) is opened again every few seconds, until the deaths thread stops it.
fn watch(game: &str, hwnd: isize, words: &[String], known: bool, stop: &AtomicBool, status: &AtomicU8, on_death: &dyn Fn()) {
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
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
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
        }
        let now = Instant::now();
        if cap.is_none() {
            if now < reopen_at {
                continue;
            }
            match capture::Capture::new(hwnd, RATE) {
                Ok(c) => {
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
            if r.0 == game {
                let t = super::now_ms();
                while r.1.len() >= KEEP || r.1.front().is_some_and(|s| t.saturating_sub(s.t) > KEEP_MS) {
                    r.1.pop_front();
                }
                r.1.push_back(Shot { t, thumb: thumb.clone(), preview: capture::grey(&frame, PW, PH), aspect });
            }
        }
        if !taught.is_empty() && taught_matcher.feed(taught.iter().any(|t| t.fits(aspect) && t.matches(&thumb)), now) {
            matcher.counted(now);
            on_death();
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
                let (w, h, px) = capture::middle(&frame);
                let text = ocr.read(w, h, &px).unwrap_or_default();
                if matcher.feed(detect::is_death_text(&text, words), now) {
                    taught_matcher.counted(now);
                    on_death();
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
    }
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
        let w = Watch::start("test-gone", 0x7fff_fff0, &[], || {});
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(w.status(), FAILED, "reported");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(w.status(), FAILED, "still reported, no crash");
        drop(w); // stops it
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
        let w = Watch::start("test", hwnd, &[], move || {
            d2.fetch_add(1, Ordering::Relaxed);
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
        let _w = Watch::start("teach-test", hwnd, &[], move || {
            d2.fetch_add(1, Ordering::Relaxed);
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
