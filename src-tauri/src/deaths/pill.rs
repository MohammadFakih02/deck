//! The in-game counter's window: the controller overlay's layered window, click-through,
//! topmost, never focused, on the game's monitor in the chosen corner. What it shows is drawn by
//! `look.rs`; this only puts a picture on screen, fades it in, and hides it.

use super::cfg::DeathSettings;
use crate::pad::raster::Img;
use crate::pad::{place, window::Layered};
use std::time::{Duration, Instant};

/// A new picture fades in over this long.
pub const FADE: Duration = Duration::from_millis(150);

pub struct Pill {
    win: Option<Layered>,
    /// what's drawn now (a key the caller makes from everything that changes the picture)
    drawn: Option<String>,
    /// when the current kind of picture (counter, card, label) came up, for the fade-in
    since: Option<Instant>,
    kind: String,
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

/// The monitor's DPI scale.
pub fn dpi(mon: (i32, i32, i32, i32)) -> f32 {
    place::monitors().into_iter().find(|m| m.rect == mon).map(|m| m.dk()).unwrap_or(1.0)
}

/// How far a fade-in is, 0 – 1.
pub fn faded_in(since: Duration) -> f32 {
    (since.as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
}

impl Pill {
    pub fn new() -> Pill {
        Pill { win: None, drawn: None, since: None, kind: String::new() }
    }

    pub fn hide(&mut self) {
        if let Some(w) = self.win.as_mut() {
            if w.alpha > 0.0 {
                w.set_alpha(0.0);
            }
        }
        self.kind.clear();
    }

    /// Shows the picture `draw` makes (called only when `key` changed) on monitor `mon`. A new
    /// `kind` ("counter", "card", "label") fades in. Returns true while it is still fading.
    pub fn show(&mut self, kind: &str, key: &str, draw: impl FnOnce() -> Img, mon: (i32, i32, i32, i32), s: &DeathSettings) -> bool {
        if self.win.is_none() {
            self.win = Layered::new("Deck death counter");
        }
        let Some(w) = self.win.as_mut() else { return false };
        if self.drawn.as_deref() != Some(key) {
            w.set_image(&draw());
            self.drawn = Some(key.to_string());
        }
        if self.kind != kind {
            self.kind = kind.to_string();
            self.since = Some(Instant::now());
        }
        w.exclude_from_capture(!s.pill_in_recordings);
        let (x, y) = position(&s.pill_corner, mon, w.size, (14.0 * dpi(mon)) as i32);
        w.move_to(x, y);
        w.styles(true);
        let part = self.since.map_or(1.0, |t| faded_in(t.elapsed()));
        let a = s.pill_opacity * part;
        if (w.alpha - a).abs() > 0.001 {
            w.set_alpha(a);
        }
        w.topmost();
        part < 1.0
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

    #[test]
    fn a_new_picture_fades_in() {
        assert_eq!(faded_in(Duration::ZERO), 0.0);
        assert!((faded_in(FADE / 2) - 0.5).abs() < 0.01);
        assert_eq!(faded_in(FADE * 3), 1.0);
    }
}
