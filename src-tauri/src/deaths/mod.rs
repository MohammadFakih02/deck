//! The death counter: deaths per game and per boss, in any game. See
//! `docs/superpowers/specs/2026-10-19-death-counter-design.md`.
//!
//! One thread looks at the game in front every second and draws the in-game pill; actions
//! (tray, page, hotkeys) come in through `act`. Every event is saved before it's shown.

pub mod book;
pub mod catalog;
pub mod cfg;
pub mod combo;
pub mod edge;
pub mod ident;
pub mod library;
pub mod look;
pub mod pill;
pub mod save;
pub mod screen;
pub mod stats;
pub mod store;
pub mod view;

use crate::app::App;
use crate::pad::art;
use book::boss_key;
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
    /// the play session going on now
    playing: Option<Playing>,
    /// totals across every game, worked out at most every 10 s (or when a game changes)
    overall: Option<(Instant, u64, Value)>,
    /// the card after a death: (game, when, what it shows)
    card: Option<(String, Instant, look::Card)>,
    /// a red edge to show (a death a moment ago)
    edge: Option<Instant>,
}

/// The deaths the last beaten boss in this game took: the ring style fills toward it.
fn ring_mark(b: &Book) -> Option<u32> {
    let boss = boss_key(&b.fights.get(b.wins.last()?.1)?.boss);
    b.bosses(0).into_iter().find(|s| boss_key(&s.boss) == boss).and_then(|s| s.deaths_to_first_win)
}

/// A play session being recorded: a counted game in front since `start`, last seen at `seen`,
/// its play line last written at `saved`.
#[derive(Clone, Debug, PartialEq)]
struct Playing {
    key: String,
    start: u64,
    seen: u64,
    saved: u64,
}

/// A play line is written this often while a session goes on (a crash loses at most this much).
const PLAY_CHECKPOINT_MS: u64 = 5 * 60_000;

/// One look at what's in front (`front`: the counted game's key). Returns the session now and the
/// play lines to write: (game, start, ms so far). Another game in front ends the session at once;
/// nothing in front ends it after `stats::SESSION_GAP_MS` (alt-tabbing doesn't split a session).
fn session_step(p: Option<Playing>, front: Option<&str>, now: u64) -> (Option<Playing>, Vec<(String, u64, u64)>) {
    let mut writes = vec![];
    let mut out = match (p, front) {
        (Some(mut p), Some(k)) if p.key == k => {
            p.seen = now;
            if now.saturating_sub(p.saved) >= PLAY_CHECKPOINT_MS {
                writes.push((p.key.clone(), p.start, now.saturating_sub(p.start)));
                p.saved = now;
            }
            (Some(p), writes)
        }
        (old, Some(k)) => {
            if let Some(o) = old {
                writes.push((o.key, o.start, now.saturating_sub(o.start)));
            }
            (Some(Playing { key: k.to_string(), start: now, seen: now, saved: now }), writes)
        }
        (Some(p), None) if now.saturating_sub(p.seen) >= stats::SESSION_GAP_MS => {
            writes.push((p.key, p.start, p.seen.saturating_sub(p.start)));
            (None, writes)
        }
        (p, None) => (p, writes),
    };
    // a session of no length leaves no line
    out.1.retain(|w| w.2 > 0);
    out
}

/// Writes a play line into a game's history: the line with the same start is updated (a checkpoint),
/// else a new one is added.
fn put_play(log: &mut Log, start: u64, ms: u64) {
    let e = Event { ms, ..Event::new(start, Ev::Play) };
    match log.events.iter_mut().find(|x| x.ev == Ev::Play && x.t == start) {
        Some(x) => x.ms = ms,
        None => log.events.push(e.clone()),
    }
    log.book.apply(&e);
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

    /// What the counter shows for a game: the open fight's boss and all its deaths, the game's total.
    fn counter_of(&mut self, key: &str) -> look::Counter {
        let name = self.name_of(key);
        let b = &self.log(key).book;
        let boss = b.open().map(|f| {
            let k = boss_key(&f.boss);
            let d = b.bosses(0).into_iter().find(|s| boss_key(&s.boss) == k).map_or(f.deaths, |s| s.deaths);
            (f.boss.clone(), d)
        });
        look::Counter { boss, game: (name, b.total_deaths()), ring_mark: ring_mark(b) }
    }

    /// The card after a death in `key`: the boss (or the game), its deaths, the try just lost,
    /// time on the boss, deaths this session, the last tries, and a note.
    fn card_of(&mut self, key: &str) -> look::Card {
        let name = self.name_of(key);
        let since = self.playing.as_ref().filter(|p| p.key == key).map(|p| p.start);
        let log = self.log(key);
        let b = &log.book;
        let tries = stats::tries(b);
        let open = b.fights.iter().rposition(|f| f.ended.is_none());
        let boss = open.map(|i| boss_key(&b.fights[i].boss));
        let mine: Vec<&stats::Try> = match &boss {
            Some(k) => tries.iter().filter(|t| t.fight.is_some_and(|j| boss_key(&b.fights[j].boss) == *k)).collect(),
            None => tries.iter().collect(),
        };
        let bs = boss.as_ref().and_then(|k| b.bosses(now_ms()).into_iter().find(|s| boss_key(&s.boss) == *k));
        let session_start = since.or_else(|| stats::sessions(b, &log.events).last().map(|s| s.start)).unwrap_or(0);
        look::Card {
            title: open.map_or(name, |i| b.fights[i].boss.clone()),
            in_fight: open.is_some(),
            try_next: bs.as_ref().map_or(0, |s| s.attempts + 1),
            deaths: bs.as_ref().map_or(b.total_deaths(), |s| s.deaths),
            try_ms: mine.last().map_or(0, |t| t.ms),
            boss_ms: bs.as_ref().map_or(0, |s| s.time_ms),
            session_deaths: b.deaths.iter().filter(|d| d.0 >= session_start).count() as u32,
            bars: mine[mine.len().saturating_sub(10)..].iter().map(|t| t.ms).collect(),
            note: stats::death_note(b),
            my_note: boss.and_then(|k| store::load_notes(key).get(&k).cloned()),
        }
    }

    /// A death was counted in `key`: the card opens and the red edge glows, as set.
    fn after_death(&mut self, key: &str, s: &cfg::DeathSettings) {
        if s.card {
            let c = self.card_of(key);
            self.card = Some((key.to_string(), Instant::now(), c));
        }
        if s.red_edge {
            self.edge = Some(Instant::now());
        }
    }

    /// A death was taken back: its card closes.
    fn after_undo(&mut self, key: &str) {
        if self.card.as_ref().is_some_and(|c| c.0 == key) {
            self.card = None;
        }
    }

    /// Totals across every game for the Games tab.
    fn overall(&mut self, now: u64) -> Value {
        let stamp = self.games.values().map(|g| g.last).max().unwrap_or(0);
        if let Some((at, s, v)) = &self.overall {
            if *s == stamp && at.elapsed() < Duration::from_secs(10) {
                return v.clone();
            }
        }
        let keys: Vec<(String, String)> = self.games.iter().map(|(k, g)| (k.clone(), g.name.clone())).collect();
        for (k, _) in &keys {
            self.log(k);
        }
        let list: Vec<(String, String, &Book, &[Event])> = keys.iter().filter_map(|(k, n)| self.logs.get(k).map(|l| (k.clone(), n.clone(), &l.book, l.events.as_slice()))).collect();
        let v = view::overall(&list, now);
        self.overall = Some((Instant::now(), stamp, v.clone()));
        v
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
    let ds = app.settings.lock().deaths.clone().clamped();
    let off_games = ds.off_games.clone();
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
    st.games.insert(key.clone(), store::GameInfo { name, last: t });
    match what {
        "death" => st.after_death(&key, &ds),
        "undo" => st.after_undo(&key),
        _ => {}
    }
    store::save_games(&st.games);
    if what == "death" && src != "screen" && src != "save" {
        // a death Deck didn't see: keep the frames before it, so it can be taught
        let shots = screen::recent(&key);
        if shots.last().is_some_and(|s| t.saturating_sub(s.t) < STRIP_FRESH_MS) {
            st.strip = Some((key.clone(), t, shots));
        }
    }
    // a death shows as the card (or the counter's new number); the rest as a short label
    if !flash.is_empty() && flash != "+1" {
        let who = if boss.is_empty() { open.map(|o| o.1).unwrap_or_default() } else { boss.clone() };
        let shown = if flash == "Undone" || who.is_empty() { flash.to_string() } else { format!("{flash}  ·  {who}") };
        st.flash = Some((key.clone(), shown, Instant::now()));
    }
    drop(st);
    app.changed("deaths");
    wake();
    Ok(())
}

fn png_url(img: &crate::pad::raster::Img) -> Option<String> {
    let png = crate::apps::encode_png(&img.rgba_straight(), img.w() as u32, img.h() as u32)?;
    Some(format!("data:image/png;base64,{}", crate::util::base64(&png)))
}

/// The counter and the card with the saved settings and made-up numbers, for the settings'
/// preview: drawn by the same code as in game.
pub fn preview(app: &App) -> Value {
    let (s, theme) = {
        let st = app.settings.lock();
        (st.deaths.clone().clamped(), st.pad.theme.clone())
    };
    let t = art::theme(&theme);
    let c = look::Counter { boss: Some(("Margit, the Fell Omen".into(), 14)), game: ("ELDEN RING".into(), 212), ring_mark: Some(22) };
    let card = look::Card {
        title: "Margit, the Fell Omen".into(),
        in_fight: true,
        try_next: 16,
        deaths: 15,
        try_ms: 161_000,
        boss_ms: 41 * 60_000,
        session_deaths: 23,
        bars: vec![30_000, 45_000, 25_000, 70_000, 55_000, 80_000, 40_000, 90_000, 65_000, 100_000],
        note: Some("Your longest try yet".into()),
        my_note: Some("Dodge left on the third swing".into()),
    };
    json!({
        "counter": look::counter(&c, &s, t, 1.0).as_ref().and_then(png_url),
        "card": png_url(&look::card(&card, &s, t, 1.0)),
    })
}

/// A picture of `game`'s numbers, or one boss's, to share.
pub fn share_picture(app: &App, game: &str, boss: Option<&str>) -> Result<crate::pad::raster::Img, String> {
    let theme = app.settings.lock().pad.theme.clone();
    let sh = shared().ok_or("the death counter isn't running")?;
    let mut st = sh.st.lock();
    let name = st.name_of(game);
    let log = st.log(game);
    let b = &log.book;
    let tries = stats::tries(b);
    let t = art::theme(&theme);
    match boss {
        Some(boss) => {
            let k = boss_key(boss);
            let s = b.bosses(now_ms()).into_iter().find(|s| boss_key(&s.boss) == k).ok_or("That boss isn't in this game's history.")?;
            let mine: Vec<u64> = tries.iter().filter(|t| t.fight.is_some_and(|i| boss_key(&b.fights[i].boss) == k)).map(|t| t.ms).collect();
            let best = mine.iter().copied().max().unwrap_or(0);
            let status = match s.deaths_to_first_win {
                Some(d) => format!("{name} · beaten after {d} deaths"),
                None => name.clone(),
            };
            let rows = [("deaths", s.deaths.to_string()), ("tries", s.attempts.to_string()), ("on this boss", look::span(s.time_ms)), ("best try", look::dur(best))];
            Ok(look::share(&s.boss, &status, &rows, &mine, t))
        }
        None => {
            let played: u64 = stats::sessions(b, &log.events).iter().map(|s| s.end.saturating_sub(s.start)).sum();
            let r = stats::records(b, &[]);
            let rows = [("deaths", b.total_deaths().to_string()), ("bosses beaten", b.wins.len().to_string()), ("played", look::span(played)), ("longest try", look::dur(r.longest_try_ms))];
            let sub = r.deadliest.map(|(boss, d)| format!("deadliest: {boss} ({d})")).unwrap_or_default();
            Ok(look::share(&name, &sub, &rows, &tries.iter().map(|t| t.ms).collect::<Vec<_>>(), t))
        }
    }
}

/// Minutes east of UTC now (Windows' time zone, with daylight saving).
fn utc_offset_min() -> i64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    let now = (now_ms() / 1000 + 11_644_473_600) * 10_000_000;
    let utc = FILETIME { dwLowDateTime: now as u32, dwHighDateTime: (now >> 32) as u32 };
    let mut local = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } == 0 {
        return 0;
    }
    let l = ((local.dwHighDateTime as u64) << 32) | local.dwLowDateTime as u64;
    (l as i64 - now as i64) / 600_000_000
}

/// `game`'s history (or every game's) as a spreadsheet.
pub fn export_csv(game: Option<&str>) -> Result<String, String> {
    let sh = shared().ok_or("the death counter isn't running")?;
    let mut st = sh.st.lock();
    let keys: Vec<String> = match game {
        Some(g) => vec![g.to_string()],
        None => st.games.keys().cloned().collect(),
    };
    let mut named = vec![];
    for k in &keys {
        let name = st.name_of(k);
        st.log(k);
        named.push((k.clone(), name));
    }
    let list: Vec<(String, &[Event])> = named.iter().filter_map(|(k, n)| st.logs.get(k).map(|l| (n.clone(), l.events.as_slice()))).collect();
    Ok(view::csv(&list, utc_offset_min()))
}

/// Saves (or, empty, removes) the note for `boss` in `game`.
pub fn set_note(app: &App, game: &str, boss: &str, text: &str) -> Result<(), String> {
    let mut notes = store::load_notes(game);
    view::set_note(&mut notes, boss, text);
    store::save_notes(game, &notes)?;
    app.changed("deaths");
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
        let live = st.playing.as_ref().filter(|p| p.key == *k).map(|p| (p.start, p.seen));
        let log = st.log(k);
        let b = &log.book;
        let more = view::game_stats(&log.events, b, &store::load_notes(k), live, now);
        let mut g = json!({
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
            "off": settings.off_games.contains(k),
            "save": entry.map(|e| e.save.as_str()).filter(|s| save::known(s)),
            "auto": st.auto.as_ref().filter(|(g, _)| g == k).map(|(_, a)| *a),
            "taught": store::load_taught(k).iter().map(|p| p.made).collect::<Vec<_>>(),
            "library": library::count(k),
            "can_submit": library::good_key(k),
            "strip": st.strip.as_ref().filter(|(g, _, _)| g == k).map(|(_, at, _)| *at),
        });
        if let Some(o) = g.as_object_mut() {
            o.extend(more);
        }
        g
    });
    let overall = st.overall(now);
    json!({
        "running": true,
        "settings": settings,
        "front": front.map(|g| json!({ "key": g.key, "name": g.name, "known": g.entry.is_some() })),
        "game": current,
        "games": games,
        "overall": overall,
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
    let mut edge = edge::Edge::new();
    // the counter pops when its number changes: (game, deaths) last shown, and until when it pops
    let mut last_count: Option<(String, u32)> = None;
    let mut pop_until = Instant::now();
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
            // play sessions: the counted game in front
            let playing_key = front.as_ref().filter(|_| counted).map(|f| f.game.key.clone());
            let (p, writes) = session_step(st.playing.take(), playing_key.as_deref(), now_ms());
            st.playing = p;
            for (k, start, ms) in writes {
                let log = st.log(&k);
                put_play(log, start, ms);
                store::save(&k, &log.events);
            }
            let flash = st.flash.clone().filter(|(_, _, at)| at.elapsed() < FLASH);
            let mut animating = edge.tick();
            let mut showing = false;
            if let Some(at) = st.edge.take() {
                if let Some(f) = front.as_ref().filter(|_| s.red_edge && counted && at.elapsed() < Duration::from_secs(2)) {
                    edge.flash(f.monitor, s.pill_in_recordings);
                    animating = true;
                }
            }
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
                    let key = f.game.key.clone();
                    let k = s.pill_scale * pill::dpi(f.monitor);
                    let label = flash.as_ref().filter(|(g, _, _)| *g == key).map(|x| x.1.clone());
                    let card = st.card.as_ref().filter(|(g, at, _)| *g == key && at.elapsed() < Duration::from_secs(s.card_secs as u64)).map(|c| c.2.clone());
                    let counter = (label.is_none() && card.is_none()).then(|| st.counter_of(&key)).filter(|c| c.boss.is_some() || c.game.1 > 0 || s.pill_always).filter(|c| look::shows(c, &s));
                    drop(st);
                    let t = art::theme(&theme);
                    match (label, card, counter) {
                        (Some(text), _, _) => {
                            showing = true;
                            animating |= pill.show("label", &format!("l|{text}|{k}|{theme}"), || look::label(&text, t, k), f.monitor, &s);
                        }
                        (None, Some(c), _) => {
                            showing = true;
                            animating |= pill.show("card", &format!("c|{c:?}|{k}|{theme}|{:?}", s.card_lines), || look::card(&c, &s, t, k), f.monitor, &s);
                        }
                        (None, None, Some(c)) => {
                            let now = Instant::now();
                            let count = (key.clone(), c.game.1);
                            if last_count.as_ref().is_some_and(|l| l.0 == count.0 && l.1 != count.1) {
                                pop_until = now + Duration::from_millis(120);
                            }
                            last_count = Some(count);
                            let kk = if now < pop_until { k * 1.15 } else { k };
                            animating |= now < pop_until;
                            let id = format!("n|{c:?}|{}|{}|{}|{kk}|{theme}", s.pill_style, s.pill_counts, s.pill_outside);
                            animating |= pill.show("counter", &id, || look::counter(&c, &s, t, kk).unwrap_or_else(|| crate::pad::raster::Img::new(1, 1)), f.monitor, &s);
                        }
                        _ => pill.hide(),
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
            next_look = Instant::now()
                + if animating {
                    Duration::from_millis(33)
                } else if showing {
                    Duration::from_millis(200)
                } else {
                    Duration::from_secs(1)
                };
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

    fn st_with(key: &str, ev: Vec<Event>) -> St {
        let mut st = St::default();
        let book = Book::replay(&ev);
        st.logs.insert(key.into(), Log { events: ev, book });
        st.games.insert(key.into(), store::GameInfo { name: "Game".into(), last: 0 });
        st
    }

    fn fight(t: u64, boss: &str) -> Event {
        Event { fight: format!("f{t}"), boss: boss.into(), ..Event::new(t, Ev::FightStart) }
    }

    #[test]
    fn a_death_opens_the_card_and_undo_closes_it() {
        let mut st = st_with("exe:test-card", vec![fight(0, "Margit"), Event::new(10_000, Ev::Death), Event::new(30_000, Ev::Death)]);
        st.after_death("exe:test-card", &cfg::DeathSettings::default());
        let c = &st.card.as_ref().unwrap().2;
        assert_eq!((c.title.as_str(), c.deaths, c.try_next, c.try_ms, c.in_fight), ("Margit", 2, 3, 20_000, true));
        assert_eq!(c.bars, vec![10_000, 20_000]);
        st.after_undo("exe:test-card");
        assert!(st.card.is_none());
    }

    #[test]
    fn undo_closes_the_card() {
        let mut st = st_with("exe:test-a", vec![Event::new(5, Ev::Death)]);
        st.after_death("exe:test-a", &cfg::DeathSettings::default());
        st.after_undo("exe:test-b");
        assert!(st.card.is_some(), "another game's undo leaves it");
        st.after_undo("exe:test-a");
        assert!(st.card.is_none());
    }

    #[test]
    fn the_card_is_not_opened_when_turned_off_and_the_edge_only_when_on() {
        let mut st = st_with("exe:test-off", vec![Event::new(5, Ev::Death)]);
        let s = cfg::DeathSettings { card: false, ..Default::default() };
        st.after_death("exe:test-off", &s);
        assert!(st.card.is_none() && st.edge.is_none());
        let s = cfg::DeathSettings { red_edge: true, ..Default::default() };
        st.after_death("exe:test-off", &s);
        assert!(st.card.is_some() && st.edge.is_some());
    }

    #[test]
    fn the_ring_mark_is_the_last_beaten_boss_deaths() {
        let d = |t| Event::new(t, Ev::Death);
        let b = Book::replay(&[fight(0, "A"), d(1), d(2), d(3), Event::new(4, Ev::Win), fight(5, "B"), d(6)]);
        assert_eq!(ring_mark(&b), Some(3));
        assert_eq!(ring_mark(&Book::replay(&[fight(0, "A"), d(1)])), None, "nothing beaten yet");
    }

    #[test]
    fn a_session_starts_continues_and_checkpoints() {
        let (p, w) = session_step(None, Some("exe:a"), 0);
        assert!(w.is_empty());
        let p = p.unwrap();
        assert_eq!(p.start, 0);
        let (p, w) = session_step(Some(p), Some("exe:a"), 300_000);
        assert_eq!(w, vec![("exe:a".to_string(), 0, 300_000)]);
        let (p, w) = session_step(p, None, 600_000); // alt-tabbed
        assert!(p.is_some() && w.is_empty());
        let (p, w) = session_step(p, None, 300_000 + 15 * 60_000);
        assert!(p.is_none());
        assert_eq!(w, vec![("exe:a".to_string(), 0, 300_000)], "it ends when the game was last seen");
    }

    #[test]
    fn another_game_ends_the_session() {
        let (p, _) = session_step(None, Some("exe:a"), 0);
        let (p, w) = session_step(p, Some("exe:b"), 60_000);
        assert_eq!(w, vec![("exe:a".to_string(), 0, 60_000)]);
        assert_eq!(p.unwrap().key, "exe:b");
        let (_, w) = session_step(None, Some("exe:a"), 5);
        let (_, w2) = session_step(Some(Playing { key: "exe:a".into(), start: 5, seen: 5, saved: 5 }), Some("exe:b"), 5);
        assert!(w.is_empty() && w2.is_empty(), "a session of no length leaves no line");
    }

    #[test]
    fn a_checkpoint_replaces_the_play_line_instead_of_adding_one() {
        let mut log = Log { events: vec![Event::new(1, Ev::Death)], book: Book::default() };
        log.book = Book::replay(&log.events);
        put_play(&mut log, 100, 300_000);
        put_play(&mut log, 100, 600_000);
        assert_eq!(log.events.iter().filter(|e| e.ev == Ev::Play).count(), 1);
        assert_eq!(log.events.iter().find(|e| e.ev == Ev::Play).unwrap().ms, 600_000);
        assert_eq!(log.book.plays, vec![(100, 600_000)]);
        assert_eq!(log.book.total_deaths(), 1);
    }

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
