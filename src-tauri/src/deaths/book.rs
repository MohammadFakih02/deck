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
}

impl Event {
    pub fn new(t: u64, ev: Ev) -> Event {
        Event { t, ev, fight: String::new(), boss: String::new(), src: String::new() }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Fight {
    pub id: String,
    pub boss: String,
    pub started: u64,
    /// when it was won or abandoned
    pub ended: Option<u64>,
    pub won: bool,
    pub deaths: u32,
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
    /// time spent in its fights, ms
    pub time_ms: u64,
    pub fighting: bool,
}

/// A game's history worked out from its events.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub fights: Vec<Fight>,
    /// deaths with no fight open
    pub loose: u32,
    /// each death still counted, in order: Some(fight index) or None (loose); undo takes the last
    counted: Vec<Option<usize>>,
}

/// Bosses are the same whatever the case or spacing.
pub fn boss_key(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
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
                }
                let boss = e.boss.split_whitespace().collect::<Vec<_>>().join(" ");
                self.fights.push(Fight { id: e.fight.clone(), boss, started: e.t, ended: None, won: false, deaths: 0 });
            }
            Ev::Death => {
                let at = self.open_index();
                match at {
                    Some(i) => self.fights[i].deaths += 1,
                    None => self.loose += 1,
                }
                self.counted.push(at);
            }
            Ev::Undo => match self.counted.pop() {
                Some(Some(i)) => self.fights[i].deaths -= 1,
                Some(None) => self.loose -= 1,
                None => {}
            },
            Ev::Win | Ev::Abandon => {
                let won = e.ev == Ev::Win;
                if let Some(f) = self.open_mut() {
                    f.ended = Some(e.t);
                    f.won = won;
                }
            }
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
        self.counted.len() as u32
    }

    /// Something to undo.
    pub fn can_undo(&self) -> bool {
        !self.counted.is_empty()
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
            s.time_ms += f.ended.unwrap_or(now).saturating_sub(f.started);
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
        Event { t, ev, fight: fight.into(), boss: boss.into(), src: String::new() }
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
