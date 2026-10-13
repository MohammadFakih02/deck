//! Colours and the shared effect maths.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    /// Parses "#RRGGBB" or one of the colour names the old .NET build wrote into settings.ini.
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim();
        if let Some(h) = s.strip_prefix('#') {
            if h.len() == 6 {
                let v = u32::from_str_radix(h, 16).ok()?;
                return Some(Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8));
            }
            return None;
        }
        Some(match s.to_ascii_lowercase().as_str() {
            "red" => Rgb::new(255, 0, 0),
            "orange" => Rgb::new(255, 165, 0),
            "yellow" => Rgb::new(255, 255, 0),
            "lime" => Rgb::new(0, 255, 0),
            "green" => Rgb::new(0, 128, 0),
            "cyan" | "aqua" => Rgb::new(0, 255, 255),
            "blue" => Rgb::new(0, 0, 255),
            "dodgerblue" => Rgb::new(30, 144, 255),
            "magenta" | "fuchsia" => Rgb::new(255, 0, 255),
            "white" => Rgb::new(255, 255, 255),
            "black" => Rgb::BLACK,
            "purple" => Rgb::new(128, 0, 128),
            "pink" => Rgb::new(255, 192, 203),
            "deepskyblue" => Rgb::new(0, 191, 255),
            "orangered" => Rgb::new(255, 69, 0),
            _ => return None,
        })
    }
}

#[derive(Clone)]
pub struct EffectParams {
    pub mode: String,
    pub color: Rgb,
    pub bright: i32,
    pub speed: i32,
    pub ram: bool,
    pub music_style: String,
    pub music_rainbow: bool, // false = use `color`
    pub music_gain: f32,     // sensitivity, 1.0 = normal
    pub game_sound: String,  // Game mode: how the lights react to the game's sound
    pub game_audio_only: bool, // react only to the game's own sound, not everything playing
    pub auto_game: bool,     // switch to Game lighting while a game is in front
    pub comet_colors: String, // Comet: "My colour", "Cycle" (the comet changes colour) or "Rainbow" (a rainbow tail)
}

impl Default for EffectParams {
    fn default() -> Self {
        EffectParams { mode: "Rainbow".into(), color: Rgb::new(255, 0, 0), bright: 100, speed: 40, ram: true, music_style: "Level meter".into(), music_rainbow: true, music_gain: 1.0, game_sound: "Pulse".into(), game_audio_only: true, auto_game: false, comet_colors: "My colour".into() }
    }
}

/// Music styles for the RGB lights.
/// Game mode sound reactions.
pub const GAME_SOUNDS: [&str; 5] = ["Off", "Pulse", "Impact", "Shockwave", "Bass glow"];

/// How the Comet effect is coloured.
pub const COMET_COLORS: [&str; 3] = ["My colour", "Cycle", "Rainbow"];

pub const MUSIC_STYLES: [&str; 6] = ["Level meter", "Pulse", "Beat flash", "Bass & treble", "Spectrum", "Centre burst"];

pub fn hsv(h: f64, s: f64, v: f64) -> Rgb {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Rgb::new(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

pub fn scale(c: Rgb, k: f64) -> Rgb {
    let k = k.clamp(0.0, 1.0);
    Rgb::new((c.r as f64 * k) as u8, (c.g as f64 * k) as u8, (c.b as f64 * k) as u8)
}

/// Live inputs some effects react to.
#[derive(Clone, Default)]
pub struct FxIn {
    pub temp: f32,           // CPU temperature, C
    pub audio: crate::hw::loopback::Levels, // music levels
    pub screen: Vec<Rgb>,    // screen colours, left to right (empty = no capture)
    pub screen_avg: Rgb,
}

/// Every lighting mode, in UI order.
pub const MODES: [&str; 11] = ["Static", "Breathing", "Rainbow", "Color Cycle", "Wave", "Comet", "Temperature", "Music", "Screen", "Game", "Off"];

/// Modes that need a different colour per LED (so RAM uses per-LED writes for them).
pub fn per_led(mode: &str) -> bool {
    matches!(mode, "Rainbow" | "Wave" | "Comet" | "Music" | "Screen" | "Game")
}

/// Modes that never change on their own, so unchanged frames need not be resent.
pub fn is_static(mode: &str) -> bool {
    matches!(mode, "Static" | "Off")
}

/// Temperature to colour: blue (cool) through green and yellow to red (hot).
pub fn temp_color(temp: f32) -> Rgb {
    let k = ((80.0 - temp as f64) / 50.0).clamp(0.0, 1.0); // 30C -> 1 (blue), 80C -> 0 (red)
    hsv(k * 220.0, 1.0, 1.0)
}

/// Music lighting. `pos` = 0..1 along the strip, n = LED count (1 = single-colour zone).
fn music_pixel(p: &EffectParams, t: f64, pos: f64, n: usize, inp: &FxIn, br: f64) -> Rgb {
    let g = p.music_gain as f64;
    let a = &inp.audio;
    let lv = |x: f32| (x as f64 * g).clamp(0.0, 1.0);
    let (level, bass, mid, treble, beat) = (lv(a.level), lv(a.bass), lv(a.mid), lv(a.treble), a.beat as f64);
    // colour of this LED: rainbow follows position/time/beats, otherwise the chosen colour
    let hue_of = |x: f64| if p.music_rainbow { hsv(x, 1.0, 1.0) } else { p.color };
    let single = n == 1;
    match p.music_style.as_str() {
        "Pulse" => scale(hue_of(t * 15.0 + pos * 120.0), br * (0.06 + 0.94 * level)),
        "Beat flash" => {
            // every beat jumps to a new colour and flashes, then fades
            let c = hue_of(a.beats as f64 * 47.0 + pos * 40.0);
            scale(c, br * (0.05 + 0.95 * beat.max(level * 0.35)))
        }
        "Bass & treble" => {
            if single {
                // red = bass, green = mids, blue = treble
                Rgb::new((255.0 * bass * br) as u8, (255.0 * mid * br) as u8, (255.0 * treble * br) as u8)
            } else if pos < 0.5 {
                scale(if p.music_rainbow { Rgb::new(255, 30, 0) } else { p.color }, br * (0.04 + 0.96 * bass))
            } else {
                scale(if p.music_rainbow { Rgb::new(0, 120, 255) } else { p.color }, br * (0.04 + 0.96 * treble))
            }
        }
        "Spectrum" => {
            // along the strip: bass -> mids -> treble, each part lit by its band
            let v = if pos < 0.33 { bass } else if pos < 0.66 { mid } else { treble };
            let v = if single { level } else { v };
            scale(hue_of(pos * 300.0), br * (0.04 + 0.96 * v))
        }
        "Centre burst" => {
            // light grows outwards from the middle with the level
            let d = (pos - 0.5).abs() * 2.0;
            let on = single || d <= level;
            scale(hue_of(t * 30.0 + d * 160.0), br * if on { 0.2 + 0.8 * level.max(beat) } else { 0.03 })
        }
        _ => {
            // Level meter: the strip fills with the music level, green to red (or the chosen colour)
            if single {
                scale(hue_of(t * 20.0), br * (0.08 + 0.92 * level))
            } else {
                let c = if p.music_rainbow { hsv(120.0 - 120.0 * pos, 1.0, 1.0) } else { p.color };
                scale(c, br * if pos <= level { 1.0 } else { 0.04 })
            }
        }
    }
}

pub fn pixel(p: &EffectParams, t: f64, i: usize, n: usize) -> Rgb {
    pixel_in(p, t, i, n, &FxIn::default())
}

pub fn pixel_in(p: &EffectParams, t: f64, i: usize, n: usize, inp: &FxIn) -> Rgb {
    let sp = p.speed as f64 / 100.0;
    let br = p.bright as f64 / 100.0;
    let n = n.max(1);
    let pos = i as f64 / n as f64;
    match p.mode.as_str() {
        "Static" => scale(p.color, br),
        "Breathing" => scale(p.color, br * ((t * sp * 4.0 - std::f64::consts::FRAC_PI_2).sin() + 1.0) / 2.0),
        "Rainbow" => scale(hsv(t * sp * 120.0 + 360.0 * pos, 1.0, 1.0), br),
        "Color Cycle" => scale(hsv(t * sp * 60.0, 1.0, 1.0), br),
        // a soft band of light travelling along the strip
        "Wave" => {
            let w = ((t * sp * 4.0 - pos * std::f64::consts::TAU * 2.0).sin() + 1.0) / 2.0;
            scale(p.color, br * (0.05 + 0.95 * w * w))
        }
        // a bright head with a fading tail running along the strip
        "Comet" => {
            let head = (t * sp * 0.8).fract();
            let d = (head - pos).rem_euclid(1.0); // distance behind the head
            let k = if d < 0.25 { 1.0 - d / 0.25 } else { 0.0 };
            let c = match p.comet_colors.as_str() {
                // the whole comet slowly runs through the colours
                "Cycle" => hsv(t * sp * 60.0, 1.0, 1.0),
                // head to tail spans most of the colour wheel, and the wheel turns as it flies
                "Rainbow" => hsv(t * sp * 60.0 + d / 0.25 * 270.0, 1.0, 1.0),
                _ => p.color,
            };
            scale(c, br * k * k)
        }
        "Temperature" => scale(temp_color(inp.temp), br),
        "Music" => music_pixel(p, t, pos, n, inp, br),
        // ambient: each LED shows the colour of its part of the screen, left to right
        "Screen" => {
            if n == 1 || inp.screen.is_empty() {
                scale(inp.screen_avg, br)
            } else {
                let k = ((pos * inp.screen.len() as f64) as usize).min(inp.screen.len() - 1);
                scale(inp.screen[k], br)
            }
        }
        // game scene colours, plus a reaction to the game's own sound
        "Game" => {
            let base = if n == 1 || inp.screen.is_empty() {
                inp.screen_avg
            } else {
                inp.screen[((pos * inp.screen.len() as f64) as usize).min(inp.screen.len() - 1)]
            };
            let a = &inp.audio;
            let g = p.music_gain as f64;
            let (lvl, imp) = ((a.game as f64 * g).clamp(0.0, 1.0), a.impact as f64);
            match p.game_sound.as_str() {
                "Pulse" => scale(base, br * (0.55 + 0.45 * lvl)),
                "Impact" => {
                    let w = Rgb::new(255, 235, 200);
                    let mix = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * imp * 0.85) as u8;
                    scale(Rgb::new(mix(base.r, w.r), mix(base.g, w.g), mix(base.b, w.b)), br)
                }
                "Shockwave" => {
                    // a bright ring runs out from the middle of the strip after each loud moment
                    let d = (pos - 0.5).abs() * 2.0;
                    let front = a.since_impact as f64 * 1.6;
                    let ring = if a.since_impact < 1.2 { (-((d - front) / 0.18).powi(2)).exp() * (1.0 - a.since_impact as f64 / 1.2) } else { 0.0 };
                    let ring = if n == 1 { imp } else { ring };
                    let mix = |x: u8| (x as f64 + (255.0 - x as f64) * ring) as u8;
                    scale(Rgb::new(mix(base.r), mix(base.g), mix(base.b)), br)
                }
                "Bass glow" => scale(base, br * (0.5 + 0.5 * (a.bass as f64 * g).clamp(0.0, 1.0))),
                _ => scale(base, br),
            }
        }
        _ => Rgb::BLACK,
    }
}

#[cfg(test)]
mod comet_tests {
    use super::*;

    fn comet(colors: &str, t: f64, i: usize) -> Rgb {
        let p = EffectParams { mode: "Comet".into(), color: Rgb::new(0, 0, 255), speed: 50, comet_colors: colors.into(), ..Default::default() };
        pixel(&p, t, i, 100)
    }

    #[test]
    fn comet_colours() {
        // speed 50: the head is at 0.4 * t along the strip; LED 40 is the head at t = 1
        assert_eq!(comet("My colour", 1.0, 40), Rgb::new(0, 0, 255));
        assert_ne!(comet("Cycle", 1.0, 40), comet("Cycle", 3.0, 80), "the whole comet changes colour over time");
        assert_ne!(comet("Rainbow", 1.0, 40), comet("Rainbow", 1.0, 30), "head and tail differ");
        assert_eq!(comet("Rainbow", 1.0, 70), Rgb::new(0, 0, 0), "ahead of the head is dark");
    }
}
