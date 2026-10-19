//! The overlay's art: SVG built in code (a DualShock 4 seen from the front, a compact layout,
//! and a keyboard + mouse), rasterised once per look, then each frame is the idle base plus
//! whichever pressed sprites apply. A frame is only composed when the input changed.
//! Ported from Controller Overlay 4.2 (`Art`, `KbmArt`); positions are design units.

use super::battery::Reading;
use super::cfg;
use super::input::{KbmState, PadState};
use super::raster::{self, halo, Img};
use std::collections::HashMap;

pub struct Theme {
    pub body: &'static str,
    /// the gradients' top colours: the old renderer never drew them (see theme_defs), kept with
    /// the colour tables so a real-gradient look stays one change away
    #[allow(dead_code)]
    pub body_top: &'static str,
    pub rim: &'static str,
    pub cap: &'static str,
    #[allow(dead_code)]
    pub cap_top: &'static str,
    pub glyph: &'static str,
    pub glyph_on: &'static str,
    pub accent: &'static str,
    pub light: &'static str,
    pub text: &'static str,
    pub dim: &'static str,
    pub panel: &'static str,
    pub face_colors: bool,
}

const ACTIVE: &str = "#5ac8fa";
pub const OK_GREEN: &str = "#4cd964";
pub const LOW_RED: &str = "#ff5a5a";

pub fn theme(name: &str) -> &'static Theme {
    static DS4: Theme = Theme { body: "#0c0f15", body_top: "#29303e", rim: "#ffffff", cap: "#141820", cap_top: "#394252", glyph: "#b4bdcc", glyph_on: "#0a0d12", accent: "#3f8cff", light: "#4a94ff", text: "#eef1f6", dim: "#9aa3b5", panel: "#0f1218", face_colors: true };
    static MIDNIGHT: Theme = Theme { body: "#0a1120", body_top: "#1f2c46", rim: "#cfe6ff", cap: "#121a2a", cap_top: "#2d3b58", glyph: "#a3b8d9", glyph_on: "#06101c", accent: ACTIVE, light: ACTIVE, text: "#e8f4ff", dim: "#8ea3c2", panel: "#0d1422", face_colors: false };
    static DUALSENSE: Theme = Theme { body: "#d9dee7", body_top: "#ffffff", rim: "#7d879a", cap: "#1a1e26", cap_top: "#3b4250", glyph: "#ccd3df", glyph_on: "#0a0d12", accent: "#2f6bff", light: "#6fa0ff", text: "#eef1f6", dim: "#9aa3b5", panel: "#0f1218", face_colors: true };
    static GOLD: Theme = Theme { body: "#110e08", body_top: "#302818", rim: "#ffe7b0", cap: "#19150d", cap_top: "#3d3322", glyph: "#dccca6", glyph_on: "#140f05", accent: "#e6b450", light: "#e6b450", text: "#f3ead6", dim: "#a89a78", panel: "#14110b", face_colors: false };
    static MONO: Theme = Theme { body: "#000000", body_top: "#1e1e1e", rim: "#ffffff", cap: "#0c0c0c", cap_top: "#2c2c2c", glyph: "#e8e8e8", glyph_on: "#000000", accent: "#ffffff", light: "#ffffff", text: "#ffffff", dim: "#cfcfcf", panel: "#000000", face_colors: false };
    match name {
        "midnight" => &MIDNIGHT,
        "dualsense" => &DUALSENSE,
        "gold" => &GOLD,
        "mono" => &MONO,
        _ => &DS4,
    }
}

/// Internal keys stay PlayStation-named; each family maps them to its labels.
pub struct Family {
    /// glyph per face button: a symbol name, or a single letter
    pub glyphs: [(&'static str, &'static str); 4],
    pub colors: [(&'static str, &'static str); 4],
    pub l1: &'static str,
    pub r1: &'static str,
    pub l2: &'static str,
    pub r2: &'static str,
}

pub fn family(name: &str) -> &'static Family {
    static PS: Family = Family {
        glyphs: [("triangle", "triangle"), ("circle", "circle"), ("cross", "cross"), ("square", "square")],
        colors: [("triangle", "#3ee0a8"), ("circle", "#ff5a6e"), ("cross", "#7fa8ff"), ("square", "#f590dc")],
        l1: "L1",
        r1: "R1",
        l2: "L2",
        r2: "R2",
    };
    static XBOX: Family = Family {
        glyphs: [("triangle", "Y"), ("circle", "B"), ("cross", "A"), ("square", "X")],
        colors: [("triangle", "#f5c22d"), ("circle", "#f0524f"), ("cross", "#5cc94f"), ("square", "#3f8ef0")],
        l1: "LB",
        r1: "RB",
        l2: "LT",
        r2: "RT",
    };
    if name == "xbox" { &XBOX } else { &PS }
}

fn lookup(list: &[(&'static str, &'static str); 4], key: &str) -> &'static str {
    list.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or("")
}

/// "PlayStation" names unless the pad's own name says Xbox-style.
pub fn family_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["ps3", "ps4", "ps5", "dualsense", "dualshock", "playstation", "sony", "wireless controller"].iter().any(|s| n.contains(s)) {
        "ps"
    } else {
        "xbox"
    }
}

pub const LAYOUT_SIZE: [(&str, (f32, f32)); 2] = [("controller", (409.0, 272.0)), ("compact", (304.0, 124.0))];
pub const STICK_DEADZONE: f32 = 0.12;

// The DS4 outline, traced from a product photo and smoothed: the right half from the top centre,
// clockwise round the grip, to the notch between the grips. The left half is its mirror image.
pub const DS4_AXIS: f32 = 204.5;
const DS4_HALF: [(f32, f32); 23] = [
    (204.5, 26.5), (329.5, 27.5), (344.0, 29.5), (351.0, 32.5), (363.0, 51.5), (373.0, 74.5), (391.0, 147.5), (397.0, 193.5),
    (395.0, 226.5), (390.0, 240.5), (382.0, 251.5), (375.0, 256.5), (367.5, 257.5), (341.5, 255.5), (333.5, 252.5), (327.0, 245.5),
    (320.0, 234.5), (307.0, 205.5), (299.0, 181.5), (297.0, 177.5), (294.0, 175.5), (240.0, 174.5), (204.5, 169.5),
];
const BUMPER_R: &str = "M299,28.5 C299,21 305,17.5 314,17.5 L332,17.5 C341,17.5 347,21.5 348.5,31 C341,29.4 336,28.2 329,27.6 L306,27.6 Q300,27.6 299,28.5 Z";
const TRIGGER_R: &str = "M302,19 C303,11 309,7 318,7 L330,7 C339,7 344,11 345,19 Z";

/// A number as SVG wants it: short, no exponent.
pub fn n(v: f32) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn catmull(points: &[(f32, f32)]) -> String {
    let len = points.len();
    let mut d = format!("M{},{}", n(points[0].0), n(points[0].1));
    for i in 0..len {
        let (p0, p1, p2, p3) = (points[(i + len - 1) % len], points[i], points[(i + 1) % len], points[(i + 2) % len]);
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        d += &format!(" C{},{} {},{} {},{}", n(c1.0), n(c1.1), n(c2.0), n(c2.1), n(p2.0), n(p2.1));
    }
    d + " Z"
}

/// The DS4 body outline as an SVG path.
pub fn body_path() -> String {
    let mut pts: Vec<(f32, f32)> = DS4_HALF.to_vec();
    pts.extend(DS4_HALF[1..DS4_HALF.len() - 1].iter().rev().map(|&(x, y)| (2.0 * DS4_AXIS - x, y)));
    catmull(&pts)
}

pub fn rect(x: f32, y: f32, w: f32, h: f32, r: f32, a: &str) -> String {
    format!(r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" ry="{}" {a}/>"#, n(x), n(y), n(w), n(h), n(r), n(r))
}
pub fn circle(cx: f32, cy: f32, r: f32, a: &str) -> String {
    format!(r#"<circle cx="{}" cy="{}" r="{}" {a}/>"#, n(cx), n(cy), n(r))
}
pub fn path(d: &str, a: &str) -> String {
    format!(r#"<path d="{d}" {a}/>"#)
}

/// The paint servers the art refers to. Controller Overlay's renderer (SDL_image's nanosvg)
/// filled every gradient with its end colour, and that flat look is what its users know, so
/// each gradient here is drawn the same way: both stops take the end stop's colour and opacity.
pub fn theme_defs(t: &Theme, style: &str) -> String {
    let body_a = if style == "glass" { 0.80 } else { 1.0 };
    let lin = |id: &str, color: &str, a: f32| {
        format!(r#"<linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{color}" stop-opacity="{}"/><stop offset="1" stop-color="{color}" stop-opacity="{}"/></linearGradient>"#, n(a), n(a))
    };
    lin("body", t.body, body_a) + &lin("rim", t.rim, 0.07) + &lin("caprim", "#ffffff", 0.04) + &lin("cap", t.cap, 0.97) + &lin("sheen", "#ffffff", 0.03)
        + &format!(r#"<radialGradient id="knob" cx="0.42" cy="0.32" r="0.75"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{}"/></radialGradient>"#, t.cap, t.cap)
        + r##"<radialGradient id="well" cx="0.5" cy="0.42" r="0.6"><stop offset="0" stop-color="#000000" stop-opacity="0.30"/><stop offset="1" stop-color="#000000" stop-opacity="0.30"/></radialGradient>"##
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// One line of text in its line box (1.33 × the size tall, baseline where Segoe UI puts it),
/// as wide as its ink plus a little: centring it on a point centres the line like the old app.
pub fn text_line(text: &str, px: f32, weight: &str, color: &str) -> Img {
    let px = px.max(4.0);
    let h = (px * 1.33).ceil();
    let w = (text.chars().count() as f32 * px * 0.75 + px * 2.0).ceil();
    let wt = match weight {
        "regular" => 400,
        "semibold" => 600,
        _ => 700,
    };
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><text x="{}" y="{}" font-size="{}" font-weight="{wt}" fill="{color}">{}</text></svg>"#,
        n(px * 0.5),
        n(px * 1.079),
        n(px),
        esc(text)
    );
    let img = raster::doc(&svg).unwrap_or_else(|| Img::new(1, 1));
    match img.alpha_box() {
        Some((x0, _, x1, _)) => {
            let pad = (px * 0.06).round() as i32;
            raster::crop(&img, (x0 - pad).max(0), 0, (x1 + pad).min(img.w()), img.h())
        }
        None => Img::new(1, h as u32),
    }
}

/// Text centred on a pixel point of `dest` (whose top-left is at `origin`).
fn blit_text(dest: &mut Img, origin: (i32, i32), text: &str, center: (f32, f32), color: &str, px: f32, weight: &str) {
    let img = text_line(text, px, weight, color);
    let x = (center.0 - origin.0 as f32 - img.w() as f32 / 2.0).round() as i32;
    let y = (center.1 - origin.1 as f32 - img.h() as f32 / 2.0).round() as i32;
    dest.blit(&img, x, y);
}

/// A lightning bolt `h` tall centred on (cx, cy): the charging mark. `stretch` > 1 widens it.
pub fn bolt_path(cx: f32, cy: f32, h: f32, stretch: f32) -> String {
    let pts = [(0.12, -0.5), (-0.24, 0.07), (-0.01, 0.07), (-0.12, 0.5), (0.24, -0.07), (0.01, -0.07)];
    "M".to_string() + &pts.iter().map(|(x, y)| format!("{},{}", n(cx + x * h * stretch), n(cy + y * h))).collect::<Vec<_>>().join(" L") + " Z"
}

/// PlayStation symbols as strokes, so they stay crisp at any size.
fn glyph_svg(kind: &str, cx: f32, cy: f32, r: f32, color: &str, opacity: f32) -> String {
    let a = format!(r#"fill="none" stroke="{color}" stroke-width="{}" stroke-opacity="{}" stroke-linecap="round" stroke-linejoin="round""#, n(r * 0.15), n(opacity));
    match kind {
        "triangle" => {
            let s = r * 0.50;
            let p: Vec<(f32, f32)> = [-90f32, 30.0, 150.0].iter().map(|d| (cx + s * d.to_radians().cos(), cy + r * 0.07 + s * d.to_radians().sin())).collect();
            path(&format!("M{},{} L{},{} L{},{} Z", n(p[0].0), n(p[0].1), n(p[1].0), n(p[1].1), n(p[2].0), n(p[2].1)), &a)
        }
        "circle" => circle(cx, cy, r * 0.42, &a),
        "cross" => {
            let s = r * 0.36;
            path(&format!("M{},{} L{},{} M{},{} L{},{}", n(cx - s), n(cy - s), n(cx + s), n(cy + s), n(cx - s), n(cy + s), n(cx + s), n(cy - s)), &a)
        }
        "square" => {
            let s = r * 0.34;
            rect(cx - s, cy - s, 2.0 * s, 2.0 * s, r * 0.04, &a)
        }
        _ => String::new(),
    }
}

/// A small glass pill holding one or two lines of text (toasts, messages): (text, px, weight, colour).
pub fn pill(lines: &[(&str, f32, &str, &str)], t: &Theme, k: f32, border: Option<&str>) -> Img {
    let imgs: Vec<Img> = lines.iter().map(|(text, px, w, c)| text_line(text, px * k, w, c)).collect();
    let (padx, pady, gap, m) = (16.0 * k, 8.0 * k, 1.0 * k, 10.0 * k);
    let w = imgs.iter().map(|i| i.w()).max().unwrap_or(1) as f32 + 2.0 * padx;
    let h = imgs.iter().map(|i| i.h()).sum::<i32>() as f32 + gap * (imgs.len().saturating_sub(1)) as f32 + 2.0 * pady;
    let defs = theme_defs(t, "solid");
    let stroke = border.map(str::to_string).unwrap_or_else(|| "url(#rim)".into());
    let body = rect(m, m, w, h, (h / 2.0).min(16.0 * k), &format!(r#"fill="{}" fill-opacity="0.92" stroke="{stroke}" stroke-width="{}""#, t.panel, n((1.2 * k).max(1.0))));
    let (shape, _) = raster::svg(&body, &defs, 1.0, (0.0, 0.0, w + 2.0 * m, h + 2.0 * m));
    let (shadow, pad) = halo(&shape, "#000000", 5.0 * k, 0.55);
    let mut out = Img::new(shape.w() as u32, shape.h() as u32);
    out.blit(&shadow, -pad, -pad + (2.0 * k) as i32);
    out.blit(&shape, 0, 0);
    let mut y = m + pady;
    for img in &imgs {
        out.blit(img, ((out.w() - img.w()) as f32 / 2.0).round() as i32, y.round() as i32);
        y += img.h() as f32 + gap;
    }
    out
}

/// What one overlay picture is built for. Two equal looks share one set of sprites.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// "controller" | "compact" | "kbm"
    pub layout: String,
    pub theme: String,
    pub style: String,
    /// scale × the monitor's DPI factor, rounded to 3 places
    pub k: f32,
    /// "ps" | "xbox"
    pub family: &'static str,
    pub keys: Vec<String>,
    pub mouse: bool,
}

/// A sprite at a pixel position.
#[derive(Clone)]
struct Sprite {
    img: Img,
    x: i32,
    y: i32,
}

struct Trigger {
    key: &'static str,
    fill: Sprite,
    /// the fill's columns, in its own pixels
    x0: i32,
    x1: i32,
    rtl: bool,
    idle: Sprite,
    on: Sprite,
}

struct Stick {
    kx: &'static str,
    ky: &'static str,
    kb: &'static str,
    /// idle, moved, pressed: layers relative to `pos`
    looks: [Vec<Sprite>; 3],
    pos: (i32, i32),
    travel: f32,
}

/// What the overlay shows this frame.
pub enum Frame<'a> {
    Pad { st: &'a PadState, battery: Option<Reading> },
    /// no controller (or one SDL can't map: its name)
    NoPad { note: &'a str },
    Kbm(&'a KbmState),
}

#[derive(PartialEq, Clone)]
enum Key {
    Off(String, bool),
    Pad(Vec<&'static str>, Vec<i32>, Vec<(i32, i32, bool)>, Option<Reading>, bool),
    Kbm(Vec<&'static str>, Vec<&'static str>, (i32, i32), bool),
}

pub struct Art {
    pub look: Look,
    k: f32,
    t: &'static Theme,
    fam: &'static Family,
    style: String,
    pub dw: f32,
    pub dh: f32,
    pub w: i32,
    pub h: i32,
    defs: String,
    base: Img,
    backdrop: Img,
    buttons: HashMap<&'static str, Vec<Sprite>>,
    order: Vec<&'static str>,
    triggers: Vec<Trigger>,
    sticks: Vec<Stick>,
    caption_at: Option<(f32, f32)>,
    caption: Option<(Option<Reading>, Option<Img>)>,
    // keyboard & mouse
    kb_keys: Vec<&'static (&'static str, &'static str, u16, u8, f32, f32)>,
    row_y: HashMap<u8, usize>,
    kx0: f32,
    kb_h: f32,
    move_dot: Option<([Vec<Sprite>; 2], f32)>,
    last: Option<Key>,
    frame: Img,
}

const KEY_UNIT: f32 = 34.0;
const KEY_GAP: f32 = 4.0;
const MOUSE_W: f32 = 80.0;
const MOUSE_H: f32 = 128.0;
pub const MOUSE_BUTTONS: [&str; 5] = ["left", "right", "middle", "x1", "x2"];
const KBM_M: f32 = 12.0;

impl Art {
    pub fn new(look: Look) -> Art {
        let k = look.k;
        let t = theme(&look.theme);
        let mut a = Art {
            k,
            t,
            fam: family(look.family),
            style: look.style.clone(),
            dw: 0.0,
            dh: 0.0,
            w: 1,
            h: 1,
            defs: theme_defs(t, &look.style),
            base: Img::new(1, 1),
            backdrop: Img::new(1, 1),
            buttons: HashMap::new(),
            order: Vec::new(),
            triggers: Vec::new(),
            sticks: Vec::new(),
            caption_at: None,
            caption: None,
            kb_keys: Vec::new(),
            row_y: HashMap::new(),
            kx0: 0.0,
            kb_h: 0.0,
            move_dot: None,
            last: None,
            frame: Img::new(1, 1),
            look,
        };
        let (dw, dh) = if a.look.layout == "kbm" { a.kbm_size() } else { LAYOUT_SIZE.iter().find(|(l, _)| *l == a.look.layout).map(|(_, s)| *s).unwrap_or(LAYOUT_SIZE[0].1) };
        a.dw = dw;
        a.dh = dh;
        a.w = (dw * k).ceil() as i32;
        a.h = (dh * k).ceil() as i32;
        a.base = Img::new(a.w as u32, a.h as u32);
        match a.look.layout.as_str() {
            "kbm" => a.build_kbm(),
            "compact" => a.build_compact(),
            _ => a.build_controller(),
        }
        a.drop_shadow();
        a.backdrop = a.make_backdrop();
        a
    }

    // --- helpers -------------------------------------------------------------------
    fn sprite(&self, body: &str, bx: (f32, f32, f32, f32)) -> Sprite {
        let (img, (x, y)) = raster::svg(body, &self.defs, self.k, bx);
        Sprite { img, x, y }
    }

    /// Draws straight onto the idle base.
    fn paint(&mut self, body: &str, bx: (f32, f32, f32, f32), glow: Option<(&str, f32, f32)>) {
        let s = self.sprite(body, bx);
        if let Some((c, r, st)) = glow {
            let (g, pad) = halo(&s.img, c, r * self.k, st);
            self.base.blit(&g, s.x - pad, s.y - pad);
        }
        self.base.blit(&s.img, s.x, s.y);
    }

    /// The pressed look of `key`: its sprite (with its label in the dark "on" colour) and a glow.
    fn pressed(&mut self, key: &'static str, body: &str, bx: (f32, f32, f32, f32), color: &str, text: Option<(&str, f32, f32, f32)>) {
        let mut s = self.sprite(body, bx);
        if let Some((txt, cx, cy, px)) = text {
            blit_text(&mut s.img, (s.x, s.y), txt, (cx * self.k, cy * self.k), self.t.glyph_on, px * self.k, "bold");
        }
        let (g, pad) = halo(&s.img, color, 5.0 * self.k, 0.9);
        if !self.buttons.contains_key(key) {
            self.order.push(key);
        }
        let list = self.buttons.entry(key).or_default();
        list.push(Sprite { img: g, x: s.x - pad, y: s.y - pad });
        list.push(s);
    }

    fn label(&mut self, text: &str, cx: f32, cy: f32, color: &str, px: f32) {
        let k = self.k;
        blit_text(&mut self.base, (0, 0), text, (cx * k, cy * k), color, px * k, "bold");
    }

    fn drop_shadow(&mut self) {
        let (shadow, pad) = halo(&self.base, "#000000", 7.0 * self.k, 0.65);
        let mut out = Img::new(self.w as u32, self.h as u32);
        out.blit(&shadow, -pad, -pad + (3.0 * self.k).round() as i32);
        out.blit(&self.base, 0, 0);
        self.base = out;
    }

    /// Move / resize: a dashed, grabbable plate behind everything (fully transparent pixels
    /// don't receive the mouse).
    fn make_backdrop(&self) -> Img {
        let s = self.sprite(
            &rect(3.0, 3.0, self.dw - 6.0, self.dh - 6.0, 18.0, &format!(r#"fill="{}" fill-opacity="0.45" stroke="{}" stroke-width="2" stroke-dasharray="7 5""#, self.t.panel, self.t.accent)),
            (0.0, 0.0, self.dw, self.dh),
        );
        let mut out = Img::new(self.w as u32, self.h as u32);
        out.blit(&s.img, s.x, s.y);
        out.blit(&self.base, 0, 0);
        out
    }

    // --- controls ------------------------------------------------------------------
    fn round(&mut self, cx: f32, cy: f32, r: f32, key: &'static str, glyph: Option<&str>, color: Option<&str>) {
        let t = self.t;
        let bx = (cx - r - 2.0, cy - r - 2.0, cx + r + 2.0, cy + r + 2.0);
        let idle = color.unwrap_or(t.glyph);
        let is_text = glyph.is_some_and(|g| g.chars().count() == 1);
        let mut body = circle(cx, cy, r, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#);
        if let (Some(g), false) = (glyph, is_text) {
            body += &glyph_svg(g, cx, cy, r, idle, 0.95);
        }
        self.paint(&body, bx, None);
        if is_text {
            self.label(glyph.unwrap_or(""), cx, cy, idle, r * 1.2);
        }
        let on = color.unwrap_or(t.accent);
        let mut body = circle(cx, cy, r, &format!(r##"fill="{on}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##));
        if let (Some(g), false) = (glyph, is_text) {
            body += &glyph_svg(g, cx, cy, r, t.glyph_on, 1.0);
        }
        let text = if is_text { glyph.map(|g| (g, cx, cy, r * 1.2)) } else { None };
        self.pressed(key, &body, bx, on, text);
    }

    #[allow(clippy::too_many_arguments)]
    fn pill_key(&mut self, x: f32, y: f32, w: f32, h: f32, key: &'static str, text: Option<&str>, r: Option<f32>, font: Option<f32>, sheen: bool) {
        let t = self.t;
        let r = r.unwrap_or(h / 2.0);
        let bx = (x - 2.0, y - 2.0, x + w + 2.0, y + h + 2.0);
        let mut body = rect(x, y, w, h, r, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#);
        if sheen {
            body += &rect(x, y, w, h, r, &format!(r#"fill="url(#sheen)" stroke="{}" stroke-opacity="0.28" stroke-width="1.2""#, t.rim));
        }
        self.paint(&body, bx, None);
        let font = font.unwrap_or(h * 0.66);
        if let Some(txt) = text {
            self.label(txt, x + w / 2.0, y + h / 2.0, t.glyph, font);
        }
        let on = rect(x, y, w, h, r, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##, t.accent));
        self.pressed(key, &on, bx, t.accent, text.map(|s| (s, x + w / 2.0, y + h / 2.0, font)));
    }

    /// L2 / R2 in the compact layout: fills sideways with analog pressure.
    #[allow(clippy::too_many_arguments)]
    fn trigger(&mut self, x: f32, y: f32, w: f32, h: f32, key: &'static str, text: &str, rtl: bool) {
        let (t, k) = (self.t, self.k);
        let bx = (x - 2.0, y - 2.0, x + w + 2.0, y + h + 2.0);
        self.paint(&rect(x, y, w, h, 6.0, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#), bx, None);
        let fill = self.sprite(&rect(x, y, w, h, 6.0, &format!(r#"fill="{}""#, t.accent)), bx);
        let (x0, x1) = (((x + 1.0) * k) as i32 - fill.x, ((x + w - 1.0) * k) as i32 - fill.x);
        self.trigger_state(key, text, x + w / 2.0, y + h / 2.0, h * 0.62, fill, x0, x1, rtl);
    }

    /// The right shoulder's art, wrapped in a mirror transform for the left side.
    fn side(&self, bx: (f32, f32, f32, f32), cx: f32, mirror: bool) -> (Box<dyn Fn(&str) -> String>, (f32, f32, f32, f32), f32) {
        if !mirror {
            return (Box::new(|b: &str| b.to_string()), bx, cx);
        }
        let m = 2.0 * DS4_AXIS;
        let tr = format!("matrix(-1 0 0 1 {} 0)", n(m));
        (Box::new(move |b: &str| format!(r#"<g transform="{tr}">{b}</g>"#)), (m - bx.2, bx.1, m - bx.0, bx.3), m - cx)
    }

    fn bumper(&mut self, key: &'static str, text: &'static str, mirror: bool) {
        let t = self.t;
        let (wrap, bx, cx) = self.side((296.0, 14.0, 352.0, 34.0), 323.5, mirror);
        let (cy, font) = (22.6, 7.6);
        self.paint(&wrap(&path(BUMPER_R, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#)), bx, None);
        self.label(text, cx, cy, t.glyph, font);
        let on = wrap(&path(BUMPER_R, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##, t.accent)));
        self.pressed(key, &on, bx, t.accent, Some((text, cx, cy, font)));
    }

    /// L2 / R2 peeking out behind the bumper; fills from the outer edge inwards.
    fn shoulder_trigger(&mut self, key: &'static str, text: &'static str, mirror: bool) {
        let (t, k) = (self.t, self.k);
        let (wrap, bx, cx) = self.side((299.0, 4.0, 348.0, 22.0), 323.5, mirror);
        self.paint(&wrap(&path(TRIGGER_R, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#)), bx, None);
        let fill = self.sprite(&wrap(&path(TRIGGER_R, &format!(r#"fill="{}""#, t.accent))), bx);
        let m = 2.0 * DS4_AXIS;
        let (lo, hi) = if mirror { (m - 345.0, m - 302.0) } else { (302.0, 345.0) };
        let (x0, x1) = ((lo * k) as i32 - fill.x, (hi * k) as i32 - fill.x);
        self.trigger_state(key, text, cx, 13.2, 7.6, fill, x0, x1, !mirror);
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger_state(&mut self, key: &'static str, text: &str, cx: f32, cy: f32, font: f32, fill: Sprite, x0: i32, x1: i32, rtl: bool) {
        let (t, k) = (self.t, self.k);
        let make = |color: &str| {
            let img = text_line(text, font * k, "bold", color);
            let (x, y) = ((cx * k - img.w() as f32 / 2.0).round() as i32, (cy * k - img.h() as f32 / 2.0).round() as i32);
            Sprite { img, x, y }
        };
        let idle = make(t.glyph);
        let on = make(t.glyph_on);
        self.base.blit(&idle.img, idle.x, idle.y);
        self.triggers.push(Trigger { key, fill, x0, x1, rtl, idle, on });
    }

    /// The raised round pad the d-pad and the face buttons sit on.
    fn disk(&mut self, cx: f32, cy: f32, r: f32) {
        self.paint(&circle(cx, cy, r, r##"fill="#ffffff" fill-opacity="0.05" stroke="url(#caprim)" stroke-width="0.9""##), (cx - r - 2.0, cy - r - 2.0, cx + r + 2.0, cy + r + 2.0), None);
    }

    fn recess(&mut self, cx: f32, cy: f32, r: f32) {
        self.paint(&circle(cx, cy, r, r##"fill="#000000" fill-opacity="0.30" stroke="url(#caprim)" stroke-width="0.8""##), (cx - r - 2.0, cy - r - 2.0, cx + r + 2.0, cy + r + 2.0), None);
    }

    /// DS4 d-pad: four separate arrow keys, pointed at the centre.
    fn dpad(&mut self, cx: f32, cy: f32, arm: f32, w: f32, gap: f32, disk: Option<f32>) {
        let t = self.t;
        match disk {
            Some(d) => self.disk(cx, cy, d),
            None => self.recess(cx, cy, arm + 6.0),
        }
        let (x0, x1, top, inner, rr) = (cx - w / 2.0, cx + w / 2.0, cy - arm, cy - gap, 4.0);
        let d = format!(
            "M{},{} L{},{} Q{},{} {},{} L{},{} Q{},{} {},{} L{},{} L{},{} Z",
            n(x0), n(inner), n(x0), n(top + rr), n(x0), n(top), n(x0 + rr), n(top), n(x1 - rr), n(top), n(x1), n(top), n(x1), n(top + rr), n(x1), n(inner), n(cx), n(inner + w * 0.42)
        );
        let ay = top + (inner - top) * 0.42;
        let a = w * 0.17;
        let arrow = format!("M{},{} L{},{} L{},{} Z", n(cx), n(ay - a * 0.9), n(cx + a), n(ay + a * 0.6), n(cx - a), n(ay + a * 0.6));
        for (key, rot) in [("up", 0), ("right", 90), ("down", 180), ("left", 270)] {
            let tr = format!("rotate({rot} {} {})", n(cx), n(cy));
            let bx = (cx - arm - 2.0, cy - arm - 2.0, cx + arm + 2.0, cy + arm + 2.0);
            let idle = format!(
                r#"<g transform="{tr}">{}{}</g>"#,
                path(&d, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#),
                path(&arrow, &format!(r#"fill="{}" fill-opacity="0.45""#, t.glyph))
            );
            self.paint(&idle, bx, None);
            let on = format!(
                r#"<g transform="{tr}">{}{}</g>"#,
                path(&d, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##, t.accent)),
                path(&arrow, &format!(r#"fill="{}" fill-opacity="0.8""#, t.glyph_on))
            );
            self.pressed(key, &on, bx, t.accent, None);
        }
    }

    fn face(&mut self, cx: f32, cy: f32, off: f32, r: f32, disk: Option<f32>) {
        match disk {
            Some(d) => self.disk(cx, cy, d),
            None => self.recess(cx, cy, off + r + 6.0),
        }
        for (key, (dx, dy)) in [("triangle", (0.0, -off)), ("circle", (off, 0.0)), ("cross", (0.0, off)), ("square", (-off, 0.0))] {
            let color = self.t.face_colors.then(|| lookup(&self.fam.colors, key));
            let glyph = lookup(&self.fam.glyphs, key);
            self.round(cx + dx, cy + dy, r, key, Some(glyph), color);
        }
    }

    /// A thumbstick: optional raised housing, a dark well, and a domed cap with the DS4's
    /// concave ring that slides with the stick.
    #[allow(clippy::too_many_arguments)]
    fn stick(&mut self, cx: f32, cy: f32, keys: (&'static str, &'static str, &'static str), well: f32, knob: f32, housing: Option<(f32, f32, f32)>, travel: Option<f32>) {
        let (t, k) = (self.t, self.k);
        if let Some((hx, hy, hr)) = housing {
            self.paint(&circle(hx, hy, hr, r##"fill="#ffffff" fill-opacity="0.05" stroke="url(#caprim)" stroke-width="0.9""##), (hx - hr - 2.0, hy - hr - 2.0, hx + hr + 2.0, hy + hr + 2.0), None);
        }
        self.paint(&circle(cx, cy, well, r#"fill="url(#well)" stroke="url(#caprim)" stroke-width="1.0""#), (cx - well - 2.0, cy - well - 2.0, cx + well + 2.0, cy + well + 2.0), None);
        let bx = (cx - knob - 2.0, cy - knob - 2.0, cx + knob + 2.0, cy + knob + 2.0);
        let dish = circle(cx, cy, knob * 0.66, r##"fill="#000000" fill-opacity="0.18" stroke="#000000" stroke-opacity="0.45" stroke-width="1.3""##);
        let looks = [
            circle(cx, cy, knob, r#"fill="url(#knob)" stroke="url(#caprim)" stroke-width="1.1""#) + &dish,
            circle(cx, cy, knob, &format!(r#"fill="url(#knob)" stroke="{}" stroke-width="2.0""#, t.accent)) + &dish,
            circle(cx, cy, knob, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.6" stroke-width="1.2""##, t.accent)) + &dish,
        ];
        let mut pos = (0, 0);
        let sprites: Vec<Vec<Sprite>> = looks
            .iter()
            .enumerate()
            .map(|(i, body)| {
                let s = self.sprite(body, bx);
                pos = (s.x, s.y);
                let (shadow, pad) = halo(&s.img, "#000000", 3.0 * k, 0.7);
                let mut layers = vec![Sprite { img: shadow, x: -pad, y: -pad + (2.0 * k).round() as i32 }];
                if i > 0 {
                    let (g, gp) = halo(&s.img, t.accent, 5.0 * k, 0.8);
                    layers.push(Sprite { img: g, x: -gp, y: -gp });
                }
                layers.push(Sprite { img: s.img, x: 0, y: 0 });
                layers
            })
            .collect();
        let [a, b, c]: [Vec<Sprite>; 3] = sprites.try_into().unwrap_or_else(|_| unreachable!());
        self.sticks.push(Stick { kx: keys.0, ky: keys.1, kb: keys.2, looks: [a, b, c], pos, travel: travel.unwrap_or(well - knob - 1.0) * k });
    }

    /// The big black touchpad with its fine dot texture.
    fn touchpad(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let t = self.t;
        let mut dots = String::new();
        for j in 0..((h - 18.0) / 3.6) as i32 {
            for i in 0..((w - 8.0) / 3.6) as i32 + 1 {
                dots += &circle(x + 4.0 + i as f32 * 3.6, y + 14.0 + j as f32 * 3.6, 0.55, "");
            }
        }
        let body = rect(x, y, w, h, r, &format!(r##"fill="#06080c" fill-opacity="0.9" stroke="{}" stroke-opacity="0.25" stroke-width="1.1""##, t.rim))
            + &rect(x, y, w, h, r, r#"fill="url(#sheen)""#)
            + &format!(r##"<g fill="#ffffff" fill-opacity="0.07">{dots}</g>"##);
        let bx = (x - 2.0, y - 2.0, x + w + 2.0, y + h + 2.0);
        self.paint(&body, bx, None);
        let on = rect(x, y, w, h, r, &format!(r#"fill="{}" fill-opacity="0.55" stroke="{}" stroke-width="1.4""#, t.accent, t.accent));
        self.pressed("touchpad", &on, bx, t.accent, None);
    }

    /// The speaker grille under the touchpad.
    fn grille(&mut self, cx: f32, y: f32) {
        let mut dots = String::new();
        for (row, cnt) in [6, 5, 4].iter().enumerate() {
            for i in 0..*cnt {
                dots += &circle(cx + (i as f32 - (*cnt as f32 - 1.0) / 2.0) * 5.6, y + row as f32 * 4.8, 1.25, "");
            }
        }
        self.paint(&format!(r##"<g fill="#000000" fill-opacity="0.55">{dots}</g>"##), (cx - 20.0, y - 4.0, cx + 20.0, y + 14.0), None);
    }

    // --- layouts -----------------------------------------------------------------------
    /// A DS4 seen from the front; positions measured from a photo of one.
    fn build_controller(&mut self) {
        let (f, t, ax) = (self.fam, self.t, DS4_AXIS);
        self.shoulder_trigger("l2", f.l2, true);
        self.shoulder_trigger("r2", f.r2, false);
        if self.style != "minimal" {
            self.paint(&path(&body_path(), r#"fill="url(#body)" stroke="url(#rim)" stroke-width="1.4""#), (0.0, 0.0, 409.0, 272.0), None);
        }
        self.bumper("l1", f.l1, true);
        self.bumper("r1", f.r1, false);
        self.touchpad(140.0, 32.0, 129.0, 73.0, 8.0);
        self.paint(&rect(162.0, 43.4, 85.0, 2.4, 1.2, &format!(r#"fill="{}""#, t.light)), (150.0, 34.0, 259.0, 56.0), Some((t.light, 4.0, 0.9)));
        self.pill_key(119.5, 43.0, 11.0, 20.0, "create", None, Some(5.5), None, true);
        self.pill_key(278.5, 43.0, 11.0, 20.0, "options", None, Some(5.5), None, true);
        self.dpad(84.0, 91.5, 33.0, 23.0, 12.0, Some(45.0));
        self.face(2.0 * ax - 84.0, 91.5, 28.5, 14.0, Some(45.0));
        if self.style != "minimal" {
            self.grille(ax, 118.0);
            self.paint(&rect(ax - 8.0, 168.0, 16.0, 4.5, 2.0, r##"fill="#000000" fill-opacity="0.5""##), (ax - 10.0, 166.0, ax + 10.0, 174.0), None);
        }
        self.round(ax, 146.0, 11.0, "ps", None, None);
        for (sx, keys) in [(142.0, ("lx", "ly", "l3")), (2.0 * ax - 142.0, ("rx", "ry", "r3"))] {
            self.stick(sx, 145.0, keys, 26.0, 21.0, Some((sx, 148.0, 34.0)), Some(8.0));
        }
        self.caption_at = Some((ax, 213.0));
    }

    fn build_compact(&mut self) {
        let f = self.fam;
        self.trigger(16.0, 8.0, 58.0, 13.0, "l2", f.l2, false);
        self.pill_key(16.0, 25.0, 58.0, 11.0, "l1", Some(f.l1), Some(5.5), Some(7.5), false);
        self.trigger(230.0, 8.0, 58.0, 13.0, "r2", f.r2, true);
        self.pill_key(230.0, 25.0, 58.0, 11.0, "r1", Some(f.r1), Some(5.5), Some(7.5), false);
        self.round(152.0, 30.0, 7.0, "ps", None, None);
        self.dpad(46.0, 80.0, 22.0, 14.0, 9.0, None);
        self.stick(120.0, 82.0, ("lx", "ly", "l3"), 21.0, 14.0, None, None);
        self.stick(184.0, 82.0, ("rx", "ry", "r3"), 21.0, 14.0, None, None);
        self.face(258.0, 80.0, 20.0, 10.0, None);
    }

    // --- keyboard & mouse ----------------------------------------------------------------
    fn kbm_size(&mut self) -> (f32, f32) {
        self.kb_keys = self.look.keys.iter().filter_map(|n| cfg::key_info(n)).collect();
        let mut rows: Vec<u8> = self.kb_keys.iter().map(|k| k.3).collect();
        rows.sort();
        rows.dedup();
        self.row_y = rows.iter().enumerate().map(|(i, r)| (*r, i)).collect();
        self.kx0 = self.kb_keys.iter().map(|k| k.4).fold(f32::INFINITY, f32::min);
        if !self.kx0.is_finite() {
            self.kx0 = 0.0;
        }
        let kw = self.kb_keys.iter().map(|k| k.4 + k.5).fold(0.0, f32::max) - self.kx0;
        let has = !self.kb_keys.is_empty();
        let kb_w = kw * KEY_UNIT - if has { KEY_GAP } else { 0.0 };
        let kb_h = rows.len() as f32 * KEY_UNIT - if has { KEY_GAP } else { 0.0 };
        self.kb_h = kb_h;
        let w = kb_w + if self.look.mouse { (if has { 22.0 } else { 0.0 }) + MOUSE_W } else { 0.0 };
        let h = kb_h.max(if self.look.mouse { MOUSE_H } else { 0.0 });
        (w.max(40.0) + 2.0 * KBM_M, h.max(30.0) + 2.0 * KBM_M)
    }

    fn build_kbm(&mut self) {
        let t = self.t;
        let top = KBM_M + (self.dh - 2.0 * KBM_M - self.kb_h); // keys sit on the bottom line
        for key in self.kb_keys.clone() {
            let (name, label, _, row, x, w) = *key;
            let kx = KBM_M + (x - self.kx0) * KEY_UNIT;
            let ky = top + self.row_y[&row] as f32 * KEY_UNIT;
            let (kw, kh) = (w * KEY_UNIT - KEY_GAP, KEY_UNIT - KEY_GAP);
            let bx = (kx - 2.0, ky - 2.0, kx + kw + 2.0, ky + kh + 2.0);
            let font = if label.chars().count() == 1 { 12.5 } else { 9.5 };
            let (cx, cy) = (kx + kw / 2.0, ky + kh / 2.0 - 0.5);
            self.paint(&(rect(kx, ky, kw, kh, 6.0, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#) + &rect(kx + 3.0, ky + 2.5, kw - 6.0, kh - 8.0, 4.0, r#"fill="url(#sheen)""#)), bx, None);
            self.label(label, cx, cy, t.glyph, font);
            let on = rect(kx, ky, kw, kh, 6.0, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##, t.accent));
            self.pressed(name, &on, bx, t.accent, Some((label, cx, cy, font)));
        }
        if self.look.mouse {
            self.mouse(self.dw - KBM_M - MOUSE_W, self.dh - KBM_M - MOUSE_H);
        }
    }

    fn mouse(&mut self, x: f32, y: f32) {
        let (t, k) = (self.t, self.k);
        let (cx, w, h) = (x + MOUSE_W / 2.0, MOUSE_W, MOUSE_H);
        let body = [
            format!("M{},{}", n(cx), n(y)),
            format!("C{},{} {},{} {},{}", n(x + w * 0.80), n(y), n(x + w), n(y + 17.0), n(x + w), n(y + 46.0)),
            format!("L{},{}", n(x + w), n(y + h - 40.0)),
            format!("C{},{} {},{} {},{}", n(x + w), n(y + h - 12.0), n(cx + w * 0.28), n(y + h), n(cx), n(y + h)),
            format!("C{},{} {},{} {},{}", n(cx - w * 0.28), n(y + h), n(x), n(y + h - 12.0), n(x), n(y + h - 40.0)),
            format!("L{},{}", n(x), n(y + 46.0)),
            format!("C{},{} {},{} {},{} Z", n(x), n(y + 17.0), n(x + w * 0.20), n(y), n(cx), n(y)),
        ]
        .join(" ");
        if self.style != "minimal" {
            self.paint(&path(&body, r#"fill="url(#body)" stroke="url(#rim)" stroke-width="1.4""#), (x - 3.0, y - 3.0, x + w + 3.0, y + h + 3.0), None);
        }
        let split = y + 54.0;
        let halves = [
            ("left", format!("M{},{} C{},{} {},{} {},{} L{},{} L{},{} Z", n(cx - 2.5), n(y + 3.5), n(x + 16.0), n(y + 4.0), n(x + 3.5), n(y + 18.0), n(x + 3.5), n(y + 46.0), n(x + 3.5), n(split), n(cx - 2.5), n(split))),
            ("right", format!("M{},{} C{},{} {},{} {},{} L{},{} L{},{} Z", n(cx + 2.5), n(y + 3.5), n(x + w - 16.0), n(y + 4.0), n(x + w - 3.5), n(y + 18.0), n(x + w - 3.5), n(y + 46.0), n(x + w - 3.5), n(split), n(cx + 2.5), n(split))),
        ];
        let bx = (x, y, x + w, split + 2.0);
        for (name, d) in halves {
            self.paint(&path(&d, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.1""#), bx, None);
            self.pressed(name, &path(&d, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##, t.accent)), bx, t.accent, None);
        }
        // the wheel: lights whole for a middle click, one half per scroll direction
        let (wx, wy, ww, wh) = (cx - 5.5, y + 13.0, 11.0, 28.0);
        let wbx = (wx - 2.0, wy - 2.0, wx + ww + 2.0, wy + wh + 2.0);
        let ridges: String = (0..4).map(|i| rect(wx + 2.5, wy + 5.0 + i as f32 * 4.5, ww - 5.0, 1.3, 0.6, r##"fill="#000000" fill-opacity="0.35""##)).collect();
        self.paint(&(rect(wx, wy, ww, wh, 5.5, r#"fill="url(#knob)" stroke="url(#caprim)" stroke-width="1.1""#) + &ridges), wbx, None);
        self.pressed("middle", &rect(wx, wy, ww, wh, 5.5, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.1""##, t.accent)), wbx, t.accent, None);
        for (name, (y0, y1)) in [("wheel_up", (wy, wy + wh / 2.0)), ("wheel_down", (wy + wh / 2.0, wy + wh))] {
            let (tip, base) = if name == "wheel_up" { (y0 + 4.0, y1 - 3.0) } else { (y1 - 4.0, y0 + 3.0) };
            let arrow = format!("M{},{} L{},{} L{},{} Z", n(cx), n(tip), n(cx + 3.5), n(base), n(cx - 3.5), n(base));
            let body = rect(wx, y0, ww, y1 - y0, 3.5, &format!(r#"fill="{}""#, t.accent)) + &path(&arrow, &format!(r#"fill="{}""#, t.glyph_on));
            self.pressed(name, &body, wbx, t.accent, None);
        }
        // side buttons on the left flank: forward (x2) above back (x1)
        for (name, sy) in [("x2", y + 64.0), ("x1", y + 82.0)] {
            let sbx = (x - 6.0, sy - 2.0, x + 6.0, sy + 17.0);
            self.paint(&rect(x - 3.5, sy, 7.0, 15.0, 3.5, r#"fill="url(#cap)" stroke="url(#caprim)" stroke-width="1.0""#), sbx, None);
            self.pressed(name, &rect(x - 3.5, sy, 7.0, 15.0, 3.5, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.5" stroke-width="1.0""##, t.accent)), sbx, t.accent, None);
        }
        // movement: a dot that swings the way the mouse is moving, by how fast
        let (rx, ry, rr) = (cx, y + 92.0, 21.0);
        let cross = path(&format!("M{},{} L{},{} M{},{} L{},{}", n(rx - rr + 5.0), n(ry), n(rx + rr - 5.0), n(ry), n(rx), n(ry - rr + 5.0), n(rx), n(ry + rr - 5.0)), r##"stroke="#ffffff" stroke-opacity="0.10" stroke-width="1""##);
        self.paint(&(circle(rx, ry, rr, r#"fill="url(#well)" stroke="url(#caprim)" stroke-width="1.0""#) + &cross), (rx - rr - 2.0, ry - rr - 2.0, rx + rr + 2.0, ry + rr + 2.0), None);
        let dot = 5.5;
        let dbx = (rx - dot - 2.0, ry - dot - 2.0, rx + dot + 2.0, ry + dot + 2.0);
        let idle = self.sprite(&circle(rx, ry, dot, &format!(r#"fill="{}" fill-opacity="0.5""#, t.glyph)), dbx);
        let live = self.sprite(&circle(rx, ry, dot, &format!(r##"fill="{}" stroke="#ffffff" stroke-opacity="0.6" stroke-width="1.0""##, t.accent)), dbx);
        let (g, gp) = halo(&live.img, t.accent, 4.0 * k, 0.9);
        let glow = Sprite { img: g, x: live.x - gp, y: live.y - gp };
        self.move_dot = Some(([vec![idle], vec![glow, live]], (rr - dot - 2.0) * k));
    }

    // --- per frame -------------------------------------------------------------------
    /// The battery pill under the pad: a battery icon and the percentage (a bolt while charging).
    fn caption_img(&self, r: &Reading) -> Img {
        let (t, k) = (self.t, self.k);
        let color = if r.low() { LOW_RED } else if r.charging { OK_GREEN } else { t.text };
        let pct = r.percent.map(|p| text_line(&format!("{p}%"), 15.0 * k, "bold", color));
        let (bw, bh, nub, gap) = (30.0 * k, 15.0 * k, 2.4 * k, 8.0 * k);
        let (padx, m) = (12.0 * k, 10.0 * k);
        let w = bw + nub + pct.as_ref().map_or(0.0, |p| gap + p.w() as f32) + 2.0 * padx;
        let h = bh.max(pct.as_ref().map_or(0.0, |p| p.h() as f32)) + 9.0 * k;
        let (x, cy) = (m + padx, m + h / 2.0);
        let mut body = rect(m, m, w, h, h / 2.0, &format!(r#"fill="{}" fill-opacity="0.85" stroke="url(#rim)" stroke-width="{}""#, t.panel, n(k.max(1.0))));
        body += &rect(x, cy - bh / 2.0, bw, bh, 3.2 * k, &format!(r#"fill="none" stroke="{color}" stroke-width="{}""#, n(1.6 * k)));
        body += &rect(x + bw + 0.8 * k, cy - bh / 4.0, nub, bh / 2.0, k, &format!(r#"fill="{color}""#));
        body += &rect(x + 2.8 * k, cy - bh / 2.0 + 2.8 * k, (2.0 * k).max((bw - 5.6 * k) * r.fill()), bh - 5.6 * k, 1.4 * k, &format!(r#"fill="{color}""#));
        if r.charging {
            body += &path(&bolt_path(x + bw / 2.0, cy, bh * 0.95, 1.3), &format!(r#"fill="{}""#, t.panel));
        }
        let (mut img, _) = raster::svg(&body, &self.defs, 1.0, (0.0, 0.0, w + 2.0 * m, h + 2.0 * m));
        if let Some(p) = pct {
            img.blit(&p, (x + bw + nub + gap).round() as i32, (cy - p.h() as f32 / 2.0).round() as i32);
        }
        img
    }

    /// The picture for this frame, or None when nothing visible changed.
    pub fn render(&mut self, f: &Frame, edit: bool) -> Option<&Img> {
        let key = match f {
            Frame::NoPad { note } => Key::Off(note.chars().take(26).collect(), edit),
            Frame::Pad { st, battery } => {
                let held: Vec<&'static str> = self.order.iter().copied().filter(|b| st.button(b)).collect();
                let trig: Vec<i32> = self.triggers.iter().map(|tr| (st.axis(tr.key) * (tr.x1 - tr.x0) as f32).round() as i32).collect();
                let sticks: Vec<(i32, i32, bool)> = self
                    .sticks
                    .iter()
                    .map(|s| {
                        let (mut x, mut y) = (st.axis(s.kx), st.axis(s.ky));
                        let m = x.hypot(y);
                        if m < STICK_DEADZONE {
                            x = 0.0;
                            y = 0.0;
                        } else if m > 1.0 {
                            x /= m;
                            y /= m;
                        }
                        ((x * s.travel).round() as i32, (y * s.travel).round() as i32, st.button(s.kb))
                    })
                    .collect();
                Key::Pad(held, trig, sticks, if self.caption_at.is_some() { *battery } else { None }, edit)
            }
            Frame::Kbm(st) => {
                let held: Vec<&'static str> = self.kb_keys.iter().map(|k| k.0).filter(|n| st.keys.contains(n)).collect();
                let mut mouse: Vec<&'static str> = Vec::new();
                let mut offset = (0, 0);
                if let Some((_, travel)) = &self.move_dot {
                    mouse = MOUSE_BUTTONS.iter().copied().filter(|b| st.buttons.contains(b)).collect();
                    if st.wheel != 0 {
                        mouse.push(if st.wheel > 0 { "wheel_up" } else { "wheel_down" });
                    }
                    let (vx, vy) = st.mv;
                    let m = vx.hypot(vy);
                    if m > 0.5 {
                        let reach = travel * (1.0 - (-m / 30.0).exp());
                        offset = ((vx / m * reach).round() as i32, (vy / m * reach).round() as i32);
                    }
                }
                Key::Kbm(held, mouse, offset, edit)
            }
        };
        if self.last.as_ref() == Some(&key) {
            return None;
        }
        self.last = Some(key.clone());
        self.frame = if edit { self.backdrop.clone() } else { self.base.clone() };
        match key {
            Key::Off(note, _) => {
                let t = self.t;
                let quoted = format!("“{note}” has no SDL mapping");
                let lines: Vec<(&str, f32, &str, &str)> = if note.is_empty() {
                    vec![("No controller", 13.0, "bold", t.text), ("Connect one by USB or Bluetooth", 9.5, "regular", t.dim)]
                } else {
                    vec![("Controller not recognised", 13.0, "bold", t.text), (quoted.as_str(), 9.5, "regular", t.dim)]
                };
                let msg = pill(&lines, t, self.k, None);
                self.frame.blit(&msg, (self.w - msg.w()) / 2, (self.h - msg.h()) / 2);
            }
            Key::Pad(held, trig, sticks, battery, _) => {
                for b in &held {
                    for s in &self.buttons[b] {
                        self.frame.blit(&s.img, s.x, s.y);
                    }
                }
                for (tr, px) in self.triggers.iter().zip(&trig) {
                    if *px >= 1 {
                        let x0 = if tr.rtl { tr.x1 - px } else { tr.x0 };
                        self.frame.blit_cols(&tr.fill.img, tr.fill.x, tr.fill.y, x0, x0 + px);
                        // the label back on top, two-tone: dark over the fill, light elsewhere
                        self.frame.blit(&tr.idle.img, tr.idle.x, tr.idle.y);
                        let fx0 = tr.fill.x + x0 - tr.on.x;
                        self.frame.blit_cols(&tr.on.img, tr.on.x, tr.on.y, fx0, fx0 + px);
                    }
                }
                for (s, (dx, dy, pressed)) in self.sticks.iter().zip(&sticks) {
                    let look = if *pressed { 2 } else if *dx != 0 || *dy != 0 { 1 } else { 0 };
                    for l in &s.looks[look] {
                        self.frame.blit(&l.img, s.pos.0 + dx + l.x, s.pos.1 + dy + l.y);
                    }
                }
                if let Some((cx, cy)) = self.caption_at {
                    if self.caption.as_ref().map(|c| c.0) != Some(battery) {
                        let img = battery.map(|r| self.caption_img(&r));
                        self.caption = Some((battery, img));
                    }
                    if let Some((_, Some(cap))) = &self.caption {
                        let (x, y) = ((cx * self.k - cap.w() as f32 / 2.0).round() as i32, (cy * self.k - cap.h() as f32 / 2.0).round() as i32);
                        self.frame.blit(cap, x, y);
                    }
                }
            }
            Key::Kbm(held, mouse, (dx, dy), _) => {
                for b in held.iter().chain(&mouse) {
                    if let Some(list) = self.buttons.get(b) {
                        for s in list {
                            self.frame.blit(&s.img, s.x, s.y);
                        }
                    }
                }
                if let Some((looks, _)) = &self.move_dot {
                    for s in &looks[if dx != 0 || dy != 0 { 1 } else { 0 }] {
                        self.frame.blit(&s.img, s.x + dx, s.y + dy);
                    }
                }
            }
        }
        Some(&self.frame)
    }

    /// Forget the last frame, so the next render draws again.
    pub fn invalidate(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(layout: &str, theme: &str, style: &str, family: &'static str) -> Look {
        Look { layout: layout.into(), theme: theme.into(), style: style.into(), k: 1.0, family, keys: vec!["w".into(), "a".into(), "space".into()], mouse: true }
    }

    #[test]
    fn every_look_builds() {
        for th in cfg::THEMES {
            for st in cfg::STYLES {
                for lay in ["controller", "compact", "kbm"] {
                    for fam in ["ps", "xbox"] {
                        let a = Art::new(look(lay, th, st, fam));
                        assert!(a.w > 30 && a.h > 30, "{lay} {th} {st} {fam}");
                    }
                }
            }
        }
    }

    #[test]
    fn controller_has_every_button_and_size() {
        let a = Art::new(look("controller", "ds4", "glass", "ps"));
        assert_eq!((a.w, a.h), (409, 272));
        for b in super::super::input::BUTTONS {
            if b != "l3" && b != "r3" {
                assert!(a.buttons.contains_key(b), "{b}");
            }
        }
        assert_eq!(a.triggers.len(), 2);
        assert_eq!(a.sticks.len(), 2);
    }

    #[test]
    fn same_input_draws_once() {
        let mut a = Art::new(look("compact", "mono", "solid", "xbox"));
        let st = PadState::default();
        let bat = Some(Reading { percent: Some(64), level: None, charging: false });
        assert!(a.render(&Frame::Pad { st: &st, battery: bat }, false).is_some());
        assert!(a.render(&Frame::Pad { st: &st, battery: bat }, false).is_none());
        let mut st2 = st.clone();
        st2.set("cross", true);
        assert!(a.render(&Frame::Pad { st: &st2, battery: bat }, false).is_some());
        assert!(a.render(&Frame::Pad { st: &st2, battery: bat }, true).is_some(), "edit mode redraws");
    }

    #[test]
    fn battery_pill_changes_the_picture() {
        let mut a = Art::new(look("controller", "ds4", "glass", "ps"));
        let st = PadState::default();
        let none = a.render(&Frame::Pad { st: &st, battery: None }, false).unwrap().0.data().to_vec();
        let with = a.render(&Frame::Pad { st: &st, battery: Some(Reading { percent: Some(12), level: None, charging: false }) }, false).unwrap().0.data().to_vec();
        assert_ne!(none, with);
    }

    #[test]
    fn kbm_size_follows_the_keys() {
        let mut l = look("kbm", "ds4", "glass", "ps");
        let small = Art::new(l.clone());
        l.keys = cfg::KEYS.iter().map(|k| k.0.to_string()).collect();
        let big = Art::new(l.clone());
        assert!(big.w > small.w && big.h >= small.h);
        l.mouse = false;
        l.keys.clear();
        let empty = Art::new(l);
        assert!(empty.w >= 40);
    }

    #[test]
    fn pill_wider_with_longer_text() {
        let t = theme("ds4");
        let a = pill(&[("Hi", 13.0, "semibold", t.text)], t, 1.0, None);
        let b = pill(&[("Controller disconnected", 13.0, "semibold", t.text)], t, 1.0, None);
        assert!(b.w() > a.w() + 40);
    }

    #[test]
    fn family_from_name() {
        assert_eq!(family_of("Wireless Controller"), "ps");
        assert_eq!(family_of("PS5 Controller"), "ps");
        assert_eq!(family_of("Xbox Series X Controller"), "xbox");
        assert_eq!(family_of("GameSir Nova 2 Lite"), "xbox");
    }

    #[test]
    fn numbers_are_short() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(0.125), "0.125");
        assert_eq!(n(-0.0001), "0");
        assert_eq!(n(204.5), "204.5");
    }
}

/// Writes previews of every layout to `DECK_ART_OUT` (a folder): `cargo test pad::art::previews -- --ignored`.
#[cfg(test)]
mod previews {
    use super::*;
    use crate::pad::input::KbmState;

    fn save(img: &Img, path: &std::path::Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.w() as u32, img.h() as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        // over a mid-grey "game" so glass and shadows show
        let mut bg = Img::new(img.w() as u32, img.h() as u32);
        bg.0.fill(resvg::tiny_skia::Color::from_rgba8(70, 90, 110, 255));
        bg.blit(img, 0, 0);
        enc.write_header().unwrap().write_image_data(bg.0.data()).unwrap();
    }

    #[test]
    #[ignore]
    fn previews() {
        let out = std::path::PathBuf::from(std::env::var("DECK_ART_OUT").expect("DECK_ART_OUT"));
        std::fs::create_dir_all(&out).unwrap();
        let bat = Some(Reading { percent: Some(64), level: None, charging: false });
        let mut st = PadState::default();
        for b in ["cross", "r1", "up"] {
            st.set(b, true);
        }
        st.set_axis("lx", 0.8);
        st.set_axis("ly", -0.5);
        st.set_axis("l2", 0.6);
        st.set_axis("r2", 1.0);
        for (lay, th, sty, fam) in [("controller", "ds4", "glass", "ps"), ("controller", "dualsense", "solid", "xbox"), ("controller", "gold", "minimal", "ps"), ("compact", "midnight", "glass", "ps"), ("compact", "mono", "solid", "xbox")] {
            let mut a = Art::new(Look { layout: lay.into(), theme: th.into(), style: sty.into(), k: 1.0, family: fam, keys: vec![], mouse: false });
            let img = a.render(&Frame::Pad { st: &PadState::default(), battery: bat }, false).unwrap().clone();
            save(&img, &out.join(format!("{lay}-{th}-{sty}-{fam}-idle.png")));
            let img = a.render(&Frame::Pad { st: &st, battery: Some(Reading { percent: None, level: None, charging: true }) }, false).unwrap().clone();
            save(&img, &out.join(format!("{lay}-{th}-{sty}-{fam}-pressed.png")));
        }
        let mut a = Art::new(Look { layout: "controller".into(), theme: "ds4".into(), style: "glass".into(), k: 1.0, family: "ps", keys: vec![], mouse: false });
        save(&a.render(&Frame::NoPad { note: "" }, true).unwrap().clone(), &out.join("nopad-edit.png"));
        let keys: Vec<String> = ["1", "2", "3", "4", "tab", "q", "w", "e", "r", "shift", "a", "s", "d", "f", "ctrl", "space"].iter().map(|s| s.to_string()).collect();
        let mut a = Art::new(Look { layout: "kbm".into(), theme: "ds4".into(), style: "glass".into(), k: 1.0, family: "ps", keys, mouse: true });
        let ks = KbmState { keys: vec!["w", "shift"], buttons: vec!["left"], wheel: 1, mv: (20.0, 5.0) };
        save(&a.render(&Frame::Kbm(&ks), false).unwrap().clone(), &out.join("kbm.png"));
        let t = theme("ds4");
        save(&pill(&[("Connected  ·  Wireless Controller", 13.0, "semibold", t.text)], t, 1.0, None), &out.join("toast.png"));
        for (name, r) in [("64", Reading { percent: Some(64), level: None, charging: false }), ("9", Reading { percent: Some(9), level: None, charging: false }), ("chg", Reading { percent: None, level: None, charging: true })] {
            let px = crate::pad::icon::battery(32, &r);
            let mut img = Img::new(32, 32);
            // straight → premultiplied for saving over the background
            for (d, s) in img.0.data_mut().chunks_mut(4).zip(px.chunks(4)) {
                let a = s[3] as u32;
                d[0] = (s[0] as u32 * a / 255) as u8;
                d[1] = (s[1] as u32 * a / 255) as u8;
                d[2] = (s[2] as u32 * a / 255) as u8;
                d[3] = s[3];
            }
            save(&img, &out.join(format!("tray-{name}.png")));
        }
    }
}
