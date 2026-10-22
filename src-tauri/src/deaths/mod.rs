//! The death counter: deaths per game and per boss, in any game. See
//! `docs/design/specs/2026-10-19-death-counter-design.md`.
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
use book::{boss_key, same_boss};
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

/// A death (or victory) screen Deck found by itself, to ask about.
struct Guess {
    game: String,
    win: bool,
    print: screen::teach::Print,
    /// the picture it was found on (PNG)
    png: Option<Vec<u8>>,
    /// how many counted by hand it showed up before
    seen: usize,
}

/// Strips of deaths counted by hand kept to look for the screen they share.
const HAND_STRIPS: usize = 3;

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
    /// the frames before the last death (or win) you counted yourself: (game, when, frames), to teach from
    strip: Option<(String, u64, Vec<screen::Shot>)>,
    /// the strip is from a win: what's taught from it is a victory screen
    strip_win: bool,
    /// the thumbnails before the last few deaths (or wins) you counted yourself in one game, to
    /// find the screen they share: (game, wins, strips)
    hand: Option<(String, bool, Vec<Vec<Vec<u8>>>)>,
    /// a screen found that way, waiting for a yes
    guess: Option<Guess>,
    /// guesses you said no to (their masks): not offered again
    refused: Vec<String>,
    /// the bar-like strips of the picture you pick a game's boss bar from: (game, strips)
    bar_pick: Option<(String, Vec<screen::bar::Spot>)>,
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
            // the death just counted, if it was in this fight with the bar seen
            boss_hp: b.deaths.last().and_then(|d| d.1).filter(|i| Some(*i) == open).and_then(|i| b.fights[i].hp.last().copied()).filter(|h| h.1 != book::NO_HP).map(|(phase, hp)| (hp, phase)),
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
    act_seen(app, what, boss, game, src, "")
}

/// `act`, for a death the screen watcher saw: `by` is the death screen that saw it (`Event.by`).
fn act_seen(app: &App, what: &str, boss: Option<&str>, game: Option<&str>, src: &str, by: &str) -> Result<(), String> {
    let Some(sh) = shared() else { return Err("the death counter isn't running".into()) };
    let ds = app.settings.lock().deaths.clone().clamped();
    let off_games = ds.off_games.clone();
    let mut st = sh.st.lock();
    let key = st.target(game).ok_or(NO_GAME)?;
    // what was counted and how it was seen (a hotkey, the screen, the controller combo): enough to
    // tell a misdetection from a press, without what was on screen
    let line = format!("death counter: {what} in {key} (from {src}{}{})", if by.is_empty() { "" } else { ", seen by " }, by);
    if matches!(what, "death" | "undo" | "win" | "start" | "end") {
        crate::log::info(line);
    } else {
        crate::log::debug(line);
    }
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
            e.by = by.into();
            if let Some((hp, phase)) = screen::bar_now(&key, t) {
                e.hp = Some(hp);
                e.phase = (phase > 1).then_some(phase);
            }
            (Some(e), "+1")
        }
        "not_boss" => {
            // the screen took something for a boss: forget the fight (its deaths are the game's)
            // and never open one for that name again
            let Some(f) = st.log(&key).book.open().cloned() else { return Err("No fight is open.".into()) };
            drop(st); // the settings' lock is never taken inside this one
            {
                let mut s = app.settings.lock();
                for n in std::iter::once(&f.boss).chain(&f.seen) {
                    let entry = format!("{key}\t{n}");
                    if !s.deaths.not_bosses.contains(&entry) {
                        s.deaths.not_bosses.push(entry);
                    }
                }
            }
            app.save_settings();
            st = sh.st.lock();
            let log = st.log(&key);
            log.events.retain(|e| e.fight != f.id);
            log.book = Book::replay(&log.events);
            st.flash = Some((key.clone(), format!("Not a boss  ·  {}", f.boss), Instant::now()));
            (None, "")
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
        w if w.starts_with("merge:") => {
            let from = w[6..].trim();
            if boss.is_empty() || from.is_empty() || book::boss_key(from) == book::boss_key(&boss) {
                return Err("Pick another boss to merge it into.".into());
            }
            let log = st.log(&key);
            if book::merge_boss(&mut log.events, from, &boss) == 0 {
                return Err("That boss is gone already.".into());
            }
            log.book = Book::replay(&log.events);
            // its note goes with it, unless the other boss has one
            let mut notes = store::load_notes(&key);
            if let Some(n) = notes.remove(&book::boss_key(from)) {
                notes.entry(book::boss_key(&boss)).or_insert(n);
                store::save_notes(&key, &notes)?;
            }
            (None, "")
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
    let mut guess_from = None;
    if (what == "death" || what == "win") && src != "screen" && src != "save" {
        // a death or a win Deck didn't see: keep the frames before it, so its screen can be taught
        let shots = screen::recent(&key);
        if shots.last().is_some_and(|s| t.saturating_sub(s.t) < STRIP_FRESH_MS) {
            let win = what == "win";
            // and look for the screen the last few counted by hand share
            if st.hand.as_ref().is_none_or(|h| h.0 != key || h.1 != win) {
                st.hand = Some((key.clone(), win, Vec::new()));
            }
            let asking = st.guess.as_ref().is_some_and(|g| g.game == key);
            if let Some(h) = st.hand.as_mut() {
                h.2.push(shots.iter().map(|s| s.thumb.clone()).collect());
                if h.2.len() > HAND_STRIPS {
                    h.2.remove(0);
                }
                if h.2.len() >= 2 && !asking {
                    guess_from = Some((key.clone(), win, h.2.clone(), shots.clone()));
                }
            }
            st.strip = Some((key.clone(), t, shots));
            st.strip_win = win;
        }
    }
    if what == "undo" {
        // a death taken back wasn't one: its frames say nothing
        if let Some(h) = st.hand.as_mut().filter(|h| h.0 == key && !h.1) {
            h.2.pop();
        }
    }
    // a death shows as the card (or the counter's new number); the rest as a short label
    if !flash.is_empty() && flash != "+1" {
        let who = if boss.is_empty() { open.map(|o| o.1).unwrap_or_default() } else { boss.clone() };
        let shown = if flash == "Undone" || who.is_empty() { flash.to_string() } else { format!("{flash}  ·  {who}") };
        st.flash = Some((key.clone(), shown, Instant::now()));
    }
    drop(st);
    if let Some((game, win, strips, newest)) = guess_from {
        let _ = std::thread::Builder::new().name("deaths-guess".into()).spawn(move || find_guess(game, win, strips, newest));
    }
    app.changed("deaths");
    wake();
    Ok(())
}

/// Looks for the screen the last deaths (or wins) counted by hand in `game` share, and offers it
/// ("Is this the death screen?"). Off the page's thread: it tries every frame.
fn find_guess(game: String, win: bool, strips: Vec<Vec<Vec<u8>>>, newest: Vec<screen::Shot>) {
    let Some((i, mut print)) = screen::teach::shared_screen(&strips, now_ms()) else { return };
    let Some(sh) = shared() else { return };
    let mut st = sh.st.lock();
    let have = if win { store::load_won(&game) } else { store::load_taught(&game) };
    if st.refused.contains(&print.mask) || have.len() >= screen::teach::MAX_PER_GAME || have.iter().any(|p| p.mask == print.mask) {
        return;
    }
    let shot = newest.get(i);
    print.aspect = shot.map_or(0, |s| s.aspect);
    let png = shot.and_then(|s| png_bytes(&s.preview, screen::PW, screen::PH));
    crate::log::info(format!("death counter: found the {} screen shared by {} counted by hand in {game}", if win { "victory" } else { "death" }, strips.len()));
    st.guess = Some(Guess { game, win, print, png, seen: strips.len() });
    drop(st);
    if let Some(app) = sh.app.upgrade() {
        app.changed("deaths");
    }
}

/// "Yes, that's it": the screen Deck found is learned like one you showed it.
pub fn accept_guess(app: &App, game: &str) -> Result<(), String> {
    let sh = shared().ok_or("the death counter isn't running")?;
    let mut st = sh.st.lock();
    let Some(g) = st.guess.take().filter(|g| g.game == game) else { return Err("That picture is gone.".into()) };
    let mut prints = if g.win { store::load_won(game) } else { store::load_taught(game) };
    if prints.len() >= screen::teach::MAX_PER_GAME {
        return Err(format!("This game already has {} learned screens of that kind. Forget one first.", screen::teach::MAX_PER_GAME));
    }
    let kind = if g.win { "won" } else { "taught" };
    if let Some(b) = &g.png {
        store::save_shot(game, kind, g.print.made, b);
    }
    prints.push(g.print);
    if g.win { store::save_won(game, &prints) } else { store::save_taught(game, &prints) }?;
    st.hand = None;
    st.strip = None;
    drop(st);
    screen::taught_changed();
    app.changed("deaths");
    Ok(())
}

/// "No": not that screen, and it isn't offered again.
pub fn refuse_guess(app: &App, game: &str) -> Result<(), String> {
    let sh = shared().ok_or("the death counter isn't running")?;
    let mut st = sh.st.lock();
    if let Some(g) = st.guess.take().filter(|g| g.game == game) {
        st.refused.push(g.print.mask);
        st.hand = None;
    }
    drop(st);
    app.changed("deaths");
    Ok(())
}

/// `name` was marked "Not a boss" in `game`.
fn not_a_boss(s: &cfg::DeathSettings, game: &str, name: &str) -> bool {
    s.not_bosses.iter().any(|e| e.split_once('\t').is_some_and(|(k, n)| k == game && same_boss(n, name)))
}

/// What the screen saw of a boss fight, as history lines for a game's book at `t`, and the label
/// to show: its bar came up (a try starts; a new boss opens its fight), went away, or the boss was
/// beaten. `not_a_boss`: names marked "Not a boss".
fn boss_events(b: &Book, out: screen::boss::Out, t: u64, not_a_boss: impl Fn(&str) -> bool) -> (Vec<Event>, Option<String>) {
    use screen::boss::Out;
    let open = b.open();
    match out {
        Out::Engage { since, name, phase } => {
            if !name.is_empty() && not_a_boss(&name) {
                return (vec![], None);
            }
            match open {
                // the same boss, or the same try's next phase (another name, another bar)
                Some(f) if phase > 1 || name.is_empty() || f.fits(&name) => (vec![Event { fight: f.id.clone(), boss: name, ..Event::new(since, Ev::Engage) }], None),
                // its bar back after a death, the name read badly ("aldii" for "Xaldin"): the same
                // fight, and the misreading isn't kept as one of its names
                Some(f) if f.auto && f.misread(&name) => (vec![Event { fight: f.id.clone(), ..Event::new(since, Ev::Engage) }], None),
                // a bar after a win the screen called: the win was a phase; the fight goes on
                None if phase > 1 => match b.fights.last().filter(|f| f.won && f.auto && f.ended.is_some_and(|e| since.saturating_sub(e) <= screen::boss::PHASE_GAP_MS)) {
                    Some(f) => {
                        let reopen = Event { fight: f.id.clone(), ms: f.ended.unwrap_or(0), src: "reopen".into(), ..Event::new(since, Ev::Engage) };
                        (vec![reopen], Some(format!("Not beaten yet  ·  {}", f.boss)))
                    }
                    None => (vec![], None),
                },
                _ if name.is_empty() => (vec![], None),
                _ => {
                    let id = format!("f{since}");
                    let start = Event { fight: id.clone(), boss: name.clone(), src: "screen".into(), ..Event::new(since, Ev::FightStart) };
                    let flash = format!("Fight  ·  {name}");
                    (vec![start, Event { fight: id, boss: name, ..Event::new(since, Ev::Engage) }], Some(flash))
                }
            }
        }
        // more reads agree on another spelling of a fight the screen just opened
        Out::Rename { since, name } => match open {
            Some(f) if f.auto && f.started == since && !same_boss(&f.boss, &name) && !not_a_boss(&name) => {
                (vec![Event { fight: f.id.clone(), boss: name, ..Event::new(t, Ev::Rename) }], None)
            }
            _ => (vec![], None),
        },
        Out::Leave => match open {
            Some(f) if f.engaged.is_some() => (vec![Event { fight: f.id.clone(), ..Event::new(t, Ev::Leave) }], None),
            _ => (vec![], None),
        },
        Out::Won { last_seen } => {
            // only a fight the screen saw; and a death counted since its bar was last seen (by
            // hand, the screen missed it) means it wasn't beaten
            match open.filter(|f| !f.engages.is_empty()) {
                Some(f) if b.deaths.last().is_none_or(|d| d.0 + 2_000 < last_seen) => {
                    (vec![Event { src: "screen".into(), ..Event::new(t, Ev::Win) }], Some(format!("Defeated  ·  {}", f.boss)))
                }
                _ => (vec![], None),
            }
        }
    }
}

/// Removes the win the screen called at `at` (the last one, if it's still the latest win).
fn take_back_win(events: &mut Vec<Event>, at: u64) -> bool {
    match events.iter().rposition(|e| e.ev == Ev::Win) {
        Some(i) if events[i].t == at && events[i].src == "screen" => {
            events.remove(i);
            true
        }
        _ => false,
    }
}

/// `boss_events`, recorded for `game`.
fn boss_seen(app: &App, game: &str, out: screen::boss::Out) {
    let Some(sh) = shared() else { return };
    let ds = app.settings.lock().deaths.clone();
    if !ds.on || !ds.auto_boss || ds.off_games.iter().any(|k| k == game) {
        return;
    }
    crate::log::info(format!("death counter: boss bar in {game}: {out:?}"));
    let mut st = sh.st.lock();
    let t = now_ms();
    let (mut evs, flash) = boss_events(&st.log(game).book, out, t, |n| not_a_boss(&ds, game, n));
    if evs.is_empty() {
        return;
    }
    let name = st.name_of(game);
    let log = st.log(game);
    if evs[0].src == "reopen" {
        // take back the win the screen called (`ms`: when), then go on with the fight
        let won_at = evs[0].ms;
        if !take_back_win(&mut log.events, won_at) {
            return;
        }
        log.book = Book::replay(&log.events);
        evs[0].src.clear();
        evs[0].ms = 0;
    }
    for e in evs {
        log.book.apply(&e);
        log.events.push(e);
    }
    store::save(game, &log.events);
    st.games.insert(game.to_string(), store::GameInfo { name, last: t });
    store::save_games(&st.games);
    if let Some(f) = flash {
        st.flash = Some((game.to_string(), f, Instant::now()));
    }
    drop(st);
    app.changed("deaths");
    wake();
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
            boss_hp: Some((23, 1)),
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

fn png_bytes(rgb: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    let img = image::RgbImage::from_raw(w as u32, h as u32, rgb.to_vec())?;
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", crate::util::base64(png))
}

fn png(rgb: &[u8], w: usize, h: usize) -> String {
    png_bytes(rgb, w, h).map(|b| data_url(&b)).unwrap_or_default()
}

/// A learned screen for the page: the picture it was taught on (none for older ones and shared
/// ones), where on it Deck looks, and the cells it looks for.
fn print_json(game: &str, kind: &str, p: &screen::teach::Print) -> Value {
    json!({
        "made": p.made,
        "shot": store::load_shot(game, kind, p.made).map(|b| data_url(&b)),
        "area": screen::teach::area(p),
        "img": screen::teach::picture(p).map(|px| png(&px, screen::teach::W, screen::teach::H)),
    })
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
    let win = st.strip_win;
    let mut prints = if win { store::load_won(g) } else { store::load_taught(g) };
    if prints.len() >= screen::teach::MAX_PER_GAME {
        let what = if win { "victory" } else { "death" };
        return Err(format!("This game already has {} taught {what} screens. Forget one first.", screen::teach::MAX_PER_GAME));
    }
    let thumbs: Vec<Vec<u8>> = shots.iter().map(|s| s.thumb.clone()).collect();
    let mut print = screen::teach::learn(&thumbs, pick, now_ms())?;
    // a fingerprint only fits pictures of the same shape (16:9, 21:9, ...)
    print.aspect = shots.get(pick).map_or(0, |s| s.aspect);
    let kind = if win { "won" } else { "taught" };
    if let Some(b) = shots.get(pick).and_then(|s| png_bytes(&s.preview, screen::PW, screen::PH)) {
        store::save_shot(game, kind, print.made, &b);
    }
    prints.push(print);
    if win { store::save_won(game, &prints) } else { store::save_taught(game, &prints) }?;
    st.strip = None;
    st.hand = None;
    st.guess = None;
    drop(st);
    screen::taught_changed();
    app.changed("deaths");
    Ok(())
}

/// Uses the shared library's death screens for `game` or not (one of them counts wrong deaths).
pub fn library_for(app: &App, game: &str, on: bool) -> Result<(), String> {
    if game.is_empty() {
        return Err("Which game?".into());
    }
    {
        let mut s = app.settings.lock();
        s.deaths.library_off.retain(|k| k != game);
        if !on {
            s.deaths.library_off.push(game.to_string());
        }
    }
    app.save_settings();
    app.changed("deaths");
    wake();
    Ok(())
}

/// Starts over: every game's counts, fights, sessions and boss notes are cleared (moved aside to
/// `deaths\reset-<time>`, a way back by hand). Settings and learned death screens stay.
pub fn reset(app: &App) -> Result<(), String> {
    let sh = shared().ok_or("the death counter isn't running")?;
    let mut st = sh.st.lock();
    let to = store::reset_in(&store::dir(), now_ms()).map_err(|e| format!("Couldn't reset: {e}"))?;
    crate::log::info(format!("deaths: reset; the old history is in {}", to.display()));
    st.games.clear();
    st.logs.clear();
    st.playing = None;
    st.overall = None;
    st.card = None;
    st.edge = None;
    st.flash = None;
    st.strip = None;
    st.last_death = None;
    drop(st);
    app.changed("deaths");
    wake();
    Ok(())
}

/// Forgets taught death screen `i` of `game`.
pub fn forget(app: &App, game: &str, i: usize) -> Result<(), String> {
    let mut prints = store::load_taught(game);
    if i >= prints.len() {
        return Err("That one is gone already.".into());
    }
    let gone = prints.remove(i);
    store::save_taught(game, &prints)?;
    store::remove_shot(game, "taught", gone.made);
    screen::taught_changed();
    app.changed("deaths");
    Ok(())
}

/// Forgets the victory screen `i` taught for `game`.
pub fn forget_won(app: &App, game: &str, i: usize) -> Result<(), String> {
    let mut prints = store::load_won(game);
    if i >= prints.len() {
        return Err("That one is gone already.".into());
    }
    let gone = prints.remove(i);
    store::save_won(game, &prints)?;
    store::remove_shot(game, "won", gone.made);
    screen::taught_changed();
    app.changed("deaths");
    Ok(())
}

/// The last picture of `game` with a bar-like strip on it (big, colour), with its strips
/// (thousandths of the picture), to show where the boss's bar and name are. Its strips are kept
/// until one is picked.
pub fn bar_picture(game: &str) -> Value {
    let Some(sh) = shared() else { return json!(null) };
    let Some(b) = screen::big(game) else { return json!(null) };
    sh.st.lock().bar_pick = Some((game.to_string(), b.bars.clone()));
    json!({ "img": png(&b.rgb, b.w, b.h), "w": b.w, "h": b.h, "bars": b.bars, "t": b.t, "now": now_ms() })
}

/// `game`'s boss bar is strip `bar` of the last `bar_picture`; None forgets it (the catalogue's,
/// or the middle, again).
pub fn teach_bar(app: &App, game: &str, bar: Option<usize>) -> Result<(), String> {
    let spot = match bar {
        Some(i) => {
            let sh = shared().ok_or("the death counter isn't running")?;
            let st = sh.st.lock();
            let bars = st.bar_pick.as_ref().filter(|(g, _)| g == game).map(|(_, b)| b).ok_or("That picture is gone; show it again.")?;
            Some(*bars.get(i).ok_or("That bar isn't on this picture.")?)
        }
        None => None,
    };
    {
        let mut s = app.settings.lock();
        match spot {
            Some(p) => s.deaths.bar_spots.insert(game.to_string(), p),
            None => s.deaths.bar_spots.remove(game),
        };
    }
    app.save_settings();
    wake();
    app.changed("deaths");
    Ok(())
}

/// `game` shows the boss's name in `area` (thousandths of the picture); None forgets it (read
/// next to the bar again).
pub fn teach_name(app: &App, game: &str, area: Option<screen::bar::Spot>) -> Result<(), String> {
    if let Some(a) = area {
        if !(a.x0 < a.x1 && a.y0 < a.y1 && a.x1 <= 1000 && a.y1 <= 1000 && a.x1 - a.x0 >= 20 && a.y1 - a.y0 >= 8) {
            return Err("Draw the box around the whole name.".into());
        }
    }
    {
        let mut s = app.settings.lock();
        match area {
            Some(a) => s.deaths.name_spots.insert(game.to_string(), a),
            None => s.deaths.name_spots.remove(game),
        };
    }
    app.save_settings();
    wake();
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
        let seen_by = view::by_screen(&log.events, b);
        let mut g = json!({
            "key": k,
            "name": name,
            "kind": entry.map(|e| e.kind.as_str()).unwrap_or("boss"),
            "known": entry.is_some(),
            "total": b.total_deaths(),
            "loose": b.loose,
            "open": b.open().map(|f| json!({ "boss": f.boss, "deaths": f.deaths, "started": f.started, "auto": f.auto, "seen": !f.engages.is_empty(), "in_arena": f.engaged })),
            "not_bosses": settings.not_bosses.iter().filter_map(|e| e.split_once('\t')).filter(|(g, _)| g == k).map(|(_, n)| n).collect::<Vec<_>>(),
            "bosses": b.bosses(now),
            "names": b.boss_names(),
            "can_undo": b.can_undo(),
            "off": settings.off_games.contains(k),
            "save": entry.map(|e| e.save.as_str()).filter(|s| save::known(s)),
            "auto": st.auto.as_ref().filter(|(g, _)| g == k).map(|(_, a)| *a),
            "taught": store::load_taught(k).iter().map(|p| {
                let (deaths, last) = seen_by.get(&format!("taught:{}", p.made)).copied().unwrap_or_default();
                let mut j = print_json(k, "taught", p);
                j["deaths"] = json!(deaths);
                j["last"] = json!(last);
                j
            }).collect::<Vec<_>>(),
            "library": library::count(k),
            "library_deaths": seen_by.get("library").map_or(0, |c| c.0),
            "library_off": settings.library_off.contains(k),
            "can_submit": library::good_key(k),
            "strip": st.strip.as_ref().filter(|(g, _, _)| g == k).map(|(_, at, _)| *at),
            "strip_win": st.strip_win,
            "guess": st.guess.as_ref().filter(|g| g.game == *k).map(|g| json!({
                "win": g.win,
                "seen": g.seen,
                "shot": g.png.as_deref().map(data_url),
                "area": screen::teach::area(&g.print),
                "img": screen::teach::picture(&g.print).map(|px| png(&px, screen::teach::W, screen::teach::H)),
            })),
            "won_taught": store::load_won(k).iter().map(|p| print_json(k, "won", p)).collect::<Vec<_>>(),
            "bar_spot": settings.bar_spots.get(k),
            "name_spot": settings.name_spots.get(k),
            "bar_preset": entry.is_some_and(|e| e.bar.is_some()),
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

fn window_open(hwnd: isize) -> bool {
    unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(windows::Win32::Foundation::HWND(hwnd as *mut _))).as_bool() }
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
            screen::set_bosses(s.auto_boss);
            let shared = s.on && s.community_library;
            if library::set_enabled(shared) | library::set_off(&s.library_off) {
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
            // by default only games Deck can read: listed in the catalogue, with death screens taught
            // here or shared, or that the user counted deaths in (an unknown game isn't looked at)
            let readable = |f: &ident::Front| {
                s.watch_in == "all" || f.game.entry.is_some() || !store::load_taught(&f.game.key).is_empty() || !library::prints(&f.game.key).is_empty() || sh.st.lock().log(&f.game.key).book.total_deaths() > 0
            };
            let looking = front.as_ref().filter(|f| counted && s.auto_screen && !save_ok(&f.game.key) && readable(f));
            match looking {
                Some(f) if watch.as_ref().is_none_or(|w| w.game != f.game.key || w.hwnd != f.hwnd) => {
                    let (weak, key) = (sh.app.clone(), f.game.key.clone());
                    let words = f.game.entry.map(|e| e.words.clone()).unwrap_or_default();
                    let win = f.game.entry.map(|e| e.win.clone()).unwrap_or_default();
                    // games Deck knows are watched 4 times a second, any other 2 (enough for a
                    // banner that stays up, and half the work in games that may never show one)
                    watch = Some(screen::Watch::start(&f.game.key, f.hwnd, &words, &win, move |seen| {
                        if let Some(a) = weak.upgrade() {
                            match seen {
                                screen::Seen::Death(by) => {
                                    let _ = act_seen(&a, "death", None, Some(&key), "screen", &by);
                                }
                                screen::Seen::Boss(out) => boss_seen(&a, &key, out),
                                screen::Seen::Victory => {
                                    let _ = act(&a, "win", None, Some(&key), "screen");
                                }
                            }
                        }
                    }));
                }
                Some(_) => {
                    if let Some(w) = &watch {
                        w.pause(false);
                    }
                }
                None => {
                    // another window in front (Deck, chat, a browser): the game's watcher waits
                    // instead of starting over, while its window is open and it's still watched
                    let keep = watch.as_ref().is_some_and(|w| s.on && s.auto_screen && !s.off_games.contains(&w.game) && !save_ok(&w.game) && window_open(w.hwnd));
                    match &watch {
                        Some(w) if keep => w.pause(true),
                        _ => watch = None,
                    }
                }
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
                screen::set_spot(&f.game.key, s.bar_spots.get(&f.game.key).copied().or(f.game.entry.and_then(|e| e.bar)));
                screen::set_name_spot(&f.game.key, s.name_spots.get(&f.game.key).copied());
                let known = st.log(&f.game.key).book.boss_names();
                screen::set_known(&f.game.key, known);
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

    /// The screen's boss events as history: a new boss opens a fight, its next phase (another name)
    /// stays in it, "Not a boss" names are left alone, and a win needs no death after the bar.
    #[test]
    fn boss_bars_open_fights_and_phases_stay_in_them() {
        use screen::boss::Out;
        let engage = |since: u64, name: &str, phase: u8| Out::Engage { since, name: name.into(), phase };
        let mut b = Book::default();
        let feed = |b: &mut Book, out: Out, t: u64| {
            let (evs, flash) = boss_events(b, out, t, |n| n == "Torrent");
            for e in &evs {
                b.apply(e);
            }
            (evs.len(), flash)
        };
        assert_eq!(feed(&mut b, engage(1_000, "Torrent", 1), 1_000), (0, None), "not a boss");
        assert_eq!(feed(&mut b, engage(2_000, "", 1), 2_000), (0, None), "no name, no fight");
        let (n, flash) = feed(&mut b, engage(3_000, "Radagon of the Golden Order", 1), 3_000);
        assert_eq!((n, flash.as_deref()), (2, Some("Fight  ·  Radagon of the Golden Order")));
        assert_eq!(feed(&mut b, engage(60_000, "Elden Beast", 2), 60_000).0, 1, "phase 2 joins the fight");
        assert_eq!(b.fights.len(), 1);
        b.apply(&Event { hp: Some(30), phase: Some(2), ..Event::new(90_000, Ev::Death) });
        assert_eq!(b.open().unwrap().deaths, 1);
        // the next try: phase 1's name again, and then the second name is already the fight's
        assert_eq!(feed(&mut b, engage(150_000, "Radagon of the Golden Order", 1), 150_000).0, 1);
        assert_eq!(b.fights.len(), 1);
        assert!(b.open().unwrap().fits("Elden Beast"));
        // a death counted by hand after the bar went: not a win
        b.apply(&Event::new(200_000, Ev::Death));
        assert_eq!(feed(&mut b, Out::Won { last_seen: 199_000 }, 320_000).0, 0);
        assert_eq!(feed(&mut b, engage(400_000, "Radagon of the Golden Order", 1), 400_000).0, 1);
        assert_eq!(feed(&mut b, Out::Leave, 420_000).0, 1);
        assert_eq!(feed(&mut b, Out::Leave, 421_000).0, 0, "left already");
        let (n, flash) = feed(&mut b, Out::Won { last_seen: 419_000 }, 540_000);
        assert_eq!((n, flash.as_deref()), (1, Some("Defeated  ·  Radagon of the Golden Order")));
        assert_eq!(b.bosses(0)[0].wins, 1);
        assert_eq!(b.bosses(0)[0].closest, Some((2, 30)));
        // a different boss later opens its own fight
        assert_eq!(feed(&mut b, engage(600_000, "Godrick the Grafted", 1), 600_000).0, 2);
    }

    /// Khazan, 2026-10-21: Xaldin's bar came back after a death read as "aldii" and opened a
    /// second fight. Now it's the same fight, and "aldii" isn't kept as a name.
    #[test]
    fn a_misread_name_after_a_death_stays_in_the_fight() {
        use screen::boss::Out;
        fn feed(b: &mut Book, out: Out, t: u64) -> Vec<Event> {
            let (evs, _) = boss_events(b, out, t, |_| false);
            for e in &evs {
                b.apply(e);
            }
            evs
        }
        let mut b = Book::default();
        feed(&mut b, Out::Engage { since: 0, name: "Xaldin".into(), phase: 1 }, 0);
        b.apply(&Event { hp: Some(64), ..Event::new(119_000, Ev::Death) });
        let evs = feed(&mut b, Out::Engage { since: 161_000, name: "aldii".into(), phase: 1 }, 161_000);
        assert_eq!(evs.len(), 1, "joined, no new fight: {evs:?}");
        assert!(evs[0].boss.is_empty(), "the misreading isn't a name of the fight");
        assert_eq!((b.fights.len(), b.open().unwrap().seen.clone()), (1, vec!["xaldin".to_string()]));
        b.apply(&Event::new(195_000, Ev::Death));
        assert_eq!(b.bosses(0)[0].deaths, 2);
        assert!(!b.open().unwrap().fits("Viper") && !b.open().unwrap().misread("Viper"), "another boss still opens its own fight");
        // a fight opened under a misreading is renamed when the votes say so; an older fight never is
        let mut b = Book::default();
        feed(&mut b, Out::Engage { since: 500, name: "aldii".into(), phase: 1 }, 500);
        assert_eq!(feed(&mut b, Out::Rename { since: 500, name: "Xaldin".into() }, 3_000).len(), 1);
        assert_eq!(b.open().unwrap().boss, "Xaldin");
        assert_eq!(feed(&mut b, Out::Rename { since: 9, name: "Viper".into() }, 4_000).len(), 0, "not this fight's try");
    }

    /// The screen called a win (the bar emptied and went), then the next phase's bar came up: the
    /// win is taken back and the fight goes on.
    #[test]
    fn a_win_the_screen_called_is_taken_back_when_a_phase_follows() {
        use screen::boss::Out;
        let mut events = vec![
            Event { fight: "f0".into(), boss: "Radagon".into(), src: "screen".into(), ..Event::new(0, Ev::FightStart) },
            Event { fight: "f0".into(), boss: "Radagon".into(), ..Event::new(0, Ev::Engage) },
            Event { src: "screen".into(), ..Event::new(100_000, Ev::Win) },
        ];
        let b = Book::replay(&events);
        let (evs, flash) = boss_events(&b, Out::Engage { since: 140_000, name: "Elden Beast".into(), phase: 2 }, 140_000, |_| false);
        assert_eq!((evs.len(), evs[0].src.as_str(), evs[0].ms), (1, "reopen", 100_000));
        assert_eq!(flash.as_deref(), Some("Not beaten yet  ·  Radagon"));
        assert!(take_back_win(&mut events, 100_000));
        let mut b = Book::replay(&events);
        b.apply(&Event { fight: "f0".into(), boss: "Elden Beast".into(), ..Event::new(140_000, Ev::Engage) });
        assert!(b.open().is_some_and(|f| f.fits("Elden Beast")) && b.wins.is_empty());
        assert!(!take_back_win(&mut events, 100_000), "only once");
        // too late: a bar long after a win is a new fight
        let b = Book::replay(&[events[0].clone(), events[1].clone(), Event { src: "screen".into(), ..Event::new(100_000, Ev::Win) }]);
        let (evs, _) = boss_events(&b, Out::Engage { since: 100_000 + screen::boss::PHASE_GAP_MS + 1, name: "Elden Beast".into(), phase: 2 }, 0, |_| false);
        assert!(evs.is_empty(), "{evs:?}");
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
