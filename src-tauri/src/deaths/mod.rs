//! The death counter: deaths per game and per boss, in any game. See
//! `docs/superpowers/specs/2026-10-19-death-counter-design.md`.
//!
//! One thread looks at the game in front every second and draws the in-game pill; actions
//! (tray, page, hotkeys) come in through `act`. Every event is saved before it's shown.

pub mod book;
pub mod catalog;
pub mod cfg;
pub mod combo;
pub mod ident;
pub mod library;
pub mod pill;
pub mod save;
pub mod screen;
pub mod store;

use crate::app::App;
use book::{Book, Ev, Event};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

const FLASH: Duration = Duration::from_millis(1600);
/// how often the controller combo reads the pad (only while it's on and a game is in front)
const COMBO_POLL: Duration = Duration::from_millis(50);
/// a death counted by hand this long after the game's last frame still gets a teaching strip
const STRIP_FRESH_MS: u64 = 60_000;
/// a death from one way of counting this soon after one from another (your +1 key while the screen
/// also saw it) is the same death
const SAME_DEATH_MS: u64 = 10_000;
const NO_GAME: &str = "No game yet: start a game first, then count.";

/// The error `act` gives when no game has been seen yet.
pub fn no_game_yet(e: &str) -> bool {
    e == NO_GAME
}

fn automatic(src: &str) -> bool {
    src == "screen" || src == "save"
}

/// A death from `src` at `t` is one already counted: the last death of this game came from
/// another way of counting a moment ago, and one of the two was automatic. Two presses of your own
/// key are two deaths.
fn same_death(last: Option<&(String, u64, String)>, key: &str, t: u64, src: &str) -> bool {
    last.is_some_and(|(k, at, s)| k == key && t.saturating_sub(*at) < SAME_DEATH_MS && s != src && (automatic(s) || automatic(src)))
}

struct Log {
    events: Vec<Event>,
    book: Book,
}

#[derive(Default)]
struct St {
    games: BTreeMap<String, store::GameInfo>,
    logs: HashMap<String, Log>,
    /// the game in front now
    front: Option<ident::Front>,
    /// the last game seen in front (the tray is opened from the taskbar, so "now" is gone)
    seen: Option<ident::Game>,
    flash: Option<(String, String, Instant)>,
    /// how a game is counted automatically right now: (game key, "save" | "save_missing" |
    /// "screen" | "screen_blind" | "screen_failed")
    auto: Option<(String, &'static str)>,
    /// the frames before the last death you counted yourself: (game, when, frames), to teach from
    strip: Option<(String, u64, Vec<screen::Shot>)>,
    /// the last death counted: (game, when, where from), so one death seen two ways counts once
    last_death: Option<(String, u64, String)>,
}

pub struct Shared {
    app: Weak<App>,
    st: Mutex<St>,
    wake: Mutex<Option<Sender<()>>>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl St {
    fn log(&mut self, key: &str) -> &mut Log {
        self.logs.entry(key.to_string()).or_insert_with(|| {
            let events = store::load(key);
            let book = Book::replay(&events);
            Log { events, book }
        })
    }

    fn name_of(&self, key: &str) -> String {
        if let Some(g) = self.front.as_ref().map(|f| &f.game).filter(|g| g.key == key).or(self.seen.as_ref().filter(|g| g.key == key)) {
            return g.name.clone();
        }
        self.games.get(key).map(|g| g.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| key.to_string())
    }

    /// The game an action goes to: the one named, else the game in front (or last in front),
    /// else the one with an open fight, else the last one counted.
    fn target(&mut self, game: Option<&str>) -> Option<String> {
        if let Some(g) = game.filter(|g| !g.is_empty()) {
            return Some(g.to_string());
        }
        if let Some(g) = self.front.as_ref().map(|f| f.game.key.clone()).or_else(|| self.seen.as_ref().map(|g| g.key.clone())) {
            return Some(g);
        }
        let keys: Vec<String> = self.games.keys().cloned().collect();
        let mut open: Vec<(u64, String)> = vec![];
        for k in keys {
            let last = self.games.get(&k).map(|g| g.last).unwrap_or(0);
            if self.log(&k).book.open().is_some() {
                open.push((last, k));
            }
        }
        open.into_iter().max().map(|x| x.1).or_else(|| self.games.iter().max_by_key(|g| g.1.last).map(|g| g.0.clone()))
    }

    /// Text for the pill: "Malenia · 14" in a fight, "Deaths · 52" otherwise; None when this
    /// game has nothing to show and the pill isn't wanted everywhere.
    fn pill_text(&mut self, key: &str, always: bool) -> Option<String> {
        let log = self.log(key);
        let total = log.book.total_deaths();
        match log.book.open() {
            Some(f) => Some(format!("{}  ·  {}", f.boss, f.deaths)),
            None if total > 0 || always => Some(format!("Deaths  ·  {total}")),
            None => None,
        }
    }
}

pub fn shared() -> Option<&'static Arc<Shared>> {
    SHARED.get()
}

fn wake() {
    if let Some(sh) = shared() {
        if let Some(tx) = sh.wake.lock().as_ref() {
            let _ = tx.send(());
        }
    }
}

/// Records an action. `what`: "death", "undo", "start" (needs `boss`), "win", "abandon",
/// "rename" (`boss` = new name for the open fight), "off" / "on" (count this game or not),
/// "delete:<index>" (a history line, oldest = 0). `src`: "manual", "hotkey", "pad", "save", "screen".
pub fn act(app: &App, what: &str, boss: Option<&str>, game: Option<&str>, src: &str) -> Result<(), String> {
    let Some(sh) = shared() else { return Err("the death counter isn't running".into()) };
    let off_games = app.settings.lock().deaths.off_games.clone();
    let mut st = sh.st.lock();
    let key = st.target(game).ok_or(NO_GAME)?;
    if refused(&off_games, &key, what) {
        return Err("Counting is turned off for this game.".into());
    }
    if what == "off" || what == "on" {
        drop(st);
        {
            let mut s = app.settings.lock();
            s.deaths.off_games.retain(|k| *k != key);
            if what == "off" {
                s.deaths.off_games.push(key.clone());
            }
        }
        app.save_settings();
        app.changed("deaths");
        wake();
        return Ok(());
    }
    let t = now_ms();
    if what == "death" {
        if same_death(st.last_death.as_ref(), &key, t, src) {
            // counted already; the next one from either way counts again
            st.last_death = Some((key.clone(), t, "both".into()));
            st.flash = Some((key.clone(), "Already counted".into(), Instant::now()));
            drop(st);
            wake();
            return Ok(());
        }
        st.last_death = Some((key.clone(), t, src.to_string()));
    }
    let boss = boss.map(|b| b.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
    let open = st.log(&key).book.open().map(|f| (f.id.clone(), f.boss.clone()));
    let (ev, flash) = match what {
        "death" => {
            let mut e = Event::new(t, Ev::Death);
            e.src = src.into();
            (Some(e), "+1")
        }
        "undo" => {
            if !st.log(&key).book.can_undo() {
                return Err("Nothing to undo.".into());
            }
            (Some(Event { src: src.into(), ..Event::new(t, Ev::Undo) }), "Undone")
        }
        "start" => {
            if boss.is_empty() {
                return Err("Give the boss a name.".into());
            }
            (Some(Event { fight: format!("f{t}"), boss: boss.clone(), ..Event::new(t, Ev::FightStart) }), "Fight")
        }
        "win" | "abandon" => {
            if open.is_none() {
                return Err("No fight is open.".into());
            }
            (Some(Event::new(t, if what == "win" { Ev::Win } else { Ev::Abandon })), if what == "win" { "Defeated" } else { "Abandoned" })
        }
        "rename" => {
            let Some((id, _)) = open.clone() else { return Err("No fight is open.".into()) };
            if boss.is_empty() {
                return Err("Give the boss a name.".into());
            }
            (Some(Event { fight: id, boss: boss.clone(), ..Event::new(t, Ev::Rename) }), "")
        }
        w if w.starts_with("delete:") => {
            let i: usize = w[7..].parse().map_err(|_| "bad history line")?;
            let log = st.log(&key);
            if i >= log.events.len() {
                return Err("That line is gone already.".into());
            }
            log.events.remove(i);
            log.book = Book::replay(&log.events);
            (None, "")
        }
        _ => return Err(format!("unknown death counter action {what}")),
    };
    let name = st.name_of(&key);
    let log = st.log(&key);
    if let Some(e) = ev {
        log.book.apply(&e);
        log.events.push(e);
    }
    store::save(&key, &log.events);
    let text = match log.book.open() {
        Some(f) => format!("{}  ·  {}", f.boss, f.deaths),
        None => format!("Deaths  ·  {}", log.book.total_deaths()),
    };
    st.games.insert(key.clone(), store::GameInfo { name, last: t });
    store::save_games(&st.games);
    if what == "death" && src != "screen" && src != "save" {
        // a death Deck didn't see: keep the frames before it, so it can be taught
        let shots = screen::recent(&key);
        if shots.last().is_some_and(|s| t.saturating_sub(s.t) < STRIP_FRESH_MS) {
            st.strip = Some((key.clone(), t, shots));
        }
    }
    if !flash.is_empty() {
        let who = if boss.is_empty() { open.map(|o| o.1).unwrap_or_else(|| text.clone()) } else { boss.clone() };
        let shown = if flash == "+1" || flash == "Undone" { format!("{flash}  ·  {text}") } else { format!("{flash}  ·  {who}") };
        st.flash = Some((key.clone(), shown, Instant::now()));
    }
    drop(st);
    app.changed("deaths");
    wake();
    Ok(())
}

/// A game turned off isn't counted: no death and no new fight, whatever the source (the shortcuts
/// go straight to `act`). Undo, editing the history and turning it back on still work.
fn refused(off_games: &[String], key: &str, what: &str) -> bool {
    matches!(what, "death" | "start") && off_games.iter().any(|k| k == key)
}

// ------------------------------------------------------------------ taught death screens

fn png(rgb: &[u8], w: usize, h: usize) -> String {
    let mut out = std::io::Cursor::new(Vec::new());
    let ok = image::RgbImage::from_raw(w as u32, h as u32, rgb.to_vec()).is_some_and(|img| img.write_to(&mut out, image::ImageFormat::Png).is_ok());
    if ok {
        format!("data:image/png;base64,{}", crate::util::base64(out.get_ref()))
    } else {
        String::new()
    }
}

/// The frames before the last death you counted in `game`, for the page to pick from.
pub fn strip(game: &str) -> Value {
    let Some(sh) = shared() else { return json!(null) };
    let st = sh.st.lock();
    match &st.strip {
        Some((g, at, shots)) if g == game => json!({
            "at": at,
            "frames": shots.iter().map(|s| json!({ "t": s.t, "img": png(&s.preview, screen::PW, screen::PH) })).collect::<Vec<_>>(),
        }),
        _ => json!(null),
    }
}

/// Learns the death screen in frame `pick` of `game`'s strip.
pub fn teach(app: &App, game: &str, pick: usize) -> Result<(), String> {
    let sh = shared().ok_or("the death counter isn't running")?;
    let mut st = sh.st.lock();
    let Some((g, _, shots)) = st.strip.as_ref().filter(|(g, _, _)| g == game) else { return Err("Those pictures are gone; count the next death and try again.".into()) };
    let mut prints = store::load_taught(g);
    if prints.len() >= screen::teach::MAX_PER_GAME {
        return Err(format!("This game already has {} taught death screens. Forget one first.", screen::teach::MAX_PER_GAME));
    }
    let thumbs: Vec<Vec<u8>> = shots.iter().map(|s| s.thumb.clone()).collect();
    let mut print = screen::teach::learn(&thumbs, pick, now_ms())?;
    // a fingerprint only fits pictures of the same shape (16:9, 21:9, ...)
    print.aspect = shots.get(pick).map_or(0, |s| s.aspect);
    prints.push(print);
    store::save_taught(game, &prints)?;
    st.strip = None;
    drop(st);
    screen::taught_changed();
    app.changed("deaths");
    Ok(())
}

/// Forgets taught death screen `i` of `game`.
pub fn forget(app: &App, game: &str, i: usize) -> Result<(), String> {
    let mut prints = store::load_taught(game);
    if i >= prints.len() {
        return Err("That one is gone already.".into());
    }
    prints.remove(i);
    store::save_taught(game, &prints)?;
    screen::taught_changed();
    app.changed("deaths");
    Ok(())
}

/// A code to share `game`'s taught death screens with a friend (or the shared library).
pub fn share(game: &str) -> Result<String, String> {
    let prints = store::load_taught(game);
    if prints.is_empty() {
        return Err("Nothing taught for this game yet.".into());
    }
    let name = shared().map(|sh| sh.st.lock().name_of(game)).unwrap_or_else(|| game.to_string());
    Ok(screen::teach::share(game, &name, &prints))
}

/// Adds the death screens in a friend's code to their game; returns the game's name.
pub fn import(app: &App, code: &str) -> Result<String, String> {
    let (game, name, new) = screen::teach::unshare(code)?;
    let mut prints = store::load_taught(&game);
    let before = prints.len();
    for p in new {
        if prints.len() < screen::teach::MAX_PER_GAME && !prints.iter().any(|q| q.mask == p.mask && q.values == p.values) {
            prints.push(p);
        }
    }
    if prints.len() == before {
        return Err(format!("{name} already has these (or {} taught death screens).", screen::teach::MAX_PER_GAME));
    }
    store::save_taught(&game, &prints)?;
    if let Some(sh) = shared() {
        let mut st = sh.st.lock();
        st.games.entry(game.clone()).or_insert_with(|| store::GameInfo { name: name.clone(), last: now_ms() });
        store::save_games(&st.games);
    }
    screen::taught_changed();
    app.changed("deaths");
    Ok(name)
}

/// What the Deaths page and the tray show. `game`: the page is looking at that game.
pub fn state(app: &App, game: Option<&str>) -> Value {
    let settings = app.settings.lock().deaths.clone();
    let Some(sh) = shared() else { return json!({ "running": false, "settings": settings }) };
    let mut st = sh.st.lock();
    let front = st.front.as_ref().map(|f| f.game.clone()).or_else(|| st.seen.clone());
    let key = st.target(game);
    let now = now_ms();
    let mut games: Vec<Value> = vec![];
    let keys: Vec<(String, store::GameInfo)> = st.games.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, info) in &keys {
        let log = st.log(k);
        games.push(json!({ "key": k, "name": info.name, "last": info.last, "deaths": log.book.total_deaths(), "fighting": log.book.open().is_some(), "off": settings.off_games.contains(k) }));
    }
    games.sort_by(|a, b| b["last"].as_u64().cmp(&a["last"].as_u64()));
    let current = key.as_ref().map(|k| {
        let name = st.name_of(k);
        let entry = front.as_ref().filter(|g| g.key == *k).and_then(|g| g.entry).or_else(|| catalog::by_key(k));
        let log = st.log(k);
        let b = &log.book;
        let history: Vec<Value> = log.events.iter().enumerate().rev().take(300).map(|(i, e)| json!({ "i": i, "t": e.t, "ev": e.ev, "boss": e.boss, "src": e.src })).collect();
        json!({
            "key": k,
            "name": name,
            "kind": entry.map(|e| e.kind.as_str()).unwrap_or("boss"),
            "known": entry.is_some(),
            "total": b.total_deaths(),
            "loose": b.loose,
            "open": b.open().map(|f| json!({ "boss": f.boss, "deaths": f.deaths, "started": f.started })),
            "bosses": b.bosses(now),
            "names": b.boss_names(),
            "can_undo": b.can_undo(),
            "history": history,
            "off": settings.off_games.contains(k),
            "save": entry.map(|e| e.save.as_str()).filter(|s| save::known(s)),
            "auto": st.auto.as_ref().filter(|(g, _)| g == k).map(|(_, a)| *a),
            "taught": store::load_taught(k).iter().map(|p| p.made).collect::<Vec<_>>(),
            "library": library::count(k),
            "can_submit": library::good_key(k),
            "strip": st.strip.as_ref().filter(|(g, _, _)| g == k).map(|(_, at, _)| *at),
        })
    });
    json!({
        "running": true,
        "settings": settings,
        "front": front.map(|g| json!({ "key": g.key, "name": g.name, "known": g.entry.is_some() })),
        "game": current,
        "games": games,
        "catalog": catalog::all().len(),
        "library": { "games": library::totals().0, "prints": library::totals().1, "issue_url": library::ISSUE_URL },
        "presets": catalog::all().iter().filter(|e| !e.words.is_empty() || save::known(&e.save)).map(|e| json!({ "name": e.name, "save": e.save, "words": e.words, "verified": e.verified })).collect::<Vec<_>>(),
    })
}

/// For the tray: (game name, deaths, open fight boss, last 5 bosses, can undo, counted).
pub fn tray_info(app: &App) -> Option<(String, u32, Option<String>, Vec<String>, bool, bool)> {
    let sh = shared()?;
    let off = app.settings.lock().deaths.off_games.clone();
    let mut st = sh.st.lock();
    let key = st.target(None)?;
    let name = st.name_of(&key);
    let log = st.log(&key);
    let names: Vec<String> = log.book.boss_names().into_iter().take(5).collect();
    Some((name, log.book.total_deaths(), log.book.open().map(|f| f.boss.clone()), names, log.book.can_undo(), !off.contains(&key)))
}

/// Starts the death counter's thread (once).
pub fn start(app: &Arc<App>) {
    if SHARED.get().is_some() {
        return;
    }
    let (tx, rx) = channel();
    let sh = Arc::new(Shared { app: Arc::downgrade(app), st: Mutex::new(St { games: store::load_games(), ..St::default() }), wake: Mutex::new(Some(tx)) });
    let _ = SHARED.set(sh.clone());
    let _ = std::thread::Builder::new().name("deaths".into()).spawn(move || run(sh, rx));
}

fn pump() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
    unsafe {
        let mut m: MSG = std::mem::zeroed();
        while PeekMessageW(&mut m, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

fn run(sh: Arc<Shared>, rx: Receiver<()>) {
    let mut pill = pill::Pill::new();
    let mut last_front: Option<String> = None;
    // the controller combo: its own passive reader, only while a counted game is in front
    let mut reader: Option<crate::pad::input::Reader> = None;
    let mut combo = combo::Combo::default();
    let mut combo_on = false;
    let mut next_look = Instant::now();
    let mut tracker: Option<save::Tracker> = None;
    let mut watch: Option<screen::Watch> = None;
    loop {
        pump();
        let Some(app) = sh.app.upgrade() else { return };
        let now = Instant::now();
        if now >= next_look {
            let (s, theme) = {
                let s = app.settings.lock();
                (s.deaths.clone().clamped(), s.pad.theme.clone())
            };
            let front = if s.on { ident::front() } else { None };
            let counted = front.as_ref().is_some_and(|f| !s.off_games.contains(&f.game.key));
            combo_on = s.pad_combo && counted;
            let shared = s.on && s.community_library;
            if library::set_enabled(shared) {
                screen::taught_changed();
            }
            library::maybe_refresh(shared);
            // All file reading happens here, before the lock below: the save file can be several
            // MB and a slow disk must not freeze the page and the tray, which take that lock too.
            // The save reader follows the game in front, and stays while nothing else is.
            let reader = front.as_ref().filter(|_| counted && s.auto_save).and_then(|f| f.game.entry.map(|e| (f, e))).filter(|(_, e)| save::known(&e.save));
            match (&reader, &front) {
                (Some((f, e)), _) if tracker.as_ref().is_none_or(|t| t.game != f.game.key) => {
                    let mut t = save::Tracker::new(&f.game.key, &e.save, &f.game.dir);
                    t.trusted = e.verified;
                    tracker = Some(t);
                }
                (None, Some(_)) => tracker = None,
                _ => {}
            }
            if !s.auto_save || !s.on {
                tracker = None;
            }
            let mut save_deaths = 0;
            if let Some(t) = &mut tracker {
                save_deaths = t.poll();
            }
            // the screen watcher: the game in front, unless its save file is counting it (a read
            // that hits a half-written file for a moment doesn't hand the game to the screen). A save
            // reader not checked in the real game only takes over once it has counted a death: one
            // that reads the wrong place would otherwise silence a screen watcher that works.
            let save_ok = |k: &str| tracker.as_ref().is_some_and(|t| t.game == k && t.proven());
            let looking = front.as_ref().filter(|f| counted && s.auto_screen && !save_ok(&f.game.key));
            match looking {
                Some(f) if watch.as_ref().is_none_or(|w| w.game != f.game.key || w.hwnd != f.hwnd) => {
                    let (weak, key) = (sh.app.clone(), f.game.key.clone());
                    let words = f.game.entry.map(|e| e.words.clone()).unwrap_or_default();
                    // games Deck knows are watched 4 times a second, any other 2 (enough for a
                    // banner that stays up, and half the work in games that may never show one)
                    watch = Some(screen::Watch::start(&f.game.key, f.hwnd, &words, move || {
                        if let Some(a) = weak.upgrade() {
                            let _ = act(&a, "death", None, Some(&key), "screen");
                        }
                    }));
                }
                None => watch = None,
                _ => {}
            }
            let mut st = sh.st.lock();
            let key = front.as_ref().map(|f| f.game.key.clone());
            if key != last_front {
                last_front = key.clone();
                drop(st);
                app.ui_changed("deaths");
                st = sh.st.lock();
            }
            if let Some(f) = &front {
                st.seen = Some(f.game.clone());
            }
            st.front = front.clone();
            let flash = st.flash.clone().filter(|(_, _, at)| at.elapsed() < FLASH);
            let mut flashing = false;
            // nothing watched because Deck itself is in front: keep what the page last saw
            let keep = front.is_none() && watch.is_none();
            let was = st.auto.take();
            st.auto = if keep { was } else { match (&tracker, &watch) {
                (Some(t), _) if t.proven() => Some((t.game.clone(), "save")),
                (_, Some(w)) => Some((w.game.clone(), match w.status() {
                    screen::BLIND => "screen_blind",
                    screen::FAILED => "screen_failed",
                    screen::NO_ENGLISH => "screen_lang",
                    _ => "screen",
                })),
                (Some(t), None) => Some((t.game.clone(), if t.working() { "save" } else { "save_missing" })),
                (None, None) => None,
            } };
            match &front {
                Some(f) if s.pill && counted => {
                    let text = match &flash {
                        Some((k, text, _)) if *k == f.game.key => {
                            flashing = true;
                            Some(text.clone())
                        }
                        _ => st.pill_text(&f.game.key, s.pill_always),
                    };
                    drop(st);
                    match text {
                        Some(t) => pill.show(&t, flashing, f.monitor, &s, &theme),
                        None => pill.hide(),
                    }
                }
                _ => {
                    drop(st);
                    pill.hide();
                }
            }
            if let Some(t) = tracker.as_ref().filter(|_| save_deaths > 0) {
                for _ in 0..save_deaths {
                    let _ = act(&app, "death", None, Some(&t.game), "save");
                }
            }
            next_look = Instant::now() + if flashing { Duration::from_millis(200) } else { Duration::from_secs(1) };
        }
        if combo_on {
            let r = reader.get_or_insert_with(crate::pad::input::Reader::new);
            let pad = r.read();
            if let Some(what) = combo.feed(pad.as_ref(), Instant::now()) {
                let _ = act(&app, what, None, None, "pad");
            }
        } else if reader.is_some() {
            reader = None; // lets go of the pad
            combo = combo::Combo::default();
        }
        drop(app);
        let wait = next_look.saturating_duration_since(Instant::now());
        let wait = if combo_on { wait.min(COMBO_POLL) } else { wait };
        match rx.recv_timeout(wait) {
            Ok(()) => next_look = Instant::now(), // an action: redraw at once
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_turned_off_is_not_counted_from_any_source() {
        let off = vec!["exe:eldenring".to_string()];
        assert!(refused(&off, "exe:eldenring", "death"), "the +1 shortcut");
        assert!(refused(&off, "exe:eldenring", "start"));
        assert!(!refused(&off, "exe:eldenring", "undo"), "taking a count back is fine");
        assert!(!refused(&off, "exe:eldenring", "on"), "turning it back on must work");
        assert!(!refused(&off, "exe:eldenring", "delete:3"), "editing the history too");
        assert!(!refused(&off, "exe:other", "death"), "another game");
        assert!(!refused(&[], "exe:eldenring", "death"));
    }

    #[test]
    fn one_death_seen_two_ways_counts_once() {
        let last = |src: &str| Some(("exe:game1x".to_string(), 1_000u64, src.to_string()));
        assert!(same_death(last("screen").as_ref(), "exe:game1x", 4_000, "hotkey"), "your key after the screen saw it");
        assert!(same_death(last("manual").as_ref(), "exe:game1x", 4_000, "screen"), "the screen after you counted it");
        assert!(same_death(last("save").as_ref(), "exe:game1x", 4_000, "pad"));
        assert!(!same_death(last("hotkey").as_ref(), "exe:game1x", 4_000, "hotkey"), "two presses are two deaths");
        assert!(!same_death(last("hotkey").as_ref(), "exe:game1x", 4_000, "pad"), "two ways by hand: you meant both");
        assert!(!same_death(last("screen").as_ref(), "exe:game1x", 20_000, "hotkey"), "long after: another death");
        assert!(!same_death(last("screen").as_ref(), "exe:game2x", 4_000, "hotkey"), "another game");
        assert!(!same_death(last("both").as_ref(), "exe:game1x", 4_000, "hotkey"), "after a merge the next press counts");
        assert!(!same_death(None, "exe:game1x", 4_000, "screen"));
    }
}
