//! The red edge: on a death, the edges of the game's monitor glow red for a moment and fade.
//! Four click-through layered windows (top, bottom, left, right), each a red glow fading inward,
//! drawn once per monitor size and only shown while the glow lasts (any window over a fullscreen
//! game stops it flipping straight to the screen, so they don't stay up).

use crate::pad::raster::{self, Img};
use crate::pad::window::Layered;
use std::time::{Duration, Instant};

pub const GLOW: Duration = Duration::from_millis(800);
const RED: &str = "#c81414";

/// How strong the glow is `since` the death: 1 at once, fading to 0 at GLOW.
pub fn alpha_at(since: Duration) -> f32 {
    let p = since.as_secs_f32() / GLOW.as_secs_f32();
    if p >= 1.0 {
        0.0
    } else {
        // eased: holds a moment, then fades
        1.0 - p * p
    }
}

/// A strip `w` x `h` glowing from its `side` ("top", "bottom", "left", "right") inward.
fn strip(w: i32, h: i32, side: &str) -> Img {
    let (x2, y2, x1, y1) = match side {
        "top" => (0, 1, 0, 0),
        "bottom" => (0, 0, 0, 1),
        "left" => (1, 0, 0, 0),
        _ => (0, 0, 1, 0),
    };
    let s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><defs><linearGradient id="g" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"><stop offset="0" stop-color="{RED}" stop-opacity="0.8"/><stop offset="1" stop-color="{RED}" stop-opacity="0"/></linearGradient></defs><rect width="{w}" height="{h}" fill="url(#g)"/></svg>"#
    );
    raster::doc(&s).unwrap_or_else(|| Img::new(1, 1))
}

pub struct Edge {
    wins: Vec<Layered>,
    drawn_for: Option<(i32, i32, i32, i32)>,
    since: Option<Instant>,
}

impl Edge {
    pub fn new() -> Edge {
        Edge { wins: vec![], drawn_for: None, since: None }
    }

    /// Starts the glow on monitor `mon` (l, t, r, b).
    pub fn flash(&mut self, mon: (i32, i32, i32, i32), in_recordings: bool) {
        let (l, t, r, b) = mon;
        let (w, h) = (r - l, b - t);
        let d = ((w.min(h) as f32) * 0.09).round().max(8.0) as i32;
        if self.drawn_for != Some(mon) {
            self.wins.clear();
            for (side, x, y, sw, sh) in [("top", l, t, w, d), ("bottom", l, b - d, w, d), ("left", l, t, d, h), ("right", r - d, t, d, h)] {
                let Some(mut win) = Layered::new("Deck death edge") else { continue };
                win.set_image(&strip(sw, sh, side));
                win.move_to(x, y);
                win.styles(true);
                self.wins.push(win);
            }
            self.drawn_for = Some(mon);
        }
        for w in &self.wins {
            w.exclude_from_capture(!in_recordings);
        }
        self.since = Some(Instant::now());
        self.tick();
    }

    /// Moves the glow on; true while it lasts.
    pub fn tick(&mut self) -> bool {
        let Some(since) = self.since else { return false };
        let a = alpha_at(since.elapsed());
        for w in &mut self.wins {
            w.set_alpha(a);
            if a > 0.0 {
                w.topmost();
            }
        }
        if a <= 0.0 {
            self.since = None;
        }
        a > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edge_fades_out_in_about_800ms() {
        assert_eq!(alpha_at(Duration::ZERO), 1.0);
        assert_eq!(alpha_at(GLOW), 0.0);
        assert_eq!(alpha_at(GLOW * 2), 0.0);
        let steps: Vec<f32> = (0..=8).map(|i| alpha_at(Duration::from_millis(i * 100))).collect();
        assert!(steps.windows(2).all(|w| w[1] <= w[0]), "{steps:?}");
    }

    #[test]
    fn a_strip_is_its_size_and_red() {
        let s = strip(300, 40, "top");
        assert_eq!((s.w(), s.h()), (300, 40));
    }
}
