//! The death counter's settings: the `deaths` object in settings.json.

use serde::{Deserialize, Serialize};

pub const CORNERS: [&str; 6] = ["top-left", "top", "top-right", "bottom-left", "bottom", "bottom-right"];

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
    /// keys for +1 death and undo ("" = none); RegisterHotKey, off by default
    pub hotkey_death: String,
    pub hotkey_undo: String,
    /// hold Select + D-pad Down (+1) / Up (undo) for a second on the controller
    pub pad_combo: bool,
    /// read deaths from save files where Deck knows the game's save
    pub auto_save: bool,
    /// watch the screen for death screens
    pub auto_screen: bool,
    /// use the shared death-screen library (the repo's, refreshed from GitHub at most once a day)
    pub community_library: bool,
    /// games turned off ("Don't count this game"): game keys
    pub off_games: Vec<String>,
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
            pill_in_recordings: false,
            hotkey_death: String::new(),
            hotkey_undo: String::new(),
            pad_combo: false,
            auto_save: true,
            auto_screen: true,
            community_library: true,
            off_games: Vec::new(),
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
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_bad_values_fall_back() {
        let s: DeathSettings = serde_json::from_str(r#"{"pill_corner":"middle","pill_scale":9}"#).unwrap();
        let s = s.clamped();
        assert_eq!((s.pill_corner.as_str(), s.pill_scale, s.on), ("top", 2.0, true));
    }
}
