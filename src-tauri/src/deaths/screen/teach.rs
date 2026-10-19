//! Taught death screens: for games whose death screen has no words Windows can read (Sekiro's
//! 死, a skull, a red flash), you show Deck the death screen once. Deck keeps the last 10 s of
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

/// Learns the death screen in `strip[pick]` (thumbnails, oldest first) against the frames that
/// look different from it (normal play). The error is for the page, in plain words.
pub fn learn(strip: &[Vec<u8>], pick: usize, made: u64) -> Result<Print, String> {
    let chosen = strip.get(pick).filter(|t| t.len() == TW * TH).ok_or("That picture is gone; count the next death and try again.")?;
    let others: Vec<&Vec<u8>> = strip.iter().filter(|t| t.len() == TW * TH && mean_diff(t, chosen) >= OTHER).collect();
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
    let cells: Vec<usize> = (0..TW * TH).filter(|i| chosen[*i].abs_diff(base[*i]) >= CHANGED).collect();
    if cells.len() < MIN_CELLS {
        return Err("That picture looks too much like the game around it. Pick the one where the death screen is clearest.".into());
    }
    let values: Vec<u8> = cells.iter().map(|i| chosen[*i]).collect();
    let mean = values.iter().map(|v| *v as f32).sum::<f32>() / values.len() as f32;
    let spread = values.iter().map(|v| (*v as f32 - mean).abs()).sum::<f32>() / values.len() as f32;
    if spread < MIN_SPREAD {
        return Err("That picture is mostly one flat colour, which could be any dark or loading screen. Pick one where the death screen's words or symbol show.".into());
    }
    let mut bits = vec![0u8; TW * TH / 8];
    for i in &cells {
        bits[i / 8] |= 1 << (i % 8);
    }
    let print = Print { mask: hex(&bits), values: hex(&values), made, aspect: 0 };
    let t = Taught::new(&print).ok_or("Couldn't learn that picture.")?;
    if others.iter().any(|o| t.matches(o)) {
        return Err("That picture is too close to normal play; Deck would count deaths that didn't happen. Pick the clearest death screen.".into());
    }
    Ok(print)
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

    #[test]
    fn prints_survive_a_share_code() {
        let p = learn(&strip(), 27, 7).unwrap();
        let code = share("steam:814380", "Sekiro", std::slice::from_ref(&p));
        let (game, name, prints) = unshare(&code).unwrap();
        assert_eq!((game.as_str(), name.as_str(), prints), ("steam:814380", "Sekiro", vec![p]));
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
        let (_, name, _) = unshare(&code("steam:1", &"é@[x](y)".repeat(40))).unwrap();
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
