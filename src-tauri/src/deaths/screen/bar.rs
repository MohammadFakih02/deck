//! A boss's health bar on a frame, found from its shape alone so it works in any game: a wide,
//! thin strip near the top or the bottom of the picture, about centred, with long straight edges
//! above and below and a smooth inside (a line of text has edges all along it). Once found, it
//! is followed where it is: still there or not, and how full.
//!
//! The player's own bars (a corner: Bloodborne's top left) are never centred, so they're left
//! alone. A game whose boss bar is on the side (Sekiro's, top left) has a `Spot`: from the
//! catalogue, or taught by picking the bar on a picture (`candidates` lists every bar-like strip).

use super::capture::Frame;
use serde::{Deserialize, Serialize};

/// A bar: columns `x0..x1`, rows `y0..y1` of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub x0: usize,
    pub x1: usize,
    pub y0: usize,
    pub y1: usize,
}

impl Bar {
    pub fn mid(&self) -> usize {
        (self.y0 + self.y1) / 2
    }

    /// The same bar in another frame of the same width (a pixel or two off).
    pub fn same_place(&self, o: &Bar, w: usize) -> bool {
        let tol = w / 100 + 2;
        self.x0.abs_diff(o.x0) <= tol && self.x1.abs_diff(o.x1) <= tol && self.y0.abs_diff(o.y0) <= 3 && self.y1.abs_diff(o.y1) <= 3
    }
}

/// Where a game's boss bar is, in thousandths of the frame (the same at any resolution).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spot {
    pub x0: u16,
    pub x1: u16,
    pub y0: u16,
    pub y1: u16,
}

impl Spot {
    pub fn of(b: &Bar, w: usize, h: usize) -> Spot {
        let k = |v: usize, of: usize| (v * 1000 / of.max(1)).min(1000) as u16;
        Spot { x0: k(b.x0, w), x1: k(b.x1, w), y0: k(b.y0, h), y1: k(b.y1, h) }
    }

    /// Something a bar could be: in the frame, wider than tall.
    pub fn sane(&self) -> bool {
        self.x0 < self.x1 && self.y0 < self.y1 && self.x1 <= 1000 && self.y1 <= 1000 && self.x1 - self.x0 >= 50
    }
}

/// a change this big (0–255, the most of the three colours) between two rows is an edge
const EDGE: u8 = 28;
/// columns looked at: every other one
const STEP: usize = 2;
/// as wide as this share of the frame…
const MIN_W: f32 = 0.30;
/// …but not edge to edge (a letterbox, a menu's line)
const MAX_W: f32 = 0.88;
/// its middle this close to the frame's middle
const OFF_CENTRE: f32 = 0.12;
/// a colour this close (the most of the three) is the bar's fill
const SAME_FILL: u8 = 45;

fn at(f: &Frame, x: usize, y: usize) -> [u8; 3] {
    let i = (y * f.w + x) * 4;
    [f.px[i], f.px[i + 1], f.px[i + 2]]
}

fn diff(a: [u8; 3], b: [u8; 3]) -> u8 {
    a[0].abs_diff(b[0]).max(a[1].abs_diff(b[1])).max(a[2].abs_diff(b[2]))
}

/// Where row `y` differs from the row above it, every STEP columns from `x0` to `x1`.
fn edges(f: &Frame, y: usize, x0: usize, x1: usize) -> Vec<bool> {
    (x0..x1).step_by(STEP).map(|x| diff(at(f, x, y), at(f, x, y - 1)) >= EDGE).collect()
}

/// The stretches of `e` that are solidly an edge, as (from, to) in samples: each edge counts 1,
/// each gap 3, so a few gaps (a marker, a dark spot of the scene) don't break one but a busy
/// picture's scattered edges never join up. Each stretch is the best part of a run that stays
/// above zero.
fn stretches(e: &[bool]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let (mut start, mut sum, mut best, mut best_end) = (0usize, 0i32, 0i32, 0usize);
    for (i, on) in e.iter().enumerate() {
        if sum <= 0 {
            if best > 0 {
                out.push((start, best_end));
            }
            (start, sum, best) = (i, 0, 0);
        }
        sum += if *on { 1 } else { -3 };
        if sum > best {
            (best, best_end) = (sum, i + 1);
        }
    }
    if best > 0 {
        out.push((start, best_end));
    }
    out
}

/// The inside of a bar is smooth along its middle row (a fill and an empty part, a damage mark or
/// two), and each column is about one colour from top to bottom (not two bars stacked).
fn smooth_inside(f: &Frame, b: &Bar) -> bool {
    let y = b.mid();
    let xs: Vec<usize> = (b.x0..b.x1).step_by(STEP).collect();
    let jumps = xs.windows(2).filter(|p| diff(at(f, p[0], y), at(f, p[1], y)) >= 40).count();
    if jumps > (xs.len() / 25).max(4) {
        return false;
    }
    let (r0, r1) = (b.y0 + 1, b.y1.saturating_sub(1).max(b.y0 + 2));
    let spread: u32 = xs.iter().step_by(4).map(|&x| {
        let (mut lo, mut hi) = ([255u8; 3], [0u8; 3]);
        for y in r0..r1 {
            let p = at(f, x, y);
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
        (0..3).map(|c| hi[c].saturating_sub(lo[c])).max().unwrap_or(0) as u32
    }).sum::<u32>() / (xs.len() as u32 / 4).max(1);
    spread <= 70
}

/// Bars in `rows` (looked at in that order) with edges between columns `x0..x1` that `fits`
/// takes; the first one, or with `all`, every one in a different place.
fn scan(f: &Frame, rows: &[usize], x0: usize, x1: usize, fits: &dyn Fn(usize, usize) -> bool, all: bool) -> Vec<Bar> {
    let (w, h) = (f.w, f.h);
    let max_h = (h * 35 / 1000).max(6);
    let tol = w * 25 / 1000 + 2;
    let ok = |y: &usize| *y >= 1 && *y < h;
    // each row's long edges: (y, x0, x1)
    let lines: Vec<(usize, usize, usize)> = rows
        .iter()
        .filter(|y| ok(y))
        .flat_map(|&y| stretches(&edges(f, y, x0, x1)).into_iter().map(move |(a, b)| (y, x0 + a * STEP, (x0 + b * STEP).min(w))))
        .filter(|l| fits(l.1, l.2))
        .collect();
    let mut out: Vec<Bar> = vec![];
    for &(y, a0, a1) in &lines {
        // the farthest edge below that lines up and makes a smooth bar
        for y2 in (y + 3..=(y + max_h).min(h - 1)).rev() {
            let Some(&(_, b0, b1)) = lines.iter().find(|l| l.0 == y2 && a0.abs_diff(l.1) <= tol && a1.abs_diff(l.2) <= tol) else { continue };
            let bar = Bar { x0: a0.max(b0), x1: a1.min(b1), y0: y, y1: y2 };
            if fits(bar.x0, bar.x1) && smooth_inside(f, &bar) {
                if !all {
                    return vec![bar];
                }
                if !out.iter().any(|o| o.x0 < bar.x1 && bar.x0 < o.x1 && o.y0 <= bar.y1 + 2 && bar.y0 <= o.y1 + 2) {
                    out.push(bar);
                }
                break;
            }
        }
    }
    out
}

fn big_enough(f: &Frame) -> bool {
    f.w >= 200 && f.h >= 100 && f.px.len() >= f.w * f.h * 4
}

/// A boss bar on this frame, the one nearest the top or the bottom edge.
pub fn find(f: &Frame) -> Option<Bar> {
    if !big_enough(f) {
        return None;
    }
    let (w, h) = (f.w, f.h);
    let fits = |x0: usize, x1: usize| {
        let (bw, c) = ((x1 - x0) as f32, (x0 + x1) as f32 / 2.0);
        bw >= w as f32 * MIN_W && bw <= w as f32 * MAX_W && (c - w as f32 / 2.0).abs() <= w as f32 * OFF_CENTRE
    };
    // the top band from the top down, the bottom band from the bottom up
    let top: Vec<usize> = ((h * 2 / 100).max(1)..h * 30 / 100).collect();
    let bottom: Vec<usize> = (h * 62 / 100..h * 97 / 100).rev().collect();
    [top, bottom].iter().find_map(|rows| scan(f, rows, 0, w, &fits, false).pop())
}

/// The boss bar at a game's `spot` (a little off is fine: another resolution, a scaled HUD).
pub fn find_at(f: &Frame, spot: &Spot) -> Option<Bar> {
    if !big_enough(f) || !spot.sane() {
        return None;
    }
    let (w, h) = (f.w, f.h);
    let px = |v: u16, of: usize| v as usize * of / 1000;
    let (sx0, sx1) = (px(spot.x0, w), px(spot.x1, w));
    let (slack_x, slack_y) = (w * 4 / 100, h * 2 / 100);
    let rows: Vec<usize> = (px(spot.y0, h).saturating_sub(slack_y)..(px(spot.y1, h) + slack_y).min(h)).collect();
    let fits = |x0: usize, x1: usize| x0.abs_diff(sx0) <= slack_x && x1.abs_diff(sx1) <= slack_x;
    scan(f, &rows, sx0.saturating_sub(slack_x), (sx1 + slack_x).min(w), &fits, false).pop()
}

/// Every bar-like strip on the frame, anywhere near the top or the bottom and in any place
/// across: what you pick from when you show Deck where a game's boss bar is.
pub fn candidates(f: &Frame) -> Vec<Bar> {
    if !big_enough(f) {
        return vec![];
    }
    let (w, h) = (f.w, f.h);
    let fits = |x0: usize, x1: usize| {
        let bw = (x1 - x0) as f32;
        bw >= w as f32 * 0.10 && bw <= w as f32 * MAX_W
    };
    let rows: Vec<usize> = (1..h * 40 / 100).chain(h * 55 / 100..h - 1).collect();
    let mut out = scan(f, &rows, 0, w, &fits, true);
    out.truncate(6);
    out
}

/// The bar's fill colour: its left end's (bars empty from the right).
pub fn fill_colour(f: &Frame, b: &Bar) -> [u8; 3] {
    let y = b.mid();
    let end = b.x0 + ((b.x1 - b.x0) / 20).max(2);
    let mut px: Vec<[u8; 3]> = (b.x0 + 1..end.min(f.w)).map(|x| at(f, x, y)).collect();
    if px.is_empty() {
        return [0; 3];
    }
    let mut out = [0u8; 3];
    for (c, o) in out.iter_mut().enumerate() {
        px.sort_by_key(|p| p[c]);
        *o = px[px.len() / 2][c];
    }
    out
}

/// How full the bar is, 0–100: the share of its middle row in the fill colour.
pub fn fill_pct(f: &Frame, b: &Bar, fill: [u8; 3]) -> u8 {
    let y = b.mid();
    let xs: Vec<usize> = (b.x0..b.x1.min(f.w)).step_by(STEP).collect();
    if xs.is_empty() || y >= f.h {
        return 0;
    }
    let n = xs.iter().filter(|&&x| diff(at(f, x, y), fill) <= SAME_FILL).count();
    ((n * 100 + xs.len() / 2) / xs.len()).min(100) as u8
}

/// The bar found before is still on screen: its edges, or (a nearly empty bar on a dark scene,
/// where the edges fade) a strip of its fill colour at its left end that the picture above lacks.
pub fn still_there(f: &Frame, b: &Bar, fill: [u8; 3]) -> bool {
    if f.w == 0 || b.x1 > f.w || b.y1 + 2 >= f.h || b.y0 < 3 {
        return false;
    }
    let cover = |y: usize| {
        let e = edges(f, y, b.x0, b.x1);
        e.iter().filter(|x| **x).count() as f32 / e.len().max(1) as f32
    };
    let rows = [b.y0 - 1, b.y0, b.y0 + 1, b.y1 - 1, b.y1, b.y1 + 1];
    if rows.iter().any(|&y| cover(y) >= 0.4) {
        return true;
    }
    let (y, above) = (b.mid(), b.y0 - 3);
    let need = ((b.x1 - b.x0) * 15 / 1000 / STEP).max(2);
    let mut run = 0;
    for x in (b.x0..b.x1).step_by(STEP) {
        if diff(at(f, x, y), fill) <= SAME_FILL && diff(at(f, x, above), fill) > SAME_FILL {
            run += 1;
            if run >= need {
                return true;
            }
        } else {
            break;
        }
    }
    false
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const W: usize = 1280;
    pub const H: usize = 720;

    /// A busy game picture (BGRA): stripes and blotches of colour that change every few pixels,
    /// with long horizontal edges nowhere in the bands.
    pub fn scene() -> Frame {
        let mut px = vec![0u8; W * H * 4];
        for y in 0..H {
            for x in 0..W {
                let v = ((x * 7 + y * 13) % 97) as u8;
                let i = (y * W + x) * 4;
                px[i..i + 4].copy_from_slice(&[40 + v, 60 + v / 2, 30 + ((x / 9 + y / 7) % 50) as u8, 255]);
            }
        }
        Frame { w: W, h: H, px }
    }

    pub fn rect(f: &mut Frame, x0: usize, x1: usize, y0: usize, y1: usize, bgr: [u8; 3]) {
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * f.w + x) * 4;
                f.px[i..i + 4].copy_from_slice(&[bgr[0], bgr[1], bgr[2], 255]);
            }
        }
    }

    /// A boss bar like Elden Ring's (bottom, centred): a dark frame, red fill, dark grey empty part.
    pub fn boss_bar(f: &mut Frame, y0: usize, pct: usize) {
        let (x0, x1) = (290, 990);
        rect(f, x0, x1, y0, y0 + 12, [14, 14, 16]);
        let full = x0 + 2 + (x1 - x0 - 4) * pct / 100;
        rect(f, x0 + 2, full, y0 + 2, y0 + 10, [30, 30, 170]);
        rect(f, full, x1 - 2, y0 + 2, y0 + 10, [40, 40, 44]);
    }

    #[test]
    fn a_centred_boss_bar_is_found_at_the_bottom_or_the_top() {
        let mut f = scene();
        assert_eq!(find(&f), None, "no bar in plain play");
        boss_bar(&mut f, 620, 100);
        let b = find(&f).expect("found");
        assert!(b.x0.abs_diff(290) <= 4 && b.x1.abs_diff(990) <= 4, "{b:?}");
        assert!(b.y0 >= 620 && b.y1 <= 632 && b.y1 - b.y0 >= 6, "{b:?}");
        let mut f = scene();
        boss_bar(&mut f, 60, 100);
        assert!(find(&f).is_some_and(|b| b.y0 >= 60 && b.y1 <= 72), "a bar at the top (Khazan, Hades)");
    }

    #[test]
    fn the_players_bars_letterbox_and_text_are_not_boss_bars() {
        let mut f = scene();
        // the player's health and stamina, top left
        rect(&mut f, 40, 520, 40, 50, [30, 30, 170]);
        rect(&mut f, 40, 400, 56, 62, [30, 150, 40]);
        // a cinematic's black bars, edge to edge
        rect(&mut f, 0, W, 0, 8, [0, 0, 0]);
        rect(&mut f, 0, W, 690, 720, [0, 0, 0]);
        // a line of subtitles: light letters on a dark strip
        rect(&mut f, 340, 940, 600, 616, [10, 10, 10]);
        for x in (344..936).step_by(9) {
            rect(&mut f, x, x + 4, 603, 613, [230, 230, 230]);
        }
        assert_eq!(find(&f), None);
    }

    /// Sekiro's boss bar is top left: not found by itself, found at its spot, and offered
    /// (with the player's bars) to pick from.
    #[test]
    fn a_bar_on_the_side_is_found_at_its_spot() {
        let mut f = scene();
        rect(&mut f, 40, 420, 50, 62, [14, 14, 16]);
        rect(&mut f, 42, 418, 52, 60, [30, 30, 170]);
        assert_eq!(find(&f), None, "not centred: not looked for without a spot");
        let all = candidates(&f);
        assert_eq!(all.len(), 1, "{all:?}");
        let spot = Spot::of(&all[0], W, H);
        assert!(spot.sane());
        let at = find_at(&f, &spot).expect("found at its spot");
        assert!(at.x0.abs_diff(42) <= 4 && at.x1.abs_diff(418) <= 4, "{at:?}");
        assert_eq!(find_at(&scene(), &spot), None, "nothing there");
    }

    #[test]
    fn the_players_bar_and_the_bosss_are_both_offered() {
        // Bloodborne: the player's health top left, the boss's bottom middle
        let mut f = scene();
        rect(&mut f, 40, 360, 40, 50, [30, 30, 170]);
        boss_bar(&mut f, 620, 100);
        let all = candidates(&f);
        assert_eq!(all.len(), 2, "{all:?}");
        assert!(find(&f).is_some_and(|b| b.y0 >= 620), "the boss's, not the player's");
    }

    #[test]
    fn its_fill_is_measured_and_it_is_followed_until_it_goes() {
        let mut f = scene();
        boss_bar(&mut f, 620, 100);
        let b = find(&f).unwrap();
        let c = fill_colour(&f, &b);
        assert!(diff(c, [30, 30, 170]) <= 10, "red: {c:?}");
        assert!(fill_pct(&f, &b, c) >= 95);
        let mut f = scene();
        boss_bar(&mut f, 620, 23);
        let p = fill_pct(&f, &b, c);
        assert!((20..=26).contains(&p), "{p}");
        assert!(still_there(&f, &b, c));
        assert!(!still_there(&scene(), &b, c), "gone");
        // nearly empty on a black screen: the frame's edges are gone, the red sliver isn't
        let mut f = Frame { w: W, h: H, px: [12u8, 12, 14, 255].repeat(W * H) };
        rect(&mut f, 292, 312, 622, 630, [30, 30, 170]);
        assert!(still_there(&f, &b, c));
        assert!(!still_there(&Frame { w: W, h: H, px: [12u8, 12, 14, 255].repeat(W * H) }, &b, c));
    }
}
