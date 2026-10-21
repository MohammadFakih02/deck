//! How the in-game counter looks: the counter in its style (skull and number, ring, one huge
//! number), the card after a death, and short labels ("Already counted"). Pure: data in, a
//! picture out; `pill.rs` puts it on screen. Drawn with the overlay's SVG rasteriser, in the
//! overlay theme's colours with the death red as the one strong colour.

use super::cfg::DeathSettings;
use crate::pad::art::{self, n, Theme};
use crate::pad::raster::{self, halo, Img};

/// The death red: the count after a death, the newest try.
pub const RED: &str = "#ff6a6a";
const SKULL: &str = "#d23b3b";
const BAR: &str = "#6b2a2a";
const BAR_NEW: &str = "#e04848";
const GOLD: &str = "#e6c27a";
/// Names longer than this are cut with "…".
const NAME_MAX: usize = 28;
const SKULL_PATH: &str = "M12 2C6.5 2 3 5.8 3 10.5c0 2.8 1.3 4.7 3 5.9V20a1 1 0 0 0 1 1h2v-2h2v2h2v-2h2v2h2a1 1 0 0 0 1-1v-3.6c1.7-1.2 3-3.1 3-5.9C21 5.8 17.5 2 12 2zm-3.5 11a2 2 0 1 1 0-4 2 2 0 0 1 0 4zm7 0a2 2 0 1 1 0-4 2 2 0 0 1 0 4z";

/// What the counter shows: the open fight's boss and its deaths, the game and its deaths, and the
/// ring's mark (the deaths the last beaten boss took).
#[derive(Clone, Debug, PartialEq)]
pub struct Counter {
    pub boss: Option<(String, u32)>,
    pub game: (String, u32),
    pub ring_mark: Option<u32>,
}

/// The card after a death.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Card {
    /// the boss, or the game outside a fight
    pub title: String,
    pub in_fight: bool,
    pub try_next: u32,
    pub deaths: u32,
    pub try_ms: u64,
    pub boss_ms: u64,
    /// the boss's health left at this death (%) and the fight's phase, where its bar was seen
    pub boss_hp: Option<(u8, u8)>,
    pub session_deaths: u32,
    /// the last tries, oldest first (0 = unknown length)
    pub bars: Vec<u64>,
    pub note: Option<String>,
    pub my_note: Option<String>,
}

/// A name cut to NAME_MAX characters.
pub fn cut(s: &str) -> String {
    cut_to(s, NAME_MAX)
}

fn cut_to(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}

/// "2:41" under an hour, "1 h 05" over.
pub fn dur(ms: u64) -> String {
    let s = ms / 1000;
    if s < 3600 {
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        format!("{} h {:02}", s / 3600, s / 60 % 60)
    }
}

/// A longer time: "41 min" under an hour, "1 h 05" over.
pub fn span(ms: u64) -> String {
    if ms < 3_600_000 {
        format!("{} min", (ms / 60_000).max(1))
    } else {
        dur(ms)
    }
}

/// A rounded box `w` x `h` (pixels) with a soft shadow; content goes at (m, m) of the result.
fn panel(w: f32, h: f32, k: f32, t: &Theme, border: Option<&str>) -> (Img, i32) {
    let m = (10.0 * k).round();
    let stroke = border.unwrap_or("#ffffff");
    let so = if border.is_some() { 0.55 } else { 0.08 };
    let body = art::rect(m, m, w, h, (12.0 * k).min(h / 2.0), &format!(r#"fill="{}" fill-opacity="0.9" stroke="{stroke}" stroke-opacity="{so}" stroke-width="{}""#, t.panel, n((1.2 * k).max(1.0))));
    let (shape, _) = raster::svg(&body, "", 1.0, (0.0, 0.0, w + 2.0 * m, h + 2.0 * m));
    let (shadow, pad) = halo(&shape, "#000000", 5.0 * k, 0.55);
    let mut out = Img::new(shape.w() as u32, shape.h() as u32);
    out.blit(&shadow, -pad, -pad + (2.0 * k) as i32);
    out.blit(&shape, 0, 0);
    (out, m as i32)
}

fn icon(d: &str, color: &str, px: f32) -> Img {
    let s = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 24 24"><path d="{d}" fill="{color}" fill-rule="evenodd"/></svg>"#, px.ceil(), px.ceil());
    raster::doc(&s).unwrap_or_else(|| Img::new(1, 1))
}

/// A ring `px` across: a faint track, and an arc `part` (0 – 1) of the way round in red.
fn ring(px: f32, part: f32, k: f32, t: &Theme) -> Img {
    let r = px / 2.0 - 3.0 * k;
    let c = std::f32::consts::TAU * r;
    let s = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{w}"><circle cx="{h}" cy="{h}" r="{r}" fill="{p}" fill-opacity="0.9" stroke="#ffffff" stroke-opacity="0.12" stroke-width="{sw}"/><circle cx="{h}" cy="{h}" r="{r}" fill="none" stroke="{SKULL}" stroke-width="{sw}" stroke-linecap="round" stroke-dasharray="{c}" stroke-dashoffset="{off}" transform="rotate(-90 {h} {h})"/></svg>"##,
        w = px.ceil(),
        h = n(px / 2.0),
        r = n(r),
        p = t.panel,
        sw = n(4.0 * k),
        c = n(c),
        off = n(c * (1.0 - part.clamp(0.0, 1.0)))
    );
    raster::doc(&s).unwrap_or_else(|| Img::new(1, 1))
}

/// Pieces laid out left to right, centred vertically, `gap` apart; returns the strip.
fn hstack(parts: &[&Img], gap: i32) -> Img {
    let w = parts.iter().map(|p| p.w()).sum::<i32>() + gap * (parts.len() as i32 - 1).max(0);
    let h = parts.iter().map(|p| p.h()).max().unwrap_or(1);
    let mut out = Img::new(w.max(1) as u32, h.max(1) as u32);
    let mut x = 0;
    for p in parts {
        out.blit(p, x, (h - p.h()) / 2);
        x += p.w() + gap;
    }
    out
}

/// Pieces one under another, left aligned (`right`: right aligned), `gap` apart.
fn vstack(parts: &[&Img], gap: i32, right: bool) -> Img {
    let w = parts.iter().map(|p| p.w()).max().unwrap_or(1);
    let h = parts.iter().map(|p| p.h()).sum::<i32>() + gap * (parts.len() as i32 - 1).max(0);
    let mut out = Img::new(w.max(1) as u32, h.max(1) as u32);
    let mut y = 0;
    for p in parts {
        out.blit(p, if right { w - p.w() } else { 0 }, y);
        y += p.h() + gap;
    }
    out
}

/// `content` in a panel with `pad` pixels around it.
fn boxed(content: &Img, padx: f32, pady: f32, k: f32, t: &Theme, border: Option<&str>) -> Img {
    let (mut p, m) = panel(content.w() as f32 + 2.0 * padx, content.h() as f32 + 2.0 * pady, k, t, border);
    p.blit(content, m + padx.round() as i32, m + pady.round() as i32);
    p
}

/// The settings leave something to show for `c`.
pub fn shows(c: &Counter, s: &DeathSettings) -> bool {
    c.boss.is_some() || (s.pill_counts != "boss" && s.pill_outside != "hide")
}

/// The counter while you play, or None when the settings leave nothing to show.
pub fn counter(c: &Counter, s: &DeathSettings, t: &Theme, k: f32) -> Option<Img> {
    let fight = c.boss.as_ref().filter(|_| s.pill_counts != "game");
    if !shows(c, s) {
        return None;
    }
    // the main number and its name (the skull style shows no name), and the game's total beside it
    // when counting both
    let (main, name) = match fight {
        Some((boss, d)) => (*d, cut(boss)),
        None => (c.game.1, cut(&c.game.0)),
    };
    let second = (fight.is_some() && s.pill_counts == "both").then_some(c.game.1);
    let txt = |s: &str, px: f32, w: &str, col: &str| art::text_line(s, px * k, w, col);
    match s.pill_style.as_str() {
        "ring" => {
            let part = match (fight.is_some(), c.ring_mark) {
                (true, Some(m)) if m > 0 => main as f32 / m as f32,
                _ if main == 0 => 0.0,
                _ => ((main - 1) % 10 + 1) as f32 / 10.0,
            };
            let mut r = ring(48.0 * k, part, k, t);
            let num = txt(&main.to_string(), 17.0, "bold", t.text);
            r.blit(&num, (r.w() - num.w()) / 2, (r.h() - num.h()) / 2);
            let under = match (second, c.ring_mark.filter(|_| fight.is_some())) {
                (Some(g), _) => format!("{g} in game"),
                (None, Some(m)) => format!("last boss took {m}"),
                _ => String::new(),
            };
            let n1 = txt(&name, 12.0, "semibold", t.text);
            let n2 = txt(&under, 10.0, "semibold", t.dim);
            let words = if under.is_empty() { vstack(&[&n1], 0, true) } else { vstack(&[&n1, &n2], (2.0 * k) as i32, true) };
            Some(boxed(&hstack(&[&words, &r], (9.0 * k) as i32), 10.0 * k, 6.0 * k, k, t, None))
        }
        "huge" => {
            let num = txt(&main.to_string(), 58.0, "bold", t.text);
            let lab = txt(&name.to_uppercase(), 10.5, "bold", RED);
            let mut parts = vec![&num, &lab];
            let g = second.map(|g| txt(&format!("{g} in game"), 10.0, "semibold", t.dim));
            if let Some(g) = &g {
                parts.push(g);
            }
            let body = vstack(&parts, (3.0 * k) as i32, true);
            // no box: a soft dark shadow keeps it readable on a bright scene
            let pad = (12.0 * k) as i32;
            let mut out = Img::new((body.w() + 2 * pad) as u32, (body.h() + 2 * pad) as u32);
            let (shadow, sp) = halo(&body, "#000000", 6.0 * k, 0.7);
            out.blit(&shadow, pad - sp, pad - sp + (2.0 * k) as i32);
            out.blit(&body, pad, pad);
            Some(out)
        }
        _ => {
            // just the skull and the number (the game's total after a slash when counting both)
            let sk = icon(SKULL_PATH, SKULL, 21.0 * k);
            let num = txt(&main.to_string(), 26.0, "bold", t.text);
            let mut parts: Vec<&Img> = vec![&sk, &num];
            let sec = second.map(|g| txt(&format!("/ {g}"), 16.0, "bold", t.dim));
            if let Some(g) = &sec {
                parts.push(g);
            }
            Some(boxed(&hstack(&parts, (8.0 * k) as i32), 12.0 * k, 7.0 * k, k, t, None))
        }
    }
}

/// Bars for the last tries, `w` x `h` pixels: taller = lasted longer, the newest in red.
fn bars(v: &[u64], w: f32, h: f32, k: f32) -> Img {
    let n_ = v.len().max(1) as f32;
    let gap = 3.0 * k;
    let bw = ((w - gap * (n_ - 1.0)) / n_).clamp(2.0 * k, 16.0 * k);
    let max = v.iter().copied().max().unwrap_or(0).max(1) as f32;
    let mut body = String::new();
    for (i, ms) in v.iter().enumerate() {
        let bh = if *ms == 0 { 2.0 * k } else { (*ms as f32 / max * h).max(3.0 * k) };
        let col = if i + 1 == v.len() { BAR_NEW } else { BAR };
        body += &art::rect(i as f32 * (bw + gap), h - bh, bw, bh, 1.5 * k, &format!(r#"fill="{col}""#));
    }
    raster::svg(&body, "", 1.0, (0.0, 0.0, w, h)).0
}

/// The card after a death; its lines follow `s.card_lines`.
pub fn card(c: &Card, s: &DeathSettings, t: &Theme, k: f32) -> Img {
    let l = &s.card_lines;
    let txt = |s: &str, px: f32, w: &str, col: &str| art::text_line(s, px * k, w, col);
    let width = 236.0 * k;
    let gap = (6.0 * k) as i32;
    let mut rows: Vec<Img> = vec![];
    // the name, and the next try
    let title = txt(&cut(&c.title), 13.0, "bold", t.text);
    if l.try_no && c.in_fight {
        let tn = txt(&format!("Try {} next", c.try_next), 11.0, "semibold", "#d98c8c");
        let mut row = Img::new(width as u32, title.h().max(tn.h()) as u32);
        row.blit(&title, 0, (row.h() - title.h()) / 2);
        row.blit(&tn, row.w() - tn.w(), (row.h() - tn.h()) / 2);
        rows.push(row);
    } else {
        rows.push(title);
    }
    // the deaths, big and red
    let big = txt(&c.deaths.to_string(), 32.0, "bold", RED);
    let what = txt(if c.in_fight { "deaths to this boss" } else { "deaths in this game" }, 11.0, "semibold", t.dim);
    rows.push(hstack(&[&big, &what], (8.0 * k) as i32));
    // a row of small figures
    let mut figs: Vec<Img> = vec![];
    let fig = |v: String, lab: &str| vstack(&[&txt(&v, 15.0, "bold", t.text), &txt(lab, 9.5, "semibold", t.dim)], 0, false);
    if l.try_len && c.try_ms > 0 {
        figs.push(fig(dur(c.try_ms), "this try"));
    }
    if let Some((hp, phase)) = c.boss_hp.filter(|_| l.boss_hp && c.in_fight) {
        figs.push(fig(format!("{hp}%"), &if phase > 1 { format!("boss left, phase {phase}") } else { "boss left".into() }));
    }
    if l.boss_time && c.in_fight && c.boss_ms > 0 {
        figs.push(fig(span(c.boss_ms), "on this boss"));
    }
    if l.session {
        figs.push(fig(c.session_deaths.to_string(), "this session"));
    }
    if !figs.is_empty() {
        rows.push(hstack(&figs.iter().collect::<Vec<_>>(), (16.0 * k) as i32));
    }
    if l.bars && c.bars.len() > 1 {
        rows.push(bars(&c.bars, width, 18.0 * k, k));
    }
    if let Some(note) = c.note.as_ref().filter(|_| l.note) {
        rows.push(txt(&cut_to(note, 40), 11.0, "semibold", GOLD));
    }
    if let Some(mine) = c.my_note.as_ref().filter(|m| l.my_note && !m.is_empty()) {
        rows.push(txt(&cut_to(mine, 40), 11.0, "regular", t.dim));
    }
    let body = vstack(&rows.iter().collect::<Vec<_>>(), gap, false);
    boxed(&body, 13.0 * k, 11.0 * k, k, t, Some(SKULL))
}

/// A picture to share: a boss's or a game's numbers, 600 x 315. `rows`: up to 4 (label, value);
/// `bars`: the tries, oldest first.
pub fn share(title: &str, sub: &str, rows: &[(&str, String)], bars_ms: &[u64], t: &Theme) -> Img {
    let (w, h) = (600.0, 315.0);
    let defs = format!(r#"<clipPath id="card">{}</clipPath>"#, art::rect(0.0, 0.0, w, h, 18.0, ""));
    let body = art::rect(0.0, 0.0, w, h, 18.0, &format!(r#"fill="{}""#, t.panel)) + &art::rect(0.0, 0.0, 6.0, h, 0.0, &format!(r#"fill="{SKULL}" clip-path="url(#card)""#));
    let mut out = raster::svg(&body, &defs, 1.0, (0.0, 0.0, w, h)).0;
    let sk = icon(SKULL_PATH, SKULL, 34.0);
    out.blit(&sk, 34, 30);
    let ti = art::text_line(&cut_to(title, 30), 30.0, "bold", t.text);
    out.blit(&ti, 80, 26);
    let su = art::text_line(&cut_to(sub, 50), 14.0, "semibold", t.dim);
    out.blit(&su, 82, 66);
    let mut x = 34;
    for (i, (label, value)) in rows.iter().take(4).enumerate() {
        let v = art::text_line(value, 34.0, "bold", if i == 0 { RED } else { t.text });
        let l = art::text_line(label, 13.0, "semibold", t.dim);
        out.blit(&v, x, 112);
        out.blit(&l, x + 2, 162);
        x += v.w().max(l.w()) + 36;
    }
    if bars_ms.len() > 1 {
        let b = bars(&bars_ms[bars_ms.len().saturating_sub(30)..], 532.0, 70.0, 1.6);
        out.blit(&b, 34, 202);
    }
    let deck = art::text_line("Deck", 12.0, "semibold", t.dim);
    out.blit(&deck, 600 - deck.w() - 22, 315 - deck.h() - 12);
    out
}

/// A short word in the counter's box ("Already counted", "Undone", "Defeated").
pub fn label(text: &str, t: &Theme, k: f32) -> Img {
    let tx = art::text_line(&cut(text), 14.0 * k, "semibold", t.text);
    boxed(&tx, 14.0 * k, 8.0 * k, k, t, Some(SKULL))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(name: &str, boss: u32, game: u32) -> Counter {
        Counter { boss: Some((name.into(), boss)), game: ("Game".into(), game), ring_mark: Some(22) }
    }

    fn sample() -> Card {
        Card {
            title: "Margit".into(),
            in_fight: true,
            try_next: 16,
            deaths: 15,
            try_ms: 161_000,
            boss_ms: 41 * 60_000,
            boss_hp: Some((23, 1)),
            session_deaths: 23,
            bars: vec![30_000, 45_000, 25_000, 70_000, 55_000, 80_000, 40_000, 90_000, 65_000, 100_000],
            note: Some("Your longest try yet".into()),
            my_note: Some("dodge left".into()),
        }
    }

    #[test]
    fn every_style_draws_and_grows_with_the_number() {
        for style in ["skull", "ring", "huge"] {
            let s = DeathSettings { pill_style: style.into(), ..Default::default() };
            let a = counter(&one("Margit", 9, 40), &s, art::theme("ds4"), 1.0).unwrap();
            let b = counter(&one("Margit", 1234, 40), &s, art::theme("ds4"), 1.0).unwrap();
            assert!(b.w() >= a.w(), "{style}");
            assert!(a.w() > 40 && a.h() > 30, "{style}: {}x{}", a.w(), a.h());
        }
    }

    #[test]
    fn the_skull_style_is_only_the_skull_and_the_number() {
        let s = DeathSettings::default();
        let t = art::theme("ds4");
        let a = counter(&one("M", 9, 40), &s, t, 1.0).unwrap();
        let b = counter(&one("Malenia, Blade of Miquella", 9, 40), &s, t, 1.0).unwrap();
        assert_eq!((a.w(), a.h()), (b.w(), b.h()), "no boss name");
        let outside = |name: &str| counter(&Counter { boss: None, game: (name.into(), 27), ring_mark: None }, &s, t, 1.0).unwrap();
        let (a, b) = (outside("G"), outside("The First Berserker: Khazan"));
        assert_eq!((a.w(), a.h()), (b.w(), b.h()), "no game name");
        assert!(a.w() < 110, "small: {}", a.w());
    }

    #[test]
    fn nothing_to_show_draws_nothing() {
        let none = Counter { boss: None, game: ("G".into(), 3), ring_mark: None };
        let s = DeathSettings { pill_counts: "boss".into(), ..Default::default() };
        assert!(counter(&none, &s, art::theme("ds4"), 1.0).is_none());
        let s = DeathSettings { pill_outside: "hide".into(), ..Default::default() };
        assert!(counter(&none, &s, art::theme("ds4"), 1.0).is_none());
        assert!(counter(&none, &DeathSettings::default(), art::theme("ds4"), 1.0).is_some(), "the game's total outside a fight");
    }

    #[test]
    fn both_counters_are_wider_than_one() {
        let both = counter(&one("Margit", 9, 212), &DeathSettings::default(), art::theme("ds4"), 1.0).unwrap();
        let s = DeathSettings { pill_counts: "boss".into(), ..Default::default() };
        let boss = counter(&one("Margit", 9, 212), &s, art::theme("ds4"), 1.0).unwrap();
        assert!(both.w() > boss.w());
    }

    #[test]
    fn the_card_gets_shorter_when_lines_are_off() {
        let all = card(&sample(), &DeathSettings::default(), art::theme("ds4"), 1.0);
        let mut s = DeathSettings::default();
        s.card_lines.bars = false;
        s.card_lines.note = false;
        assert!(card(&sample(), &s, art::theme("ds4"), 1.0).h() < all.h());
        s.card_lines.my_note = true;
        let with_note = card(&sample(), &s, art::theme("ds4"), 1.0);
        s.card_lines.my_note = false;
        assert!(with_note.h() > card(&sample(), &s, art::theme("ds4"), 1.0).h(), "your own note adds a line");
    }

    #[test]
    fn a_long_name_is_cut_not_widened() {
        let s = DeathSettings::default();
        let short = counter(&one(&"界".repeat(28), 5, 5), &s, art::theme("ds4"), 1.0).unwrap();
        let long = counter(&one(&"界".repeat(300), 5, 5), &s, art::theme("ds4"), 1.0).unwrap();
        assert!(long.w() <= short.w() + 20, "{} vs {}", long.w(), short.w());
        let c = Card { title: "x".repeat(400), ..sample() };
        assert!(card(&c, &s, art::theme("ds4"), 1.0).w() < 400);
    }

    #[test]
    fn the_share_picture_is_600_by_315() {
        let p = share("Margit", "ELDEN RING", &[("deaths", "24".into()), ("tries", "25".into()), ("on this boss", "1 h 36".into()), ("best try", "3:18".into())], &[1, 2, 3, 0, 5], art::theme("ds4"));
        assert_eq!((p.w(), p.h()), (600, 315));
    }

    #[test]
    fn durations_read_like_a_clock() {
        assert_eq!((dur(161_000), dur(3_900_000), dur(0)), ("2:41".into(), "1 h 05".into(), "0:00".into()));
        assert_eq!((span(41 * 60_000), span(5_000), span(3_900_000)), ("41 min".into(), "1 min".into(), "1 h 05".into()));
    }

    /// Saves every style and the card as pictures to look at: `DECK_LOOK_DIR=<folder>`,
    /// `cargo test -- --ignored draw_samples`.
    #[test]
    #[ignore]
    fn draw_samples() {
        let dir = std::path::PathBuf::from(std::env::var("DECK_LOOK_DIR").expect("DECK_LOOK_DIR"));
        let save = |name: &str, img: &Img| {
            let png = crate::apps::encode_png(&img.rgba_straight(), img.w() as u32, img.h() as u32).unwrap();
            std::fs::write(dir.join(format!("{name}.png")), png).unwrap();
        };
        let t = art::theme("ds4");
        for style in ["skull", "ring", "huge"] {
            for counts in ["both", "boss"] {
                let s = DeathSettings { pill_style: style.into(), pill_counts: counts.into(), ..Default::default() };
                save(&format!("{style}-{counts}"), &counter(&one("Malenia, Blade of Miquella", 14, 212), &s, t, 1.5).unwrap());
            }
            let s = DeathSettings { pill_style: style.into(), ..Default::default() };
            save(&format!("{style}-outside"), &counter(&Counter { boss: None, game: ("The First Berserker - Khazan".into(), 212), ring_mark: None }, &s, t, 1.5).unwrap());
        }
        let mut s = DeathSettings::default();
        s.card_lines.my_note = true;
        save("card", &card(&sample(), &s, t, 1.5));
        save("label", &label("Already counted", t, 1.5));
        save("share", &share("Malenia, Blade of Miquella", "ELDEN RING", &[("deaths", "14".into()), ("tries", "15".into()), ("on this boss", "47 min".into()), ("best try", "2:41".into())], &sample().bars, t));
    }
}
