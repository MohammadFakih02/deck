//! One game's death history: the events as saved, and everything worked out from them
//! (the open fight, each boss's attempts, wins and time). Nothing here touches the disk.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ev {
    FightStart,
    Death,
    /// takes back the latest death still counted
    Undo,
    Win,
    Abandon,
    /// gives the fight another boss name
    Rename,
    /// a play session: `t` its start, `ms` how long so far (a checkpoint is rewritten in place)
    Play,
    /// the boss's health bar showed up (`fight`; `boss`: the name read on it): a try starts
    Engage,
    /// the boss's health bar went away without a death or a win (`fight`)
    Leave,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// One line of a game's history file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    /// unix time in ms
    pub t: u64,
    pub ev: Ev,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fight: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub boss: String,
    /// where a death came from: "manual", "hotkey", "pad", "screen"
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub src: String,
    /// a play session's length so far
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ms: u64,
    /// which death screen counted a death Deck saw: "words", "taught:<when it was taught>", "library"
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    /// a death with the boss's health bar on screen: the health it had left, %
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hp: Option<u8>,
    /// …and which phase of the fight it was in (2 and up; none: the first)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<u8>,
}

impl Event {
    pub fn new(t: u64, ev: Ev) -> Event {
        Event { t, ev, fight: String::new(), boss: String::new(), src: String::new(), ms: 0, by: String::new(), hp: None, phase: None }
    }
}

/// `Fight.hp` for a death without the boss's health bar on screen
pub const NO_HP: u8 = u8::MAX;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Fight {
    pub id: String,
    pub boss: String,
    pub started: u64,
    /// when it was won or abandoned
    pub ended: Option<u64>,
    pub won: bool,
    pub deaths: u32,
    /// opened by the screen watcher (it saw the boss's health bar), not by hand
    pub auto: bool,
    /// names read on its boss bar (boss keys)
    #[serde(skip)]
    pub seen: Vec<String>,
    /// since when the boss bar has been up in the try going on now (None: not in the fight now)
    #[serde(skip)]
    pub engaged: Option<u64>,
    /// when each try's boss bar showed up
    #[serde(skip)]
    pub engages: Vec<u64>,
    /// time with the boss bar up in tries that ended, ms
    #[serde(skip)]
    pub fight_ms: u64,
    /// the fight's phase and the boss's health (%, NO_HP: not seen) at each of its deaths
    #[serde(skip)]
    pub hp: Vec<(u8, u8)>,
}

impl Fight {
    /// The try in the boss's arena ended (a death, a win, the bar went away).
    fn disengage(&mut self, t: u64) {
        if let Some(at) = self.engaged.take() {
            self.fight_ms += t.saturating_sub(at).min(super::stats::MAX_TRY_MS);
        }
    }

    /// A name read on a bar is a misreading of this fight's boss (`like_boss`).
    pub fn misread(&self, name: &str) -> bool {
        like_boss(&self.boss, name) || self.seen.iter().any(|s| like_boss(s, name))
    }

    /// A boss bar named `name` belongs to this fight: its name, a name read on its bar before,
    /// or any name for a fight opened by hand that hasn't seen a bar yet (you typed "Margit", the
    /// bar says "Margit, the Fell Omen").
    pub fn fits(&self, name: &str) -> bool {
        same_boss(&self.boss, name) || self.seen.iter().any(|s| same_boss(s, name)) || (!self.auto && self.seen.is_empty())
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct BossStats {
    pub boss: String,
    /// every death and every win ends an attempt
    pub attempts: u32,
    pub deaths: u32,
    pub wins: u32,
    /// deaths it took to win the first time; None until it's beaten
    pub deaths_to_first_win: Option<u32>,
    /// time spent in its fights, ms: with the boss bar up where Deck saw it, else from start to end
    pub time_ms: u64,
    pub fighting: bool,
    /// your closest try (where Deck saw its bar): the furthest phase, then the least health left (%)
    pub closest: Option<(u8, u8)>,
}

/// A game's history worked out from its events.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub fights: Vec<Fight>,
    /// deaths with no fight open
    pub loose: u32,
    /// each death still counted, in order: (when, Some(fight index) or None (loose)); undo takes the last
    pub deaths: Vec<(u64, Option<usize>)>,
    /// each win: (when, fight index)
    pub wins: Vec<(u64, usize)>,
    /// play sessions: (start, ms), one per start (the longest line wins), by start
    pub plays: Vec<(u64, u64)>,
}

/// Bosses are the same whatever the case or spacing.
pub fn boss_key(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Two names are one boss: the same, one the start of the other ("Margit" and "Margit, the Fell
/// Omen"), or a letter or two apart (the screen's reading of a name may slip).
pub fn same_boss(a: &str, b: &str) -> bool {
    let (a, b) = (boss_key(a), boss_key(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (short, long) = if a.chars().count() <= b.chars().count() { (&a, &b) } else { (&b, &a) };
    let n = short.chars().count();
    a == b || (n >= 5 && long.starts_with(short.as_str())) || super::screen::detect::edits(&a, &b) <= (n / 6).max(1)
}

/// The longest run of letters two names share in order ("xaldin" and "aldii": "aldi").
pub(crate) fn common(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row = vec![0usize; b.len() + 1];
    for x in &a {
        let mut prev = 0;
        for (j, y) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if x == y { prev + 1 } else { row[j + 1].max(row[j]) };
            prev = cur;
        }
    }
    row[b.len()]
}

/// A name read badly that is most likely `known` ("aldii" for "Xaldin"): most of its letters in
/// the same order. Only for a fight's own bar coming back; never to tell bosses apart.
pub fn like_boss(known: &str, read: &str) -> bool {
    let (a, b) = (boss_key(known), boss_key(read));
    let n = a.chars().count().max(b.chars().count());
    a.chars().count().min(b.chars().count()) >= 4 && common(&a, &b) * 10 >= n * 6
}

/// Every event about boss `from` becomes about `into` (a misread name, a boss under two names);
/// how many changed.
pub fn merge_boss(events: &mut [Event], from: &str, into: &str) -> usize {
    let k = boss_key(from);
    let mut n = 0;
    for e in events.iter_mut().filter(|e| !e.boss.is_empty() && boss_key(&e.boss) == k) {
        e.boss = into.to_string();
        n += 1;
    }
    n
}

impl Book {
    pub fn replay(events: &[Event]) -> Book {
        let mut b = Book::default();
        for e in events {
            b.apply(e);
        }
        b
    }

    pub fn apply(&mut self, e: &Event) {
        match e.ev {
            Ev::FightStart => {
                if let Some(f) = self.open_mut() {
                    f.ended = Some(e.t); // a new fight leaves the old one
                    f.disengage(e.t);
                }
                let boss = e.boss.split_whitespace().collect::<Vec<_>>().join(" ");
                self.fights.push(Fight {
                    id: e.fight.clone(),
                    boss,
                    started: e.t,
                    ended: None,
                    won: false,
                    deaths: 0,
                    auto: e.src == "screen",
                    seen: vec![],
                    engaged: None,
                    engages: vec![],
                    fight_ms: 0,
                    hp: vec![],
                });
            }
            Ev::Death => {
                // a fight the screen found counts deaths in its arena: dying on the way back is the game's
                let at = self.open_index().filter(|i| !self.fights[*i].auto || self.fights[*i].engaged.is_some());
                match at {
                    Some(i) => {
                        let f = &mut self.fights[i];
                        f.deaths += 1;
                        f.hp.push((e.phase.unwrap_or(1).max(1), e.hp.unwrap_or(NO_HP)));
                        f.disengage(e.t);
                    }
                    None => self.loose += 1,
                }
                self.deaths.push((e.t, at));
            }
            Ev::Undo => match self.deaths.pop() {
                Some((_, Some(i))) => {
                    self.fights[i].deaths -= 1;
                    self.fights[i].hp.pop();
                }
                Some((_, None)) => self.loose -= 1,
                None => {}
            },
            Ev::Win | Ev::Abandon => {
                let won = e.ev == Ev::Win;
                if let Some(i) = self.open_index() {
                    self.fights[i].ended = Some(e.t);
                    self.fights[i].won = won;
                    self.fights[i].disengage(e.t);
                    if won {
                        self.wins.push((e.t, i));
                    }
                }
            }
            Ev::Engage => {
                if let Some(f) = self.fights.iter_mut().rev().find(|f| f.id == e.fight && f.ended.is_none()) {
                    f.engages.push(e.t);
                    f.engaged.get_or_insert(e.t);
                    let k = boss_key(&e.boss);
                    if !k.is_empty() && !f.seen.contains(&k) {
                        f.seen.push(k);
                    }
                }
            }
            Ev::Leave => {
                if let Some(f) = self.fights.iter_mut().rev().find(|f| f.id == e.fight) {
                    f.disengage(e.t);
                }
            }
            Ev::Play => match self.plays.binary_search_by_key(&e.t, |p| p.0) {
                Ok(i) => self.plays[i].1 = self.plays[i].1.max(e.ms),
                Err(i) => self.plays.insert(i, (e.t, e.ms)),
            },
            Ev::Rename => {
                if let Some(f) = self.fights.iter_mut().find(|f| f.id == e.fight) {
                    f.boss = e.boss.split_whitespace().collect::<Vec<_>>().join(" ");
                }
            }
        }
    }

    fn open_index(&self) -> Option<usize> {
        self.fights.iter().rposition(|f| f.ended.is_none())
    }

    fn open_mut(&mut self) -> Option<&mut Fight> {
        let i = self.open_index()?;
        Some(&mut self.fights[i])
    }

    pub fn open(&self) -> Option<&Fight> {
        self.open_index().map(|i| &self.fights[i])
    }

    /// Deaths on this game, in fights or not.
    pub fn total_deaths(&self) -> u32 {
        self.deaths.len() as u32
    }

    /// Something to undo.
    pub fn can_undo(&self) -> bool {
        !self.deaths.is_empty()
    }

    /// Each boss, the latest fought first. `now` closes the open fight's time.
    pub fn bosses(&self, now: u64) -> Vec<BossStats> {
        let mut out: Vec<(String, BossStats, u64)> = vec![];
        for f in &self.fights {
            let k = boss_key(&f.boss);
            let i = match out.iter().position(|o| o.0 == k) {
                Some(i) => i,
                None => {
                    out.push((k, BossStats::default(), 0));
                    out.len() - 1
                }
            };
            let (_, s, last) = &mut out[i];
            s.boss = f.boss.clone(); // the latest spelling
            s.deaths += f.deaths;
            s.attempts += f.deaths + f.won as u32;
            if f.won {
                s.wins += 1;
                s.deaths_to_first_win.get_or_insert(s.deaths);
            }
            s.time_ms += if f.engages.is_empty() {
                f.ended.unwrap_or(now).saturating_sub(f.started)
            } else {
                f.fight_ms + f.engaged.map_or(0, |at| now.saturating_sub(at).min(super::stats::MAX_TRY_MS))
            };
            // furthest phase first, then least health: (phase, 255 - hp) is largest
            let best = |a: (u8, u8)| (a.0, NO_HP - a.1);
            if let Some(c) = f.hp.iter().copied().filter(|p| p.1 != NO_HP).max_by_key(|p| best(*p)) {
                s.closest = Some(s.closest.filter(|o| best(*o) >= best(c)).unwrap_or(c));
            }
            s.fighting |= f.ended.is_none();
            *last = f.started;
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.2));
        out.into_iter().map(|o| o.1).collect()
    }

    /// Boss names fought on this game, latest first (for the name box's suggestions).
    pub fn boss_names(&self) -> Vec<String> {
        self.bosses(0).into_iter().map(|b| b.boss).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: u64, ev: Ev, fight: &str, boss: &str) -> Event {
        Event { t, ev, fight: fight.into(), boss: boss.into(), ..Event::new(t, ev) }
    }

    #[test]
    fn a_play_line_saves_its_length() {
        let p = Event { ms: 60_000, ..Event::new(5, Ev::Play) };
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(s, r#"{"t":5,"ev":"play","ms":60000}"#);
        assert_eq!(serde_json::from_str::<Event>(&s).unwrap(), p);
        assert_eq!(serde_json::to_string(&Event::new(1, Ev::Death)).unwrap(), r#"{"t":1,"ev":"death"}"#, "no ms on a death");
    }

    #[test]
    fn the_longest_play_line_per_start_is_kept() {
        let play = |t: u64, ms: u64| Event { ms, ..Event::new(t, Ev::Play) };
        let b = Book::replay(&[play(100, 300_000), play(0, 5), play(100, 600_000), play(100, 200_000)]);
        assert_eq!(b.plays, vec![(0, 5), (100, 600_000)]);
    }

    #[test]
    fn deaths_count_toward_the_open_fight() {
        let b = Book::replay(&[
            ev(0, Ev::FightStart, "f1", "Tree Sentinel"),
            ev(10, Ev::Death, "", ""),
            ev(20, Ev::Death, "", ""),
        ]);
        let f = b.open().expect("still fighting");
        assert_eq!((f.boss.as_str(), f.deaths), ("Tree Sentinel", 2));
        assert_eq!(b.total_deaths(), 2);
    }

    #[test]
    fn a_death_with_no_fight_still_counts_for_the_game() {
        let b = Book::replay(&[ev(0, Ev::Death, "", ""), ev(5, Ev::FightStart, "f1", "A"), ev(6, Ev::Win, "", "")]);
        assert_eq!((b.loose, b.total_deaths()), (1, 1));
        assert_eq!(b.open(), None);
    }

    #[test]
    fn undo_takes_back_the_latest_death_wherever_it_was() {
        let mut b = Book::replay(&[ev(0, Ev::Death, "", ""), ev(1, Ev::FightStart, "f1", "A"), ev(2, Ev::Death, "", "")]);
        b.apply(&ev(3, Ev::Undo, "", ""));
        assert_eq!((b.open().unwrap().deaths, b.loose), (0, 1));
        b.apply(&ev(4, Ev::Undo, "", ""));
        assert_eq!((b.loose, b.total_deaths()), (0, 0));
        b.apply(&ev(5, Ev::Undo, "", ""));
        assert_eq!(b.total_deaths(), 0, "nothing left to undo: nothing happens");
        assert!(!b.can_undo());
    }

    /// The fight stays open across retries, so dying on the way back to the boss counts toward it.
    #[test]
    fn a_boss_keeps_its_attempts_wins_and_time_across_fights() {
        let b = Book::replay(&[
            ev(0, Ev::FightStart, "f1", "Malenia"),
            ev(1_000, Ev::Death, "", ""),
            ev(2_000, Ev::Death, "", ""),
            ev(3_000, Ev::Abandon, "", ""),
            ev(10_000, Ev::FightStart, "f2", "  malenia "),
            ev(11_000, Ev::Death, "", ""),
            ev(15_000, Ev::Win, "", ""),
            ev(20_000, Ev::FightStart, "f3", "Malenia"),
            ev(21_000, Ev::Death, "", ""),
        ]);
        let s = &b.bosses(25_000)[0];
        assert_eq!(s.boss, "Malenia");
        assert_eq!((s.deaths, s.wins, s.attempts), (4, 1, 5));
        assert_eq!(s.deaths_to_first_win, Some(3));
        assert_eq!(s.time_ms, 3_000 + 5_000 + 5_000, "the open fight counts up to now");
        assert!(s.fighting);
    }

    #[test]
    fn a_new_fight_closes_the_old_one_without_a_win() {
        let b = Book::replay(&[ev(0, Ev::FightStart, "f1", "A"), ev(1, Ev::Death, "", ""), ev(5, Ev::FightStart, "f2", "B")]);
        assert_eq!(b.fights[0].ended, Some(5));
        assert!(!b.fights[0].won);
        assert_eq!(b.open().unwrap().boss, "B");
        let names = b.boss_names();
        assert_eq!(names, vec!["B", "A"], "latest first");
    }

    #[test]
    fn a_renamed_fight_moves_its_deaths_to_the_other_boss() {
        let b = Book::replay(&[
            ev(0, Ev::FightStart, "f1", "Godrick"),
            ev(1, Ev::Death, "", ""),
            ev(2, Ev::Rename, "f1", "Margit"),
        ]);
        let bs = b.bosses(3);
        assert_eq!(bs.len(), 1);
        assert_eq!((bs[0].boss.as_str(), bs[0].deaths), ("Margit", 1));
    }

    #[test]
    fn an_unbeaten_boss_has_no_deaths_to_first_win() {
        let b = Book::replay(&[ev(0, Ev::FightStart, "f1", "A"), ev(1, Ev::Death, "", "")]);
        assert_eq!(b.bosses(2)[0].deaths_to_first_win, None);
    }

    fn engage(t: u64, fight: &str, boss: &str) -> Event {
        ev(t, Ev::Engage, fight, boss)
    }

    fn auto_fight(t: u64, fight: &str, boss: &str) -> Event {
        Event { src: "screen".into(), ..ev(t, Ev::FightStart, fight, boss) }
    }

    fn death_at(t: u64, hp: u8) -> Event {
        Event { hp: Some(hp), ..Event::new(t, Ev::Death) }
    }

    /// A fight the screen found counts the deaths with its bar up; dying on the way back is the
    /// game's. Its time is the time with the bar up, and the closest try is the least health left.
    #[test]
    fn a_fight_the_screen_found_counts_only_its_arena() {
        let b = Book::replay(&[
            auto_fight(0, "f0", "Margit, the Fell Omen"),
            engage(0, "f0", "Margit, the Fell Omen"),
            death_at(60_000, 40),
            Event::new(90_000, Ev::Death), // on the way back
            engage(120_000, "f0", "Margit, the Fell Omen"),
            death_at(200_000, 12),
            engage(300_000, "f0", "Margit, the Fell Omen"),
            ev(320_000, Ev::Leave, "f0", ""), // ran out of the arena
            Event::new(330_000, Ev::Death),
        ]);
        assert_eq!((b.open().unwrap().deaths, b.loose, b.total_deaths()), (2, 2, 4));
        let s = &b.bosses(400_000)[0];
        assert_eq!(s.time_ms, 60_000 + 80_000 + 20_000, "only with the bar up");
        assert_eq!(s.closest, Some((1, 12)));
        let mut b2 = b.clone();
        b2.apply(&ev(1, Ev::Undo, "", ""));
        b2.apply(&ev(2, Ev::Undo, "", ""));
        b2.apply(&ev(3, Ev::Undo, "", ""));
        assert_eq!(b2.bosses(0)[0].closest, Some((1, 40)), "undo takes the health with the death");
        let mut b3 = b2.clone();
        b3.apply(&engage(500_000, "f0", "Margit, the Fell Omen"));
        b3.apply(&Event { phase: Some(2), ..death_at(600_000, 70) });
        assert_eq!(b3.bosses(0)[0].closest, Some((2, 70)), "phase 2 at 70% is closer than phase 1 at 40%");
    }

    #[test]
    fn a_fight_opened_by_hand_counts_every_death_and_takes_the_bar_name() {
        let mut b = Book::replay(&[ev(0, Ev::FightStart, "f0", "Margit"), Event::new(10, Ev::Death)]);
        assert_eq!(b.open().unwrap().deaths, 1, "no bar seen: every death is the boss's, as before");
        let f = b.open().unwrap();
        assert!(f.fits("Margit, the Fell Omen") && f.fits("Anything at all"), "the first bar is this fight's");
        b.apply(&engage(20, "f0", "Margit, the Fell Omen"));
        let f = b.open().unwrap();
        assert!(f.fits("MARGIT, THE FELL 0MEN") && !f.fits("Godrick the Grafted"));
        b.apply(&Event::new(30, Ev::Death));
        b.apply(&Event::new(40, Ev::Death));
        assert_eq!(b.open().unwrap().deaths, 3, "a fight opened by hand still counts deaths without the bar");
    }

    #[test]
    fn a_misread_name_is_like_the_boss() {
        assert!(like_boss("Xaldin", "aldii"));
        assert!(like_boss("Blade Phantom Ozma", "Blade Phant0m 0zma"));
        assert!(!like_boss("Xaldin", "Viper"));
        assert!(!like_boss("Xaldin", "ald"), "too short to tell");
        assert!(!like_boss("Blade Phantom Ozma", "Xaldin"));
    }

    #[test]
    fn names_read_on_screen_are_the_same_boss_despite_a_slip() {
        assert!(same_boss("Margit", "Margit, the Fell Omen"));
        assert!(same_boss("Godrick the Grafted", "Godrick the Grafled"));
        assert!(same_boss("Abyss Watchers", "abyss  watchers"));
        assert!(!same_boss("Godskin Apostle", "Godskin Noble"));
        assert!(!same_boss("Ira", "Iron"), "short names must match exactly or by a letter");
        assert!(!same_boss("", "Margit"));
    }

    #[test]
    fn events_save_as_short_json_lines() {
        let e = Event { src: "hotkey".into(), ..Event::new(42, Ev::Death) };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#"{"t":42,"ev":"death","src":"hotkey"}"#);
        assert_eq!(serde_json::from_str::<Event>(&s).unwrap(), e);
        let f: Event = serde_json::from_str(r#"{"t":1,"ev":"fight_start","fight":"f1","boss":"A"}"#).unwrap();
        assert_eq!((f.ev, f.fight.as_str(), f.boss.as_str()), (Ev::FightStart, "f1", "A"));
    }
}
