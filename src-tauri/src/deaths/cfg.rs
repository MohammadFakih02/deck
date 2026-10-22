//! The death counter's settings: the `deaths` object in settings.json.

use serde::{Deserialize, Serialize};

pub const CORNERS: [&str; 6] = ["top-left", "top", "top-right", "bottom-left", "bottom", "bottom-right"];
/// how the counter looks while you play
pub const STYLES: [&str; 3] = ["skull", "ring", "huge"];
/// what it counts: this boss, the whole game, or both
pub const COUNTS: [&str; 3] = ["boss", "game", "both"];
/// which games the screen is watched in: games Deck knows how to read, or every game
pub const WATCH_IN: [&str; 2] = ["known", "all"];
/// outside a fight: the game's total, or nothing
pub const OUTSIDE: [&str; 2] = ["game", "hide"];

/// The lines of the card shown after a death, each on or off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct CardLines {
    /// "Try N next"
    pub try_no: bool,
    /// how long the try you just lost lasted
    pub try_len: bool,
    /// time spent on this boss
    pub boss_time: bool,
    /// the boss's health left (where Deck saw its bar)
    pub boss_hp: bool,
    /// deaths this session
    pub session: bool,
    /// the last 10 tries as bars
    pub bars: bool,
    /// "Your longest try yet", milestones
    pub note: bool,
    /// your own note for the boss
    pub my_note: bool,
}

impl Default for CardLines {
    fn default() -> Self {
        CardLines { try_no: true, try_len: true, boss_time: true, boss_hp: true, session: true, bars: true, note: true, my_note: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DeathSettings {
    /// counting at all (the tray, the page and the pill)
    pub on: bool,
    /// the in-game pill
    pub pill: bool,
    /// the pill in every game, not only games with deaths or an open fight
    pub pill_always: bool,
    /// one of CORNERS, on the game's monitor
    pub pill_corner: String,
    /// 0.5 – 2.0
    pub pill_scale: f32,
    /// 0.2 – 1.0
    pub pill_opacity: f32,
    /// show the pill in screenshots and recordings (streamers)
    pub pill_in_recordings: bool,
    /// one of STYLES
    pub pill_style: String,
    /// one of COUNTS
    pub pill_counts: String,
    /// one of OUTSIDE
    pub pill_outside: String,
    /// open into the card after a death
    pub card: bool,
    /// how long the card stays, 2 – 10 s
    pub card_secs: u32,
    pub card_lines: CardLines,
    /// the screen's edges glow red for a moment on a death
    pub red_edge: bool,
    /// keys for +1 death and undo ("" = none); RegisterHotKey, off by default
    pub hotkey_death: String,
    pub hotkey_undo: String,
    /// hold Select + D-pad Down (+1) / Up (undo) for a second on the controller
    pub pad_combo: bool,
    /// read deaths from save files where Deck knows the game's save
    pub auto_save: bool,
    /// watch the screen for death screens
    pub auto_screen: bool,
    /// one of WATCH_IN: "known" = games in the catalogue, with taught or shared death screens, or
    /// with deaths counted; "all" = any game in front
    pub watch_in: String,
    /// while watching the screen, open and time boss fights from the boss's health bar
    pub auto_boss: bool,
    /// names the screen found that aren't bosses ("Not a boss"): "game key\tname"
    pub not_bosses: Vec<String>,
    /// where a game's boss bar is, shown by picking it (game key -> spot)
    pub bar_spots: std::collections::BTreeMap<String, super::screen::bar::Spot>,
    /// where a game shows the boss's name, shown by drawing a box (game key -> spot)
    pub name_spots: std::collections::BTreeMap<String, super::screen::bar::Spot>,
    /// use the shared death-screen library (the repo's, refreshed from GitHub at most once a day)
    pub community_library: bool,
    /// games turned off ("Don't count this game"): game keys
    pub off_games: Vec<String>,
    /// games that don't use the shared library's death screens (one was wrong): game keys
    pub library_off: Vec<String>,
}

impl Default for DeathSettings {
    fn default() -> Self {
        DeathSettings {
            on: true,
            pill: true,
            pill_always: false,
            pill_corner: "top".into(),
            pill_scale: 1.0,
            pill_opacity: 0.9,
            pill_in_recordings: true,
            pill_style: "skull".into(),
            pill_counts: "both".into(),
            pill_outside: "game".into(),
            card: true,
            card_secs: 5,
            card_lines: CardLines::default(),
            red_edge: false,
            hotkey_death: String::new(),
            hotkey_undo: String::new(),
            pad_combo: false,
            auto_save: true,
            auto_screen: true,
            watch_in: "known".into(),
            auto_boss: true,
            not_bosses: Vec::new(),
            bar_spots: Default::default(),
            name_spots: Default::default(),
            community_library: true,
            off_games: Vec::new(),
            library_off: Vec::new(),
        }
    }
}

impl DeathSettings {
    /// Values out of range back to something usable.
    pub fn clamped(mut self) -> Self {
        if !CORNERS.contains(&self.pill_corner.as_str()) {
            self.pill_corner = "top".into();
        }
        self.pill_scale = if self.pill_scale.is_finite() { self.pill_scale.clamp(0.5, 2.0) } else { 1.0 };
        self.pill_opacity = if self.pill_opacity.is_finite() { self.pill_opacity.clamp(0.2, 1.0) } else { 0.9 };
        if !WATCH_IN.contains(&self.watch_in.as_str()) {
            self.watch_in = "known".into();
        }
        if !STYLES.contains(&self.pill_style.as_str()) {
            self.pill_style = "skull".into();
        }
        if !COUNTS.contains(&self.pill_counts.as_str()) {
            self.pill_counts = "both".into();
        }
        if !OUTSIDE.contains(&self.pill_outside.as_str()) {
            self.pill_outside = "game".into();
        }
        self.card_secs = self.card_secs.clamp(2, 10);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_counter_settings_default_and_clamp() {
        let s: DeathSettings = serde_json::from_str(r#"{"pill":true}"#).unwrap();
        assert_eq!((s.pill_style.as_str(), s.pill_counts.as_str(), s.pill_outside.as_str(), s.card, s.card_secs, s.red_edge), ("skull", "both", "game", true, 5, false));
        assert!(s.card_lines.bars && !s.card_lines.my_note);
        assert!(s.pill_in_recordings, "hiding from capture makes NVIDIA turn Instant Replay off");
        let s: DeathSettings = serde_json::from_str(r#"{"pill_style":"neon","pill_counts":"x","pill_outside":"y","card_secs":60,"card_lines":{"bars":false}}"#).unwrap();
        let s = s.clamped();
        assert_eq!((s.pill_style.as_str(), s.pill_counts.as_str(), s.pill_outside.as_str(), s.card_secs), ("skull", "both", "game", 10));
        assert!(!s.card_lines.bars && s.card_lines.try_no, "a missing line keeps its default");
    }

    #[test]
    fn missing_or_bad_values_fall_back() {
        let s: DeathSettings = serde_json::from_str(r#"{"pill_corner":"middle","pill_scale":9}"#).unwrap();
        let s = s.clamped();
        assert_eq!((s.pill_corner.as_str(), s.pill_scale, s.on), ("top", 2.0, true));
    }
}
