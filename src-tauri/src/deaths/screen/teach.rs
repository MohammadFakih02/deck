//! Taught death screens: for games whose death screen has no words Windows can read (Sekiro's
//! 死, a skull, a red flash), you show Deck the death screen once. Deck keeps the last 20 s of
//! the game's frames in memory; after a death you counted yourself, you pick the frame where the
//! death screen is clearest, and the part that differs from the game around it becomes a
//! fingerprint, matched on every frame from then on (no OCR needed). Up to 3 per game.
//!
//! A fingerprint is cells of the 64x36 grey thumbnail: which cells (the mask) and their values.
//! A frame matches when those cells have the same shape (correlation) and about the same
//! brightness. Shareable as a short code ("Share" / "Add a shared one").

use super::detect::{TH, TW};
use serde::{Deserialize, Serialize};

pub const MAX_PER_GAME: usize = 3;
/// a cell counts as part of the death screen when it differs this much from normal play
const CHANGED: u8 = 24;
const MIN_CELLS: usize = 24;
/// the fingerprint needs some shape: a plain darkening of the screen isn't one
const MIN_SPREAD: f32 = 8.0;
const MIN_CORR: f32 = 0.85;
const MAX_DIFF: f32 = 30.0;
/// frames this different from the picked one are "normal play" for the comparison
const OTHER: u32 = 12;
/// more than this share of the screen changed (in tenths): the whole screen faded or flashed, and the
/// death screen is what stands out within the picked picture (words, a symbol)
const FADED_TENTHS: usize = 4;
/// a cell stands out when it differs this much from the cells around it
const STANDS_OUT: u8 = 9;
/// dim words on a faded screen are a thin line in the thumbnail; the ring around them makes the print
const MIN_WORD_CELLS: usize = 8;
/// cells in a row that make a line of words
const LINE: usize = 5;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Print {
    /// 64x36 bits, row by row, as hex
    pub mask: String,
    /// the masked cells' grey values in order, as hex
    pub values: String,
    /// when it was taught (ms)
    pub made: u64,
    /// the shape of the picture it was taught on, width / height x 1000 (0: unknown, fits any)
    #[serde(default)]
    pub aspect: u32,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// A print decoded for matching.
pub struct Taught {
    cells: Vec<usize>,
    values: Vec<f32>,
    mean: f32,
    norm: f32,
    aspect: u32,
}

impl Taught {
    pub fn new(p: &Print) -> Option<Taught> {
        let bits = unhex(&p.mask)?;
        let values: Vec<f32> = unhex(&p.values)?.into_iter().map(f32::from).collect();
        if bits.len() != TW * TH / 8 {
            return None;
        }
        let cells: Vec<usize> = (0..TW * TH).filter(|i| bits[i / 8] & (1 << (i % 8)) != 0).collect();
        if cells.len() != values.len() || cells.len() < MIN_CELLS {
            return None;
        }
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let norm = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>().sqrt();
        Some(Taught { cells, values, mean, norm, aspect: p.aspect })
    }

    /// Taught on a picture of this shape (within 3 %): a 16:9 death screen lands on other cells of
    /// a 21:9 picture, so it is only matched where it can be right.
    pub fn fits(&self, aspect: u32) -> bool {
        self.aspect == 0 || aspect == 0 || self.aspect.abs_diff(aspect) * 100 <= self.aspect * 3
    }

    /// The thumbnail shows this death screen.
    pub fn matches(&self, thumb: &[u8]) -> bool {
        if thumb.len() != TW * TH || self.norm < 1.0 {
            return false;
        }
        let got: Vec<f32> = self.cells.iter().map(|i| thumb[*i] as f32).collect();
        let n = got.len() as f32;
        let mean = got.iter().sum::<f32>() / n;
        let diff = got.iter().zip(&self.values).map(|(a, b)| (a - b).abs()).sum::<f32>() / n;
        if diff > MAX_DIFF {
            return false;
        }
        let norm = got.iter().map(|v| (v - mean).powi(2)).sum::<f32>().sqrt();
        if norm < 1.0 {
            return false;
        }
        let dot: f32 = got.iter().zip(&self.values).map(|(a, b)| (a - mean) * (b - self.mean)).sum();
        dot / (norm * self.norm) >= MIN_CORR
    }
}

fn mean_diff(a: &[u8], b: &[u8]) -> u32 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u32).sum::<u32>() / a.len().max(1) as u32
}

/// Another picture than the picked one: different on average, or very different in enough places
/// (a dark game's play and its death screen have nearly the same average; the banner is the change).
fn other_picture(t: &[u8], chosen: &[u8]) -> bool {
    mean_diff(t, chosen) >= OTHER || t.iter().zip(chosen).filter(|(x, y)| x.abs_diff(**y) >= CHANGED).count() >= MIN_CELLS
}

/// Learns the death screen in `strip[pick]` (thumbnails, oldest first) against the frames that
/// look different from it (normal play). The error is for the page, in plain words.
pub fn learn(strip: &[Vec<u8>], pick: usize, made: u64) -> Result<Print, String> {
    let chosen = strip.get(pick).filter(|t| t.len() == TW * TH).ok_or("That picture is gone; count the next death and try again.")?;
    let others: Vec<&Vec<u8>> = strip.iter().filter(|t| t.len() == TW * TH && other_picture(t, chosen)).collect();
    if others.len() < 3 {
        return Err("Deck needs a few seconds of the game before the death screen. Next time, count the death while the death screen is still up or just after.".into());
    }
    // normal play, cell by cell: the median of the other frames
    let base: Vec<u8> = (0..TW * TH)
        .map(|i| {
            let mut v: Vec<u8> = others.iter().map(|t| t[i]).collect();
            v.sort_unstable();
            v[v.len() / 2]
        })
        .collect();
    let mut cells: Vec<usize> = (0..TW * TH).filter(|i| chosen[*i].abs_diff(base[*i]) >= CHANGED).collect();
    let faded = cells.len() * 10 > TW * TH * FADED_TENTHS;
    if faded {
        // the game darkened (or flashed) everywhere: a print of all of it would be "a dark screen",
        // which any loading screen is too. What makes it the death screen is what stands out in it.
        cells = in_a_line(&stands_out(chosen));
        if cells.len() < MIN_WORD_CELLS {
            return Err("That picture is mostly one flat colour, which could be any dark or loading screen. Pick one where the death screen's words or symbol show.".into());
        }
    } else if cells.len() < MIN_CELLS {
        return Err("That picture looks too much like the game around it. Pick the one where the death screen is clearest.".into());
    }
    // the cells around the change too: dim words on a dark screen are all about the same grey, and
    // their edges against the screen are what tells them apart
    let mut mask = vec![false; TW * TH];
    for &i in &cells {
        let (x, y) = (i % TW, i / TW);
        mask[i] = true;
        if x > 0 {
            mask[i - 1] = true;
        }
        if x + 1 < TW {
            mask[i + 1] = true;
        }
        if y > 0 {
            mask[i - TW] = true;
        }
        if y + 1 < TH {
            mask[i + TW] = true;
        }
    }
    let cells: Vec<usize> = (0..TW * TH).filter(|i| mask[*i]).collect();
    let values: Vec<u8> = cells.iter().map(|i| chosen[*i]).collect();
    let mean = values.iter().map(|v| *v as f32).sum::<f32>() / values.len() as f32;
    let spread = values.iter().map(|v| (*v as f32 - mean).abs()).sum::<f32>() / values.len() as f32;
    // (on a faded screen the cells that stand out are the shape; the dark around them is most of it)
    if spread < MIN_SPREAD && !faded {
        return Err("That picture is mostly one flat colour, which could be any dark or loading screen. Pick one where the death screen's words or symbol show.".into());
    }
    let mut bits = vec![0u8; TW * TH / 8];
    for i in &cells {
        bits[i / 8] |= 1 << (i % 8);
    }
    let print = Print { mask: hex(&bits), values: hex(&values), made, aspect: 0 };
    let t = Taught::new(&print).ok_or("Couldn't learn that picture.")?;
    // the death screen shows in the pictures around the picked one (it fades in and stays a while);
    // anywhere else in the strip it would be a death that didn't happen
    let hits: Vec<bool> = strip.iter().map(|s| s.len() == TW * TH && t.matches(s)).collect();
    let (mut a, mut b) = (pick, pick);
    while a > 0 && (hits[a - 1] || (a > 1 && hits[a - 2])) {
        a -= 1;
    }
    while b + 1 < hits.len() && (hits[b + 1] || (b + 2 < hits.len() && hits[b + 2])) {
        b += 1;
    }
    if hits.iter().enumerate().any(|(i, h)| *h && (i < a || i > b)) {
        return Err("That picture is too close to normal play; Deck would count deaths that didn't happen. Pick the clearest death screen.".into());
    }
    Ok(print)
}

/// The cells that are part of a line across: words ("KHAZAN HAS FALLEN"), or a symbol's strokes. The
/// figure and the scene behind a death screen are different at every death; those edges go.
fn in_a_line(cells: &[usize]) -> Vec<usize> {
    let mut on = vec![false; TW * TH];
    for &i in cells {
        on[i] = true;
    }
    let mut keep = vec![];
    for y in 0..TH {
        let mut x = 0;
        while x < TW {
            if !on[y * TW + x] {
                x += 1;
                continue;
            }
            // a run along the row, one missing cell allowed (the gap between words)
            let start = x;
            let mut end = x;
            while end + 1 < TW && (on[y * TW + end + 1] || (end + 2 < TW && on[y * TW + end + 2])) {
                end += if on[y * TW + end + 1] { 1 } else { 2 };
            }
            if end - start + 1 >= LINE {
                keep.extend((start..=end).map(|x| y * TW + x).filter(|i| on[*i]));
            }
            x = end + 1;
        }
    }
    keep
}

/// The cells of a picture that differ from the cells around them (5x5): words, a symbol, an edge.
fn stands_out(p: &[u8]) -> Vec<usize> {
    (0..TW * TH)
        .filter(|i| {
            let (x, y) = (i % TW, i / TW);
            let (mut sum, mut n) = (0u32, 0u32);
            for yy in y.saturating_sub(2)..(y + 3).min(TH) {
                for xx in x.saturating_sub(2)..(x + 3).min(TW) {
                    sum += p[yy * TW + xx] as u32;
                    n += 1;
                }
            }
            p[*i].abs_diff((sum / n) as u8) >= STANDS_OUT
        })
        .collect()
}

/// A share code: the game and its prints, as one line of text.
pub fn share(game: &str, name: &str, prints: &[Print]) -> String {
    let v = serde_json::json!({ "deck_death_screen": 1, "game": game, "name": name, "prints": prints });
    format!("deck-death-screen:{}", crate::util::base64(v.to_string().as_bytes()))
}

/// (game key, game name, prints) from a share code.
pub fn unshare(code: &str) -> Result<(String, String, Vec<Print>), String> {
    let bad = || "That isn't a Deck death-screen code.".to_string();
    let body = code.trim().strip_prefix("deck-death-screen:").ok_or_else(bad)?;
    let bytes = crate::util::unbase64(body).ok_or_else(bad)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    // the same key and name rules as the shared library: a code from a friend (or anyone) must not
    // make a phantom game with an odd key or an enormous name
    let game = v["game"].as_str().filter(|g| crate::deaths::library::good_key(g)).ok_or_else(bad)?.to_string();
    let name = crate::deaths::library::clean_name(v["name"].as_str().unwrap_or(&game)).unwrap_or_else(|| game.clone());
    let prints: Vec<Print> = serde_json::from_value(v["prints"].clone()).map_err(|_| bad())?;
    if prints.is_empty() || prints.iter().any(|p| Taught::new(p).is_none()) {
        return Err(bad());
    }
    Ok((game, name, prints.into_iter().take(MAX_PER_GAME).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// gameplay: a soft gradient that drifts a little each frame
    fn play(i: usize) -> Vec<u8> {
        (0..TW * TH).map(|c| (60 + (c % TW) + (c / TW) + (i * 3) % 20) as u8).collect()
    }

    /// the death screen: the picture darkened, a red symbol (bright cells) in the middle
    fn death(i: usize) -> Vec<u8> {
        let mut f: Vec<u8> = play(i).iter().map(|v| v / 4).collect();
        for y in 12..24 {
            for x in 26..38 {
                if (x + y) % 3 != 0 {
                    f[y * TW + x] = 150;
                }
            }
        }
        f
    }

    fn strip() -> Vec<Vec<u8>> {
        (0..24).map(play).chain((24..30).map(death)).collect()
    }

    #[test]
    fn a_picked_death_screen_is_learned_and_then_seen() {
        let p = learn(&strip(), 27, 1).unwrap();
        let t = Taught::new(&p).unwrap();
        assert!(t.matches(&death(40)), "the same death screen later");
        assert!(!t.matches(&play(41)), "normal play");
        assert!(!t.matches(&vec![2u8; TW * TH]), "a black loading screen");
        let darker: Vec<u8> = play(42).iter().map(|v| v / 4).collect();
        assert!(!t.matches(&darker), "a dark scene without the symbol");
    }

    #[test]
    fn a_bad_pick_says_why() {
        assert!(learn(&strip(), 5, 1).is_err(), "a frame of normal play");
        let only_death: Vec<Vec<u8>> = (0..10).map(death).collect();
        assert!(learn(&only_death, 3, 1).unwrap_err().contains("few seconds"));
        let flat: Vec<Vec<u8>> = (0..20).map(play).chain(std::iter::once(vec![10u8; TW * TH])).collect();
        assert!(learn(&flat, 20, 1).unwrap_err().contains("flat"), "a plain black frame isn't a death screen");
        assert!(learn(&strip(), 99, 1).is_err());
    }

    /// A dark game (Khazan): the death screen is the darkened scene with dim words, nearly as dark as
    /// the play before it, and most of the strip is the death screen (counted a while after).
    /// Khazan as it really is: bright play, then the whole screen fades nearly black and dim words
    /// fade in ("KHAZAN HAS FALLEN"); the user counts it 16 s later. A print of "everything changed"
    /// is all black and was refused as one flat colour.
    #[test]
    fn a_death_screen_that_darkens_everything_is_learned_by_its_words() {
        let bright = |i: usize| -> Vec<u8> { (0..TW * TH).map(|c| (55 + (c * 7 + i * 13) % 40) as u8).collect() };
        let dark = |i: usize, words: u8| -> Vec<u8> {
            let mut f: Vec<u8> = (0..TW * TH).map(|c| (8 + (c * 3 + i) % 5) as u8).collect();
            // a faint figure in the middle and a HUD mark top right
            for y in 14..26 {
                f[y * TW + 32] += 6;
            }
            f[3 * TW + 60] += 20;
            for x in 22..42 {
                if x % 4 != 0 {
                    f[17 * TW + x] += words;
                    f[18 * TW + x] += words / 2;
                }
            }
            f
        };
        // 24 pictures of play, 6 fading dark, 8 with the words fading in then shown
        let strip: Vec<Vec<u8>> = (0..24).map(bright).chain((24..30).map(|i| dark(i, 0))).chain((30..38).map(|i| dark(i, (8 + (i - 30) * 4).min(24) as u8))).collect();
        let p = learn(&strip, 36, 1).expect("learned");
        let t = Taught::new(&p).unwrap();
        assert!(t.matches(&dark(50, 24)), "the next death");
        assert!(!t.matches(&dark(51, 0)), "a dark screen without the words (a loading screen)");
        assert!(!t.matches(&bright(52)), "play");
    }

    /// Real death screens from picture files (never the screen): `DECK_TEACH_IMAGES=a.png;b.png`,
    /// `cargo test -- --ignored teaching_learns_real_pictures`. Taught on the first, darkened as the game
    /// shows it after bright play; every picture must then match.
    #[test]
    #[ignore]
    fn teaching_learns_real_pictures() {
        let paths = std::env::var("DECK_TEACH_IMAGES").expect("DECK_TEACH_IMAGES");
        let thumbs: Vec<Vec<u8>> = paths
            .split(';')
            .map(|p| {
                let img = image::open(p).expect("a png").to_rgba8();
                let px: Vec<u8> = img.pixels().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
                super::super::capture::thumb(&super::super::capture::Frame { w: img.width() as usize, h: img.height() as usize, px })
            })
            .collect();
        let bright = |i: usize| -> Vec<u8> { (0..TW * TH).map(|c| (60 + (c * 7 + i * 13) % 50) as u8).collect() };
        let shot = &thumbs[0];
        let strip: Vec<Vec<u8>> = (0..24).map(bright).chain(std::iter::repeat_n(shot.clone(), 6)).collect();
        let p = learn(&strip, 27, 1).expect("learned");
        let t = Taught::new(&p).unwrap();
        let rows: std::collections::BTreeSet<usize> = t.cells.iter().map(|i| i / TW).collect();
        println!("print: {} cells, rows {rows:?}", t.cells.len());
        let scaled = |k: f32| -> Vec<u8> { shot.iter().map(|v| (*v as f32 * k).min(255.0) as u8).collect() };
        assert!(t.matches(&scaled(0.8)) && t.matches(&scaled(1.25)), "a little darker or brighter");
        // another death: the scene below and above the words is different (moved 9 cells across)
        let words = *rows.iter().next().unwrap()..=*rows.iter().last().unwrap();
        let other: Vec<u8> = (0..TW * TH).map(|i| if words.contains(&(i / TW)) { shot[i] } else { shot[(i / TW) * TW + (i % TW + 9) % TW] }).collect();
        assert!(t.matches(&other), "the next death, Khazan elsewhere");
        let gone: Vec<u8> = (0..TW * TH).map(|i| if words.contains(&(i / TW)) { shot[(words.start() - 2) * TW + i % TW] } else { shot[i] }).collect();
        assert!(!t.matches(&gone), "the same dark scene without the words");
        assert!(!t.matches(&vec![6u8; TW * TH]), "a black screen");
    }

    #[test]
    fn a_dark_games_death_screen_is_learned_from_a_mostly_death_strip() {
        let dark_play = |i: usize| -> Vec<u8> { (0..TW * TH).map(|c| (14 + (c % 7) + (i % 3)) as u8).collect() };
        let banner = |i: usize| -> Vec<u8> {
            let mut f = dark_play(i);
            for y in 16..19 {
                for x in 10..54 {
                    if x % 3 != 0 {
                        f[y * TW + x] = 70;
                    }
                }
            }
            f
        };
        let black = vec![1u8; TW * TH];
        // 3 pictures of play, 14 of the death screen, 2 black, then the next one
        let strip: Vec<Vec<u8>> = (0..3).map(dark_play).chain((3..17).map(banner)).chain([black.clone(), black.clone()]).chain(std::iter::once(dark_play(20))).collect();
        let p = learn(&strip, 10, 1).expect("learned");
        let t = Taught::new(&p).unwrap();
        assert!(t.matches(&banner(30)), "the next death");
        assert!(!t.matches(&dark_play(31)), "dark play");
        assert!(!t.matches(&black), "a black loading screen");
    }

    #[test]
    fn prints_survive_a_share_code() {
        let p = learn(&strip(), 27, 7).unwrap();
        let code = share("exe:sekiro", "Sekiro", std::slice::from_ref(&p));
        let (game, name, prints) = unshare(&code).unwrap();
        assert_eq!((game.as_str(), name.as_str(), prints), ("exe:sekiro", "Sekiro", vec![p]));
        assert!(unshare("hello").is_err());
        assert!(unshare("deck-death-screen:!!!").is_err());
    }

    #[test]
    fn a_code_with_an_odd_game_or_a_huge_name_is_cleaned_or_refused() {
        let p = learn(&strip(), 27, 7).unwrap();
        let code = |game: &str, name: &str| {
            let v = serde_json::json!({ "deck_death_screen": 1, "game": game, "name": name, "prints": [p] });
            format!("deck-death-screen:{}", crate::util::base64(v.to_string().as_bytes()))
        };
        assert!(unshare(&code("../../x", "G")).is_err(), "not a key the library accepts");
        assert!(unshare(&code("", "G")).is_err());
        let (_, name, _) = unshare(&code("exe:game1x", &"é@[x](y)".repeat(40))).unwrap();
        assert!(name.chars().count() <= 100 && !name.contains(['@', '[', ']']), "{name}");
        let (_, name, _) = unshare(&code("exe:somegame", "")).unwrap();
        assert_eq!(name, "exe:somegame", "no name: the key stands in");
    }

    #[test]
    fn a_print_only_fits_pictures_of_its_shape() {
        let mut p = learn(&strip(), 27, 7).unwrap();
        p.aspect = 1777;
        let t = Taught::new(&p).unwrap();
        assert!(t.fits(1777) && t.fits(1778) && t.fits(0), "16:9, or a shape not known");
        assert!(!t.fits(2370), "21:9");
        assert!(!t.fits(1600), "16:10");
        p.aspect = 0;
        assert!(Taught::new(&p).unwrap().fits(2370), "an old print without a shape fits any");
    }
}
