//! The in-game pill ("Malenia · 14"): the controller overlay's layered window and pill art,
//! click-through, topmost, never focused, on the game's monitor in the chosen corner.

use super::cfg::DeathSettings;
use crate::pad::{art, place, window::Layered};

pub struct Pill {
    win: Option<Layered>,
    /// what's drawn now: (text, accent, scale, theme)
    drawn: Option<(String, bool, u32, String)>,
}

/// Where a pill of `size` goes on a monitor `(l, t, r, b)` in `corner`, `margin` px in.
pub fn position(corner: &str, mon: (i32, i32, i32, i32), size: (i32, i32), margin: i32) -> (i32, i32) {
    let (l, t, r, b) = mon;
    let x = match corner {
        "top-left" | "bottom-left" => l + margin,
        "top-right" | "bottom-right" => r - margin - size.0,
        _ => l + (r - l - size.0) / 2,
    };
    let y = if corner.starts_with("bottom") { b - margin - size.1 } else { t + margin };
    (x, y)
}

impl Pill {
    pub fn new() -> Pill {
        Pill { win: None, drawn: None }
    }

    pub fn hide(&mut self) {
        if let Some(w) = self.win.as_mut() {
            if w.alpha > 0.0 {
                w.set_alpha(0.0);
            }
        }
    }

    /// Shows `text` on the monitor `mon` (accent = a death or undo just happened).
    pub fn show(&mut self, text: &str, accent: bool, mon: (i32, i32, i32, i32), s: &DeathSettings, theme: &str) {
        if self.win.is_none() {
            self.win = Layered::new("Deck death counter");
        }
        let Some(w) = self.win.as_mut() else { return };
        let dpi = place::monitors().into_iter().find(|m| m.rect == mon).map(|m| m.dk()).unwrap_or(1.0);
        let k = s.pill_scale * dpi;
        let key = (text.to_string(), accent, (k * 100.0) as u32, theme.to_string());
        if self.drawn.as_ref() != Some(&key) {
            let t = art::theme(theme);
            let img = art::pill(&[(text, 15.0, "semibold", t.text)], t, k, accent.then_some(t.accent));
            w.set_image(&img);
            self.drawn = Some(key);
        }
        w.exclude_from_capture(!s.pill_in_recordings);
        let (x, y) = position(&s.pill_corner, mon, w.size, (14.0 * dpi) as i32);
        w.move_to(x, y);
        w.styles(true);
        if (w.alpha - s.pill_opacity).abs() > 0.001 {
            w.set_alpha(s.pill_opacity);
        }
        w.topmost();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_sits_in_its_corner_of_the_games_monitor() {
        let mon = (1920, 0, 3840, 1080);
        assert_eq!(position("top", mon, (200, 40), 14), (1920 + 860, 14));
        assert_eq!(position("top-left", mon, (200, 40), 14), (1934, 14));
        assert_eq!(position("bottom-right", mon, (200, 40), 14), (3840 - 214, 1080 - 54));
    }
}
