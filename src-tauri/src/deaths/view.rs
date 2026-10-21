//! What the Deaths page shows, worked out from a game's history: tries, play sessions with their
//! events, bosses with their progress, records, and totals across every game. Pure (no files, no
//! locks): `mod.rs` passes the history in.

use super::book::{boss_key, Book, Ev, Event};
use super::stats::{self, Session, Try};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Sessions and events shown at most (the page stays quick with years of history).
const MAX_SESSIONS: usize = 60;
const MAX_EVENTS: usize = 400;

/// A note per boss: at most this many characters.
pub const NOTE_MAX: usize = 500;

/// Sets (or, when empty, removes) the note for `boss`; kept under its boss key, trimmed and cut.
pub fn set_note(notes: &mut BTreeMap<String, String>, boss: &str, text: &str) {
    let k = boss_key(boss);
    let t = text.trim();
    if t.is_empty() {
        notes.remove(&k);
    } else {
        notes.insert(k, t.chars().take(NOTE_MAX).collect());
    }
}

/// An event as the History tab shows it, with the boss it was about.
struct Line<'a> {
    i: usize,
    e: &'a Event,
    boss: String,
    /// a win or abandon: the fight's deaths and length
    deaths: u32,
    ms: u64,
}

fn clean(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every event but play lines, with the boss of the fight it happened in.
fn lines(events: &[Event]) -> Vec<Line<'_>> {
    // (boss, started, deaths, found on screen, its bar up now)
    let mut open: Option<(String, u64, u32, bool, bool)> = None;
    let mut out = Vec::with_capacity(events.len());
    for (i, e) in events.iter().enumerate() {
        let mut line = Line { i, e, boss: String::new(), deaths: 0, ms: 0 };
        match e.ev {
            Ev::Play => continue,
            Ev::Engage | Ev::Leave => {
                if let Some(o) = open.as_mut() {
                    o.4 = e.ev == Ev::Engage;
                }
                continue;
            }
            Ev::FightStart => {
                open = Some((clean(&e.boss), e.t, 0, e.src == "screen", false));
                line.boss = clean(&e.boss);
            }
            Ev::Death => {
                // as the book counts it: a fight found on screen only has the deaths in its arena
                if let Some(o) = open.as_mut().filter(|o| !o.3 || o.4) {
                    o.2 += 1;
                    o.4 = false;
                    line.boss = o.0.clone();
                }
            }
            Ev::Undo => {
                if let Some(o) = open.as_mut() {
                    o.2 = o.2.saturating_sub(1);
                    line.boss = o.0.clone();
                }
            }
            Ev::Rename => {
                if let Some(o) = open.as_mut() {
                    o.0 = clean(&e.boss);
                    line.boss = o.0.clone();
                }
            }
            Ev::Win | Ev::Abandon => {
                if let Some((boss, started, deaths, _, _)) = open.take() {
                    line.boss = boss;
                    line.deaths = deaths;
                    line.ms = e.t.saturating_sub(started);
                }
            }
        }
        out.push(line);
    }
    out
}

/// How a session went, for its card and the "this session" tiles.
#[derive(Default)]
struct Sum {
    deaths: u32,
    fought: Vec<String>,
    beaten: Vec<String>,
    by_boss: BTreeMap<String, (String, u32)>,
    lines: Vec<usize>,
    /// the boss keys already in `fought` / `beaten`
    seen: std::collections::HashSet<(bool, String)>,
}

impl Sum {
    fn add(&mut self, beaten: bool, name: &str) {
        if !name.is_empty() && self.seen.insert((beaten, boss_key(name))) {
            if beaten { &mut self.beaten } else { &mut self.fought }.push(name.to_string());
        }
    }
}

/// Deaths still counted, per death screen that saw them (`Event.by`): (deaths, the last one's time).
pub fn by_screen(events: &[Event], b: &Book) -> BTreeMap<String, (u32, u64)> {
    let counted: std::collections::HashSet<u64> = b.deaths.iter().map(|d| d.0).collect();
    let mut out: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    for e in events.iter().filter(|e| e.ev == Ev::Death && !e.by.is_empty() && counted.contains(&e.t)) {
        let c = out.entry(e.by.clone()).or_default();
        c.0 += 1;
        c.1 = c.1.max(e.t);
    }
    out
}

/// The game's numbers for the page: `tries`, `session`, `bosses`, `sessions`, `records`.
/// `live`: the session being recorded now for this game (start, last seen).
pub fn game_stats(events: &[Event], b: &Book, notes: &BTreeMap<String, String>, live: Option<(u64, u64)>, now: u64) -> Map<String, Value> {
    let all = stats::tries(b);
    let mut sess = stats::sessions(b, events);
    if let Some((start, seen)) = live {
        match sess.last_mut() {
            Some(s) if start < s.end + stats::SESSION_GAP_MS => s.end = s.end.max(seen),
            _ => sess.push(Session { start, end: seen, estimated: false }),
        }
    }
    let mut m = Map::new();

    // the open fight's last tries, else the game's
    let open = b.fights.iter().rposition(|f| f.ended.is_none());
    let pick: Vec<u64> = all.iter().filter(|t| open.is_none() || t.fight == open).map(|t| t.ms).collect();
    m.insert("tries".into(), json!(pick[pick.len().saturating_sub(10)..]));

    // each event to the session it happened in (the last one started before it)
    let ls = lines(events);
    let mut sums: Vec<Sum> = sess.iter().map(|_| Sum::default()).collect();
    let at = |t: u64| sess.partition_point(|s| s.start <= t).saturating_sub(1);
    for (n, l) in ls.iter().enumerate() {
        if sums.is_empty() {
            break;
        }
        let s = &mut sums[at(l.e.t)];
        s.lines.push(n);
        match l.e.ev {
            Ev::Death => {
                s.deaths += 1;
                s.add(false, &l.boss);
                if !l.boss.is_empty() {
                    s.by_boss.entry(boss_key(&l.boss)).or_insert_with(|| (l.boss.clone(), 0)).1 += 1;
                }
            }
            Ev::Undo => s.deaths = s.deaths.saturating_sub(1),
            Ev::FightStart => s.add(false, &l.boss),
            Ev::Win => s.add(true, &l.boss),
            _ => {}
        }
    }
    let line_json = |l: &Line| json!({ "i": l.i, "t": l.e.t, "ev": l.e.ev, "boss": l.boss, "src": l.e.src, "deaths": l.deaths, "ms": l.ms, "hp": l.e.hp });
    let mut list = vec![];
    for (s, sum) in sess.iter().zip(&sums).rev().take(MAX_SESSIONS) {
        let len = s.end.saturating_sub(s.start);
        let bucket = (15 * 60_000u64).max(len.div_ceil(48));
        let mut per = vec![0u32; (len / bucket + 1) as usize];
        let from = b.deaths.partition_point(|d| d.0 < s.start);
        for d in b.deaths[from..].iter().take_while(|d| d.0 <= s.end.max(s.start)) {
            let i = (d.0.saturating_sub(s.start) / bucket) as usize;
            let last = per.len() - 1;
            per[i.min(last)] += 1;
        }
        list.push(json!({
            "start": s.start, "end": s.end, "estimated": s.estimated, "deaths": sum.deaths,
            "fought": sum.fought, "beaten": sum.beaten, "per15": per,
            "events": sum.lines.iter().rev().take(MAX_EVENTS).map(|n| line_json(&ls[*n])).collect::<Vec<_>>(),
        }));
    }
    m.insert("sessions".into(), Value::Array(list));

    // the latest session, for the Now tab
    let latest = sess.last().zip(sums.last()).map(|(s, sum)| {
        let ts: Vec<&Try> = all.iter().filter(|t| t.end >= s.start && t.end <= s.end.max(s.start) && t.ms > 0).collect();
        let pace = if ts.is_empty() { 0 } else { ts.iter().map(|t| t.ms).sum::<u64>() / ts.len() as u64 };
        json!({
            "start": s.start, "ms": s.end.saturating_sub(s.start), "deaths": sum.deaths, "bosses": sum.fought.len(),
            "beaten": sum.beaten.len(), "pace_ms": pace, "estimated": s.estimated,
            "deadliest": sum.by_boss.values().max_by_key(|v| v.1).map(|v| v.0.clone()),
            "live": live.is_some_and(|(start, _)| start >= s.start),
        })
    });
    m.insert("session".into(), latest.unwrap_or(Value::Null));

    // bosses, with their tries
    let mut by_boss: BTreeMap<String, Vec<&Try>> = BTreeMap::new();
    for t in &all {
        if let Some(i) = t.fight {
            by_boss.entry(boss_key(&b.fights[i].boss)).or_default().push(t);
        }
    }
    let mut last_seen: BTreeMap<String, u64> = BTreeMap::new();
    for f in &b.fights {
        let e = last_seen.entry(boss_key(&f.boss)).or_default();
        *e = (*e).max(f.started).max(f.ended.unwrap_or(0));
    }
    let bosses: Vec<Value> = b
        .bosses(now)
        .into_iter()
        .map(|s| {
            let k = boss_key(&s.boss);
            let ts = by_boss.get(&k).map(Vec::as_slice).unwrap_or(&[]);
            let won = ts.iter().position(|t| t.won);
            let mut v = serde_json::to_value(&s).unwrap_or_default();
            if let Some(o) = v.as_object_mut() {
                o.insert("best_try_ms".into(), json!(ts.iter().map(|t| t.ms).max().unwrap_or(0)));
                o.insert("tries_ms".into(), json!(ts[ts.len().saturating_sub(200)..].iter().map(|t| t.ms).collect::<Vec<_>>()));
                o.insert("won_on".into(), json!(won.map(|i| i + 1)));
                o.insert("won_at".into(), json!(won.map(|i| ts[i].end)));
                o.insert("last".into(), json!(last_seen.get(&k).copied().unwrap_or(0).max(ts.last().map_or(0, |t| t.end))));
                o.insert("note".into(), json!(notes.get(&k).cloned().unwrap_or_default()));
            }
            v
        })
        .collect();
    m.insert("bosses".into(), Value::Array(bosses));
    m.insert("records".into(), serde_json::to_value(stats::records(b, &sess)).unwrap_or_default());
    m
}

/// "2026-10-20 21:40:05" for unix ms `t`, `offset_min` minutes east of UTC.
pub fn local_iso(t: u64, offset_min: i64) -> String {
    let secs = (t / 1000) as i64 + offset_min * 60;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A spreadsheet of every event (but play lines) of `games` (name, events): time, game, event,
/// boss, source.
pub fn csv(games: &[(String, &[Event])], offset_min: i64) -> String {
    let mut out = String::from("time,game,event,boss,source\r\n");
    for (name, events) in games {
        for l in lines(events) {
            let ev = serde_json::to_value(l.e.ev).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            out += &format!("{},{},{},{},{}\r\n", local_iso(l.e.t, offset_min), csv_field(name), ev, csv_field(&l.boss), csv_field(&l.e.src));
        }
    }
    out
}

/// Weeks since the epoch, starting on Mondays (1 Jan 1970 was a Thursday).
fn week(t: u64) -> u64 {
    (t / 86_400_000 + 3) / 7
}

/// Totals across every game: (key, name, book, events) each.
pub fn overall(games: &[(String, String, &Book, &[Event])], now: u64) -> Value {
    let (mut deaths, mut played, mut beaten) = (0u64, 0u64, 0u64);
    let mut per_game = vec![];
    let mut bosses = vec![];
    let this_week = week(now);
    let mut weeks = [0u32; 12];
    for (key, name, b, events) in games {
        let n = b.total_deaths();
        deaths += n as u64;
        beaten += b.wins.len() as u64;
        played += stats::sessions(b, events).iter().map(|s| s.end.saturating_sub(s.start)).sum::<u64>();
        per_game.push((n, json!({ "key": key, "name": name, "deaths": n })));
        for s in b.bosses(0) {
            bosses.push((s.deaths, json!({ "game": name, "boss": s.boss, "deaths": s.deaths })));
        }
        for d in &b.deaths {
            let ago = this_week.saturating_sub(week(d.0));
            if ago < 12 && d.0 <= now {
                weeks[11 - ago as usize] += 1;
            }
        }
    }
    per_game.sort_by_key(|g| std::cmp::Reverse(g.0));
    bosses.sort_by_key(|g| std::cmp::Reverse(g.0));
    json!({
        "deaths": deaths, "played_ms": played, "beaten": beaten, "weeks": weeks,
        "games": per_game.into_iter().filter(|g| g.0 > 0).take(5).map(|g| g.1).collect::<Vec<_>>(),
        "bosses": bosses.into_iter().filter(|g| g.0 > 0).take(5).map(|g| g.1).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs(t: u64, boss: &str) -> Event {
        Event { fight: format!("f{t}"), boss: boss.into(), ..Event::new(t, Ev::FightStart) }
    }
    fn d(t: u64) -> Event {
        Event::new(t, Ev::Death)
    }
    fn seen(t: u64, by: &str) -> Event {
        Event { src: "screen".into(), by: by.into(), ..Event::new(t, Ev::Death) }
    }

    #[test]
    fn deaths_are_counted_per_death_screen() {
        let ev = vec![seen(10, "taught:1"), d(20), seen(30, "library"), seen(40, "taught:1"), seen(50, "taught:1"), Event::new(51, Ev::Undo)];
        let b = Book::replay(&ev);
        let c = by_screen(&ev, &b);
        assert_eq!(c.get("taught:1"), Some(&(2, 40)), "the undone one doesn't count");
        assert_eq!(c.get("library"), Some(&(1, 30)));
        assert_eq!(c.len(), 2, "deaths counted by hand aren't a screen's");
    }
    fn win(t: u64) -> Event {
        Event::new(t, Ev::Win)
    }
    fn play(t: u64, ms: u64) -> Event {
        Event { ms, ..Event::new(t, Ev::Play) }
    }
    const H: u64 = 3_600_000;

    /// Two sessions a day apart: Margit (3 deaths, unbeaten), then Godrick (1 death, beaten).
    fn two_sessions() -> Vec<Event> {
        vec![
            play(0, H),
            fs(60_000, "Margit"),
            d(120_000),
            d(260_000),
            d(300_000),
            play(24 * H, H),
            fs(24 * H + 60_000, "Godrick"),
            d(24 * H + 90_000),
            win(24 * H + 200_000),
        ]
    }

    fn stats_of(ev: &[Event]) -> Value {
        Value::Object(game_stats(ev, &Book::replay(ev), &BTreeMap::new(), None, 30 * H))
    }

    #[test]
    fn the_game_json_has_sessions_tries_and_records() {
        let v = stats_of(&two_sessions());
        let s = v["sessions"].as_array().unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0]["deaths"].as_u64(), s[1]["deaths"].as_u64()), (Some(1), Some(3)), "newest first");
        assert_eq!(s[0]["beaten"], json!(["Godrick"]));
        assert_eq!(s[1]["fought"], json!(["Margit"]));
        assert!(s[0]["events"].as_array().unwrap().iter().all(|e| e["ev"] != "play"), "play lines aren't events to show");
        assert_eq!(s[0]["events"][0]["ev"], "win");
        assert_eq!((s[0]["events"][0]["boss"].as_str(), s[0]["events"][0]["deaths"].as_u64()), (Some("Godrick"), Some(1)));
        assert_eq!(s[1]["events"][0]["boss"], "Margit", "a death names the fight's boss");
        assert_eq!(v["tries"], json!([60_000, 140_000, 40_000, 30_000, 110_000]), "no fight open: the game's last tries");
        let margit = v["bosses"].as_array().unwrap().iter().find(|b| b["boss"] == "Margit").unwrap().clone();
        assert_eq!(margit["best_try_ms"], 140_000);
        assert_eq!(margit["tries_ms"], json!([60_000, 140_000, 40_000]));
        let godrick = v["bosses"].as_array().unwrap().iter().find(|b| b["boss"] == "Godrick").unwrap().clone();
        assert_eq!((godrick["won_on"].as_u64(), godrick["won_at"].as_u64()), (Some(2), Some(24 * H + 200_000)));
        assert_eq!(v["records"]["deadliest"], json!(["Margit", 3]));
        assert_eq!(v["session"]["deaths"], 1, "the latest session");
        assert_eq!(v["session"]["live"], false);
    }

    #[test]
    fn an_open_fight_shows_its_own_tries() {
        let ev = [fs(0, "A"), d(10_000), d(30_000), fs(40_000, "B"), d(45_000)];
        assert_eq!(stats_of(&ev)["tries"], json!([5_000]));
    }

    #[test]
    fn the_live_session_counts_until_now() {
        let ev = [fs(0, "A"), d(10_000)];
        let v = Value::Object(game_stats(&ev, &Book::replay(&ev), &BTreeMap::new(), Some((0, 20 * 60_000)), 20 * 60_000));
        assert_eq!((v["session"]["live"].as_bool(), v["session"]["ms"].as_u64()), (Some(true), Some(20 * 60_000)));
    }

    #[test]
    fn notes_are_kept_per_boss_whatever_the_spelling() {
        let mut n = BTreeMap::new();
        set_note(&mut n, "  Margit,  the Fell ", "  dodge left  ");
        assert_eq!(n.get(&boss_key("margit, the fell")).map(String::as_str), Some("dodge left"));
        set_note(&mut n, "Margit, the Fell", &"x".repeat(900));
        assert_eq!(n.values().next().unwrap().chars().count(), NOTE_MAX);
        set_note(&mut n, "MARGIT, THE FELL", "   ");
        assert!(n.is_empty(), "an empty note removes it");
        let ev = [fs(0, "Margit, the Fell"), d(5)];
        let mut n = BTreeMap::new();
        set_note(&mut n, "margit, the fell", "hi");
        let v = Value::Object(game_stats(&ev, &Book::replay(&ev), &n, None, 10));
        assert_eq!(v["bosses"][0]["note"], "hi");
    }

    #[test]
    fn state_json_for_a_huge_history_is_quick() {
        let ev: Vec<Event> = (0..40_000u64).map(|i| if i % 50 == 0 { fs(i * 60_000, &format!("B{}", i / 500)) } else { d(i * 60_000) }).collect();
        let b = Book::replay(&ev);
        let t = std::time::Instant::now();
        let v = Value::Object(game_stats(&ev, &b, &BTreeMap::new(), None, 40_001 * 60_000));
        let took = t.elapsed();
        // a debug build with the other tests running; a release build is about 10x faster. What this
        // catches is work that grows with the square of the history (it once took 2.3 s).
        assert!(took < std::time::Duration::from_millis(400), "{took:?}");
        assert!(v["sessions"].as_array().unwrap().len() <= MAX_SESSIONS);
        assert!(v["sessions"].as_array().unwrap().iter().all(|s| s["events"].as_array().unwrap().len() <= MAX_EVENTS));
    }

    #[test]
    fn export_csv_quotes_and_lists_every_event() {
        let ev = [fs(0, "He said \"hi\", twice"), d(1_000), play(0, 5_000)];
        let out = csv(&[("Game, the".into(), &ev)], 120);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "time,game,event,boss,source");
        assert_eq!(rows.len(), 3, "the play line isn't exported");
        assert_eq!(rows[1], r#"1970-01-01 02:00:00,"Game, the",fight_start,"He said ""hi"", twice","#);
        assert!(rows[2].starts_with("1970-01-01 02:00:01,\"Game, the\",death,"));
        assert_eq!(local_iso(1_792_446_505_205, 180), "2026-10-20 00:48:25");
    }

    #[test]
    fn totals_add_up_across_games_and_weeks() {
        let a = two_sessions();
        let b = vec![fs(0, "X"), d(1_000), d(2_000), d(3_000), d(4_000), d(5_000)];
        let (ba, bb) = (Book::replay(&a), Book::replay(&b));
        let v = overall(&[("exe:a".into(), "A".into(), &ba, &a), ("exe:b".into(), "B".into(), &bb, &b)], 30 * H);
        assert_eq!((v["deaths"].as_u64(), v["beaten"].as_u64()), (Some(9), Some(1)));
        assert_eq!(v["games"][0]["name"], "B", "deadliest first");
        assert_eq!(v["bosses"][0]["boss"], "X");
        assert_eq!(v["weeks"].as_array().unwrap().len(), 12);
        assert_eq!(v["weeks"].as_array().unwrap().iter().map(|w| w.as_u64().unwrap()).sum::<u64>(), 9, "all within the last 12 weeks");
        assert!(v["played_ms"].as_u64().unwrap() >= 2 * H);
    }
}
