//! What a game's history says beyond the counts: tries (how long each attempt lasted), play
//! sessions and records. Pure: everything here is worked out from a Book and its events.

use super::book::{boss_key, Book, Ev, Event};
use serde::Serialize;

/// A try longer than this spans a break or a restart: its length is unknown (0).
pub const MAX_TRY_MS: u64 = 30 * 60_000;
/// Away from the game this long ends a play session.
pub const SESSION_GAP_MS: u64 = 15 * 60_000;
/// Old history without play lines: a gap this long between events starts another session.
pub const ESTIMATE_GAP_MS: u64 = 60 * 60_000;

/// One attempt: from the fight's start (or the last death or win in it) to a death or a win.
#[derive(Clone, Debug, PartialEq)]
pub struct Try {
    pub fight: Option<usize>,
    pub end: u64,
    /// 0 when unknown
    pub ms: u64,
    pub won: bool,
}

/// Every try still counted, by when it ended. A death taken back with undo has no try.
pub fn tries(b: &Book) -> Vec<Try> {
    let len = |from: u64, to: u64| Some(to.saturating_sub(from)).filter(|ms| *ms <= MAX_TRY_MS).unwrap_or(0);
    let mut out = vec![];
    // each fight's deaths and wins, in one pass (years of history: tens of thousands of deaths)
    let mut by_fight: Vec<Vec<(u64, bool)>> = vec![vec![]; b.fights.len()];
    for d in &b.deaths {
        if let Some(i) = d.1 {
            by_fight[i].push((d.0, false));
        }
    }
    for w in &b.wins {
        by_fight[w.1].push((w.0, true));
    }
    for (i, (f, mut ends)) in b.fights.iter().zip(by_fight).enumerate() {
        ends.sort();
        let mut from = f.started;
        for (end, won) in ends {
            out.push(Try { fight: Some(i), end, ms: len(from, end), won });
            from = from.max(end);
        }
    }
    let mut from: Option<u64> = None;
    for (end, _) in b.deaths.iter().filter(|d| d.1.is_none()) {
        out.push(Try { fight: None, end: *end, ms: from.map_or(0, |f| len(f, *end)), won: false });
        from = Some(from.map_or(*end, |f| f.max(*end)));
    }
    out.sort_by_key(|t| t.end);
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub start: u64,
    pub end: u64,
    /// worked out from the events' times (history from before play sessions were recorded)
    pub estimated: bool,
}

/// Play sessions, oldest first: the recorded ones (lines less than SESSION_GAP_MS apart are one),
/// and before the first recorded one, sessions worked out from the events' times.
pub fn sessions(b: &Book, events: &[Event]) -> Vec<Session> {
    let first_play = b.plays.first().map(|p| p.0);
    let mut times: Vec<u64> = events.iter().filter(|e| e.ev != Ev::Play && first_play.is_none_or(|p| e.t < p)).map(|e| e.t).collect();
    times.sort_unstable();
    let mut out: Vec<Session> = vec![];
    for t in times {
        match out.last_mut() {
            Some(s) if t.saturating_sub(s.end) <= ESTIMATE_GAP_MS => s.end = s.end.max(t),
            _ => out.push(Session { start: t, end: t, estimated: true }),
        }
    }
    let n = out.len();
    for &(start, ms) in &b.plays {
        let end = start + ms;
        let recorded = out.len() > n;
        match out.last_mut() {
            Some(s) if recorded && start < s.end + SESSION_GAP_MS => s.end = s.end.max(end),
            _ => out.push(Session { start, end, estimated: false }),
        }
    }
    out
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Records {
    pub longest_try_ms: u64,
    pub longest_try_boss: Option<String>,
    pub fewest_to_win: Option<(String, u32)>,
    pub deadliest: Option<(String, u32)>,
    pub most_in_session: u32,
}

pub fn records(b: &Book, sessions: &[Session]) -> Records {
    let mut r = Records::default();
    for t in tries(b) {
        if t.ms > r.longest_try_ms {
            r.longest_try_ms = t.ms;
            r.longest_try_boss = t.fight.map(|i| b.fights[i].boss.clone());
        }
    }
    let bosses = b.bosses(0);
    r.fewest_to_win = bosses.iter().filter_map(|s| s.deaths_to_first_win.map(|d| (s.boss.clone(), d))).min_by_key(|x| x.1);
    r.deadliest = bosses.iter().filter(|s| s.deaths > 0).max_by_key(|s| s.deaths).map(|s| (s.boss.clone(), s.deaths));
    r.most_in_session = sessions.iter().map(|s| b.deaths.iter().filter(|d| d.0 >= s.start && d.0 <= s.end).count() as u32).max().unwrap_or(0);
    r
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Something worth saying about the latest death: a milestone (1st, 10th, 50th, 100th, then every
/// 100th death of the game), else "Your longest try yet" (the longest of at least 3 tries on this boss).
pub fn death_note(b: &Book) -> Option<String> {
    let n = b.total_deaths();
    if n == 0 {
        return None;
    }
    if matches!(n, 1 | 10 | 50) || n.is_multiple_of(100) {
        return Some(format!("{} death in this game", ordinal(n)));
    }
    let all = tries(b);
    let (when, fight) = *b.deaths.last()?;
    let last = all.iter().find(|t| t.end == when && t.fight == fight && !t.won)?;
    let boss = boss_key(&b.fights.get(fight?)?.boss);
    let same: Vec<&Try> = all.iter().filter(|t| t.fight.is_some_and(|i| boss_key(&b.fights[i].boss) == boss)).collect();
    let longest_before = same.iter().filter(|t| !std::ptr::eq(**t, last)).map(|t| t.ms).max().unwrap_or(0);
    (same.len() >= 3 && last.ms > 0 && last.ms > longest_before).then(|| "Your longest try yet".to_string())
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
    fn win(t: u64) -> Event {
        Event::new(t, Ev::Win)
    }
    fn undo(t: u64) -> Event {
        Event::new(t, Ev::Undo)
    }
    fn play(t: u64, ms: u64) -> Event {
        Event { ms, ..Event::new(t, Ev::Play) }
    }
    fn book(ev: &[Event]) -> Book {
        Book::replay(ev)
    }

    #[test]
    fn a_try_runs_from_the_fight_start_or_the_last_death() {
        let t = tries(&book(&[fs(0, "A"), d(60_000), d(200_000), win(260_000)]));
        assert_eq!(t.iter().map(|t| t.ms).collect::<Vec<_>>(), vec![60_000, 140_000, 60_000]);
        assert!(t[2].won && !t[0].won);
        assert_eq!(t[0].fight, Some(0));
    }

    #[test]
    fn tries_survive_events_out_of_order() {
        // the PC's clock went back between the fight's start and the first death
        let t = tries(&book(&[fs(100_000, "A"), d(50_000), d(400_000)]));
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|t| t.ms <= MAX_TRY_MS), "{t:?}");
    }

    #[test]
    fn a_try_longer_than_half_an_hour_is_unknown() {
        let t = tries(&book(&[fs(0, "A"), d(31 * 60_000)]));
        assert_eq!(t[0].ms, 0);
    }

    #[test]
    fn undo_removes_the_try_of_the_death_it_takes_back() {
        let t = tries(&book(&[fs(0, "A"), d(10_000), d(20_000), undo(21_000)]));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn loose_deaths_are_tries_too() {
        let t = tries(&book(&[d(0), d(90_000)]));
        assert_eq!(t.iter().map(|t| (t.fight, t.ms)).collect::<Vec<_>>(), vec![(None, 0), (None, 90_000)]);
    }

    #[test]
    fn play_lines_a_few_minutes_apart_are_one_session() {
        let ev = [play(0, 600_000), play(900_000, 600_000), play(10_000_000, 60_000)];
        let s = sessions(&book(&ev), &ev);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].start, s[0].end, s[0].estimated), (0, 1_500_000, false));
        assert_eq!((s[1].start, s[1].end), (10_000_000, 10_060_000));
    }

    #[test]
    fn old_history_gets_estimated_sessions_split_at_an_hour() {
        let ev = [d(0), d(10 * 60_000), d(3 * 3_600_000)];
        let s = sessions(&book(&ev), &ev);
        assert_eq!(s.len(), 2);
        assert!(s[0].estimated);
        assert_eq!((s[0].start, s[0].end), (0, 10 * 60_000));
    }

    #[test]
    fn history_before_the_first_play_line_is_estimated_and_after_it_is_not() {
        let ev = [d(0), play(10 * 3_600_000, 60_000), d(10 * 3_600_000 + 1_000)];
        let s = sessions(&book(&ev), &ev);
        assert_eq!(s.iter().map(|s| s.estimated).collect::<Vec<_>>(), vec![true, false]);
    }

    #[test]
    fn the_hundredth_death_is_a_milestone_and_wins_over_a_record() {
        let mut ev = vec![fs(0, "A")];
        ev.extend((1..100).map(|i| d(i * 1_000)));
        ev.push(d(99_000 + 600_000)); // the longest try, and the 100th death
        assert_eq!(death_note(&book(&ev)).as_deref(), Some("100th death in this game"));
        assert_eq!(death_note(&book(&[d(5)])).as_deref(), Some("1st death in this game"));
        assert_eq!(death_note(&book(&ev[..12])), None, "11 isn't a milestone");
    }

    #[test]
    fn the_longest_try_yet_is_noted() {
        assert_eq!(death_note(&book(&[fs(0, "A"), d(10_000), d(20_000), d(80_000)])).as_deref(), Some("Your longest try yet"));
        assert_eq!(death_note(&book(&[fs(0, "A"), d(60_000), d(70_000), d(80_000)])), None, "a short one says nothing");
        assert_eq!(death_note(&book(&[fs(0, "A"), d(10_000), d(90_000)])), None, "two tries are too few to call it a record");
    }

    #[test]
    fn records_name_the_deadliest_boss_and_the_fewest_deaths_to_win() {
        let ev = [fs(0, "Margit"), d(1_000), d(2_000), d(400_000), fs(500_000, "Godrick"), d(501_000), win(502_000)];
        let b = book(&ev);
        let r = records(&b, &sessions(&b, &ev));
        assert_eq!(r.deadliest, Some(("Margit".into(), 3)));
        assert_eq!(r.fewest_to_win, Some(("Godrick".into(), 1)));
        assert_eq!((r.longest_try_ms, r.longest_try_boss.as_deref()), (398_000, Some("Margit")));
        assert_eq!(r.most_in_session, 4);
    }
}
