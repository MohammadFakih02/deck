//! What the Thermalright PA120 Digital shows: sensors, a clock, text, a painted design, animations, a music meter.
//!
//! The display is 84 LEDs: per row (top = CPU, bottom = GPU) a label LED pair, a 3-digit 7-segment number,
//! °C / °F symbols, a "1" (for 100) plus a 2-digit number and a % sign. Every function here only decides
//! which of those LEDs are on and in which colour.
use crate::hw::fx::*;

pub const LEDS: usize = 84;

/// Every "content" the display can show, in UI order.
pub const CONTENTS: [&str; 7] = ["Sensors", "Clock", "Text", "Design", "Animation", "Music meter", "Rotate"];
pub const ANIMS: [&str; 5] = ["Spinner", "Chase", "Sparkle", "Wave", "Snake"];
pub const MUSIC_DISP: [&str; 5] = ["Bars", "Spectrum", "Pulse", "Beat flash", "Levels"];

#[derive(Clone)]
pub struct DisplayParams {
    pub color_mode: String,
    pub color: Rgb,
    pub bright: i32,
    pub speed: i32,
    pub fahrenheit: bool,
    pub cpu_temp: bool,
    pub cpu_load: bool,
    pub gpu_temp: bool,
    pub gpu_load: bool,
    pub off: bool,
    // custom content
    pub content: String,
    pub text_top: String,
    pub text_bottom: String,
    pub scroll: bool,
    pub clock24: bool,
    pub clock_bottom: String, // "Date", "Seconds", "CPU temp", "GPU temp", "Off"
    pub design: Vec<Rgb>,     // 84 colours, black = off
    pub anim: String,
    pub rotate: Vec<String>,
    pub rotate_secs: i32,
    // extras
    pub night: bool,
    pub night_from: i32,
    pub night_to: i32,
    pub night_bright: i32,
    pub alert_flash: bool,
    pub alert_at: i32,
    pub music_style: String,
}

impl Default for DisplayParams {
    fn default() -> Self {
        DisplayParams {
            color_mode: "Custom".into(),
            color: Rgb::new(255, 0, 0),
            bright: 100,
            speed: 40,
            fahrenheit: false,
            cpu_temp: true,
            cpu_load: true,
            gpu_temp: true,
            gpu_load: true,
            off: false,
            content: "Sensors".into(),
            text_top: "HELLO".into(),
            text_bottom: "".into(),
            scroll: false,
            clock24: true,
            clock_bottom: "Date".into(),
            design: vec![Rgb::BLACK; LEDS],
            anim: "Spinner".into(),
            rotate: vec!["Sensors".into(), "Clock".into()],
            rotate_secs: 8,
            night: false,
            night_from: 23,
            night_to: 7,
            night_bright: 20,
            alert_flash: false,
            alert_at: 90,
            music_style: "Bars".into(),
        }
    }
}

/// Live values the display can show.
#[derive(Clone, Default)]
pub struct DispIn {
    pub cpu_t: Option<f32>,
    pub cpu_l: Option<f32>,
    pub gpu_t: Option<f32>,
    pub gpu_l: Option<f32>,
    pub audio: crate::hw::loopback::Levels,
    /// the RGB lighting's effect + inputs, for the "Same as lighting" colour mode (and music sensitivity)
    pub light: Option<(EffectParams, FxIn)>,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub day: u32,
    pub month: u32,
}

impl DispIn {
    pub fn now(mut self) -> DispIn {
        let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st) };
        self.hour = st.wHour as u32;
        self.min = st.wMinute as u32;
        self.sec = st.wSecond as u32;
        self.day = st.wDay as u32;
        self.month = st.wMonth as u32;
        self
    }
}

// ---------------------------------------------------------------- layout
// segment order inside a digit: left-top, top, right-top, middle, left-bottom, bottom, right-bottom
const LT: usize = 0;
const T: usize = 1;
const RT: usize = 2;
const M: usize = 3;
const LB: usize = 4;
const B: usize = 5;
const RB: usize = 6;

fn temp_group(row: usize) -> Vec<usize> {
    if row == 0 { (2..23).collect() } else { (61..82).rev().collect() }
}
fn use_group(row: usize) -> Vec<usize> {
    if row == 0 { (25..41).collect() } else { (43..59).rev().collect() }
}

/// LEDs of digit `pos` (0..=2 the big number, 3..=4 the small number) in `row` (0 = top/CPU, 1 = bottom/GPU).
pub fn digit(row: usize, pos: usize) -> [usize; 7] {
    let (g, base) = if pos < 3 { (temp_group(row), pos * 7) } else { (use_group(row), 2 + (pos - 3) * 7) };
    std::array::from_fn(|s| g[base + s])
}
/// The two LEDs that form the "1" in front of the small number (for 100%).
pub fn one(row: usize) -> [usize; 2] {
    let g = use_group(row);
    [g[0], g[1]]
}
pub fn label(row: usize) -> [usize; 2] {
    if row == 0 { [0, 1] } else { [82, 83] }
}
pub fn deg_c(row: usize) -> usize {
    if row == 0 { 23 } else { 59 }
}
pub fn deg_f(row: usize) -> usize {
    if row == 0 { 24 } else { 60 }
}
pub fn pct(row: usize) -> usize {
    if row == 0 { 41 } else { 42 }
}

/// Ring of outer segments, clockwise, used by the spinner / chase animations.
const RING: [usize; 6] = [T, RT, RB, B, LB, LT];

/// 7-segment shape of a character (None = can't be drawn, shown blank).
pub fn glyph(c: char) -> Option<[u8; 7]> {
    Some(match c {
        '0' | 'O' | 'D' => [1, 1, 1, 0, 1, 1, 1],
        '1' | 'I' => [0, 0, 1, 0, 0, 0, 1],
        '2' | 'Z' | 'z' => [0, 1, 1, 1, 1, 1, 0],
        '3' => [0, 1, 1, 1, 0, 1, 1],
        '4' => [1, 0, 1, 1, 0, 0, 1],
        '5' | 'S' | 's' => [1, 1, 0, 1, 0, 1, 1],
        '6' => [1, 1, 0, 1, 1, 1, 1],
        '7' => [0, 1, 1, 0, 0, 0, 1],
        '8' | 'B' => [1, 1, 1, 1, 1, 1, 1],
        '9' | 'g' => [1, 1, 1, 1, 0, 1, 1],
        'A' | 'a' => [1, 1, 1, 1, 1, 0, 1],
        'b' => [1, 0, 0, 1, 1, 1, 1],
        'C' => [1, 1, 0, 0, 1, 1, 0],
        'c' => [0, 0, 0, 1, 1, 1, 0],
        'd' => [0, 0, 1, 1, 1, 1, 1],
        'E' | 'e' => [1, 1, 0, 1, 1, 1, 0],
        'F' | 'f' => [1, 1, 0, 1, 1, 0, 0],
        'G' => [1, 1, 0, 0, 1, 1, 1],
        'H' | 'K' | 'k' | 'X' | 'x' => [1, 0, 1, 1, 1, 0, 1],
        'h' => [1, 0, 0, 1, 1, 0, 1],
        'i' => [0, 0, 0, 0, 0, 0, 1],
        'J' | 'j' => [0, 0, 1, 0, 1, 1, 1],
        'L' | 'l' => [1, 0, 0, 0, 1, 1, 0],
        'M' | 'm' => [1, 1, 1, 0, 1, 0, 1],
        'N' | 'n' => [0, 0, 0, 1, 1, 0, 1],
        'o' => [0, 0, 0, 1, 1, 1, 1],
        'P' | 'p' => [1, 1, 1, 1, 1, 0, 0],
        'Q' | 'q' => [1, 1, 1, 1, 0, 0, 1],
        'R' | 'r' => [0, 0, 0, 1, 1, 0, 0],
        'T' | 't' => [1, 0, 0, 1, 1, 1, 0],
        'U' | 'W' | 'w' => [1, 0, 1, 0, 1, 1, 1],
        'u' | 'v' | 'V' => [0, 0, 0, 0, 1, 1, 1],
        'Y' | 'y' => [1, 0, 1, 1, 0, 1, 1],
        '-' => [0, 0, 0, 1, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 1, 0],
        '=' => [0, 0, 0, 1, 0, 1, 0],
        '\'' => [0, 0, 1, 0, 0, 0, 0],
        '"' => [1, 0, 1, 0, 0, 0, 0],
        '[' => [1, 1, 0, 0, 1, 1, 0],
        ']' => [0, 1, 1, 0, 0, 1, 1],
        '?' => [0, 1, 1, 1, 1, 0, 0],
        '*' | '°' => [1, 1, 1, 1, 0, 0, 0],
        _ => return None,
    })
}
fn put(lit: &mut [Option<Rgb>; LEDS], leds: &[usize], col: Rgb) {
    for &l in leds {
        lit[l] = Some(col);
    }
}

fn put_char(lit: &mut [Option<Rgb>; LEDS], row: usize, pos: usize, c: char, col: Rgb) {
    if let Some(g) = glyph(c) {
        let d = digit(row, pos);
        for s in 0..7 {
            if g[s] == 1 {
                lit[d[s]] = Some(col);
            }
        }
    }
}

/// Right-aligned number in the big (3) or small (2) digit block; the small block gets its "1" for 100.
fn put_number(lit: &mut [Option<Rgb>; LEDS], row: usize, small: bool, v: i32, col: Rgb) {
    let v = v.max(0);
    let (first, count) = if small { (3, 2) } else { (0, 3) };
    let s = if small && v >= 100 {
        put(lit, &one(row), col);
        format!("{:02}", v % 100)
    } else {
        let s = v.to_string();
        if s.len() > count { s[s.len() - count..].to_string() } else { format!("{:>w$}", s, w = count) }
    };
    for (k, ch) in s.chars().enumerate() {
        if ch != ' ' {
            put_char(lit, row, first + k, ch, col);
        }
    }
}

/// Up to 5 characters across a row: 3 in the big block, 2 in the small one. Longer text scrolls when `scroll` is set.
/// `cycle` = length every scrolling row is padded to, so rows that both scroll move and wrap together.
#[allow(clippy::too_many_arguments)]
fn put_text(lit: &mut [Option<Rgb>; LEDS], row: usize, text: &str, scroll: bool, cycle: usize, t: f64, speed: i32, col: Rgb) {
    let chars: Vec<char> = text.chars().collect();
    let shown: Vec<char> = if scroll && chars.len() > 5 {
        let mut padded: Vec<char> = chars.clone();
        padded.resize(cycle.max(chars.len() + 5), ' ');
        let step = (t * (0.6 + speed as f64 / 25.0)) as usize % padded.len();
        (0..5).map(|k| padded[(step + k) % padded.len()]).collect()
    } else {
        chars.iter().copied().take(5).collect()
    };
    for (k, ch) in shown.into_iter().enumerate() {
        put_char(lit, row, k, ch, col);
    }
}

fn rand01(i: usize, slot: u64) -> f64 {
    // small hash, stable per LED and time slot (no RNG crate needed)
    let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ slot.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x % 10_000) as f64 / 10_000.0
}

fn thresh(v: f32) -> Rgb {
    if v < 30.0 {
        Rgb::new(0, 255, 255)
    } else if v < 50.0 {
        Rgb::new(0, 255, 0)
    } else if v < 70.0 {
        Rgb::new(255, 255, 0)
    } else if v < 90.0 {
        Rgb::new(255, 140, 0)
    } else {
        Rgb::new(255, 0, 0)
    }
}

/// Which content is showing right now (resolves "Rotate").
pub fn current_content(p: &DisplayParams, t: f64) -> String {
    if p.content == "Rotate" {
        let list: Vec<&String> = p.rotate.iter().filter(|c| CONTENTS.contains(&c.as_str()) && c.as_str() != "Rotate").collect();
        if list.is_empty() {
            return "Sensors".into();
        }
        let k = (t / p.rotate_secs.max(2) as f64) as usize % list.len();
        return list[k].clone();
    }
    p.content.clone()
}

/// Colour of each LED (None = off) for one frame.
pub fn frame(p: &DisplayParams, t: f64, inp: &DispIn) -> [Option<Rgb>; LEDS] {
    let mut lit: [Option<Rgb>; LEDS] = [None; LEDS];
    if p.off {
        return lit;
    }
    let base = p.color; // placeholder colour; real colouring happens below for non-design content
    let content = current_content(p, t);
    let sp = p.speed as f64 / 100.0;
    match content.as_str() {
        "Clock" => {
            let h = if p.clock24 { inp.hour } else { (inp.hour + 11) % 12 + 1 };
            put_number(&mut lit, 0, false, h as i32, base);
            put_char(&mut lit, 0, 3, char::from_digit(inp.min / 10, 10).unwrap_or('0'), base);
            put_char(&mut lit, 0, 4, char::from_digit(inp.min % 10, 10).unwrap_or('0'), base);
            if inp.sec.is_multiple_of(2) {
                lit[deg_c(0)] = Some(base); // blinking separator
            }
            match p.clock_bottom.as_str() {
                "Date" => {
                    put_number(&mut lit, 1, false, inp.day as i32, base);
                    put_char(&mut lit, 1, 3, char::from_digit(inp.month / 10, 10).unwrap_or('0'), base);
                    put_char(&mut lit, 1, 4, char::from_digit(inp.month % 10, 10).unwrap_or('0'), base);
                }
                "Seconds" => {
                    put_char(&mut lit, 1, 3, char::from_digit(inp.sec / 10, 10).unwrap_or('0'), base);
                    put_char(&mut lit, 1, 4, char::from_digit(inp.sec % 10, 10).unwrap_or('0'), base);
                }
                "CPU temp" | "GPU temp" => {
                    let v = if p.clock_bottom == "CPU temp" { inp.cpu_t } else { inp.gpu_t };
                    if let Some(v) = v {
                        put_number(&mut lit, 1, false, (if p.fahrenheit { v * 9.0 / 5.0 + 32.0 } else { v }).round() as i32, base);
                        lit[if p.fahrenheit { deg_f(1) } else { deg_c(1) }] = Some(base);
                    }
                }
                _ => {}
            }
        }
        "Text" => {
            let cycle = [&p.text_top, &p.text_bottom].iter().map(|s| s.chars().count()).filter(|n| *n > 5).max().unwrap_or(0) + 5;
            put_text(&mut lit, 0, &p.text_top, p.scroll, cycle, t, p.speed, base);
            put_text(&mut lit, 1, &p.text_bottom, p.scroll, cycle, t, p.speed, base);
        }
        "Design" => {
            for i in 0..LEDS {
                let c = p.design.get(i).copied().unwrap_or(Rgb::BLACK);
                if c != Rgb::BLACK {
                    lit[i] = Some(c);
                }
            }
        }
        "Animation" => {
            let digits: Vec<[usize; 7]> = (0..2).flat_map(|r| (0..5).map(move |d| digit(r, d))).collect();
            match p.anim.as_str() {
                "Spinner" => {
                    // every digit spins a lit segment around its outer ring
                    let step = (t * (2.0 + sp * 18.0)) as usize;
                    for (k, d) in digits.iter().enumerate() {
                        lit[d[RING[(step + k) % 6]]] = Some(base);
                        lit[d[RING[(step + k + 5) % 6]]] = Some(base);
                    }
                }
                "Chase" => {
                    // one segment runs along the top edges, then back along the bottom edges
                    let path: Vec<usize> = (0..5).map(|d| digit(0, d)[T]).chain((0..5).rev().map(|d| digit(1, d)[B])).collect();
                    let step = (t * (2.0 + sp * 14.0)) as usize % path.len();
                    for tail in 0..3 {
                        lit[path[(step + path.len() - tail) % path.len()]] = Some(base);
                    }
                }
                "Sparkle" => {
                    let slot = (t * (2.0 + sp * 10.0)) as u64;
                    for i in 0..LEDS {
                        if rand01(i, slot) < 0.18 {
                            lit[i] = Some(base);
                        }
                    }
                }
                "Wave" => {
                    for d in &digits {
                        put(&mut lit, d, base);
                    }
                }
                _ => {
                    // Snake: a growing and shrinking path through the middle bars
                    let path: Vec<usize> = (0..5).map(|d| digit(0, d)[M]).chain((0..5).rev().map(|d| digit(1, d)[M])).collect();
                    let len = 1 + ((t * (1.0 + sp * 3.0)).sin().abs() * (path.len() - 1) as f64) as usize;
                    let start = (t * (2.0 + sp * 8.0)) as usize;
                    for k in 0..len {
                        lit[path[(start + k) % path.len()]] = Some(base);
                    }
                }
            }
        }
        "Music meter" => {
            let g = inp.light.as_ref().map(|l| l.0.music_gain as f64).unwrap_or(1.0);
            let a = &inp.audio;
            let lv = |x: f32| (x as f64 * g).clamp(0.0, 1.0);
            // a digit as a level bar: bottom, lower sides, middle, upper sides, top
            let bar_steps: [&[usize]; 5] = [&[B], &[LB, RB], &[M], &[LT, RT], &[T]];
            let bar = |lit: &mut [Option<Rgb>; LEDS], r: usize, d: usize, v: f64| {
                let n = (v * 5.0).round() as usize;
                let dg = digit(r, d);
                for st in bar_steps.iter().take(n) {
                    for s in st.iter() {
                        lit[dg[*s]] = Some(base);
                    }
                }
            };
            match p.music_style.as_str() {
                "Spectrum" => {
                    // left to right: bass, low mids, mids, high mids, treble
                    let bands = [lv(a.bass), (lv(a.bass) + lv(a.mid)) / 2.0, lv(a.mid), (lv(a.mid) + lv(a.treble)) / 2.0, lv(a.treble)];
                    for d in 0..5 {
                        for r in 0..2 {
                            bar(&mut lit, r, d, bands[d]);
                        }
                    }
                }
                "Pulse" | "Beat flash" => {
                    // everything lit; the brightness does the moving (see colouring below)
                    for r in 0..2 {
                        for d in 0..5 {
                            put(&mut lit, &digit(r, d), base);
                        }
                    }
                }
                "Levels" => {
                    // numbers: level / bass on top, mids / treble below
                    put_number(&mut lit, 0, false, (lv(a.level) * 99.0) as i32, base);
                    put_number(&mut lit, 0, true, (lv(a.bass) * 99.0) as i32, base);
                    put_number(&mut lit, 1, false, (lv(a.mid) * 99.0) as i32, base);
                    put_number(&mut lit, 1, true, (lv(a.treble) * 99.0) as i32, base);
                }
                _ => {
                    // Bars: every digit a level bar, with a little variation so it looks alive
                    for d in 0..5 {
                        for r in 0..2 {
                            let v = (lv(a.level) * (0.75 + 0.5 * rand01(d * 2 + r, (t * 8.0) as u64))).min(1.0);
                            bar(&mut lit, r, d, v);
                        }
                    }
                }
            }
        }
        _ => {
            // Sensors (the original view)
            let tf = |v: Option<f32>| v.map(|v| (if p.fahrenheit { v * 9.0 / 5.0 + 32.0 } else { v }).round() as i32);
            for (row, temp, load, on_t, on_l) in [(0, inp.cpu_t, inp.cpu_l, p.cpu_temp, p.cpu_load), (1, inp.gpu_t, inp.gpu_l, p.gpu_temp, p.gpu_load)] {
                if on_t || on_l {
                    put(&mut lit, &label(row), base);
                }
                if on_t {
                    lit[if p.fahrenheit { deg_f(row) } else { deg_c(row) }] = Some(base);
                    if let Some(v) = tf(temp) {
                        put_number(&mut lit, row, false, v, base);
                    }
                }
                if on_l {
                    lit[pct(row)] = Some(base);
                    if let Some(v) = load {
                        put_number(&mut lit, row, true, v.round() as i32, base);
                    }
                }
            }
        }
    }

    // colouring (designs keep their painted colours)
    let mut br = p.bright as f64 / 100.0;
    if p.night {
        let h = inp.hour as i32;
        let in_night = if p.night_from <= p.night_to { h >= p.night_from && h < p.night_to } else { h >= p.night_from || h < p.night_to };
        if in_night {
            br *= p.night_bright as f64 / 100.0;
        }
    }
    // music brightness for the pulse / flash meters
    if content == "Music meter" {
        let g = inp.light.as_ref().map(|l| l.0.music_gain as f64).unwrap_or(1.0);
        match p.music_style.as_str() {
            "Pulse" => br *= 0.06 + 0.94 * (inp.audio.level as f64 * g).clamp(0.0, 1.0),
            "Beat flash" => br *= 0.04 + 0.96 * (inp.audio.beat as f64).max(inp.audio.level as f64 * g * 0.3).clamp(0.0, 1.0),
            _ => {}
        }
    }
    let alert = p.alert_flash && inp.cpu_t.map(|v| v >= p.alert_at as f32).unwrap_or(false) && (t * 2.0) as i64 % 2 == 0;
    let ep = |mode: &str| EffectParams { mode: mode.into(), color: p.color, bright: 100, speed: p.speed, ..Default::default() };
    for i in 0..LEDS {
        let Some(c0) = lit[i] else { continue };
        let row_cpu = i < 42;
        let c = if alert {
            Rgb::new(255, 0, 0)
        } else if content == "Design" {
            c0
        } else if content == "Animation" && p.anim == "Wave" {
            let x = i as f64 / LEDS as f64;
            scale(p.color, 0.08 + 0.92 * (((t * (1.0 + sp * 6.0) - x * std::f64::consts::TAU * 2.0).sin() + 1.0) / 2.0))
        } else if p.color_mode == "Same as lighting" {
            same_as_lighting(i, t, inp).unwrap_or(p.color)
        } else if content == "Music meter" && matches!(p.color_mode.as_str(), "By temperature" | "By load") {
            // green (quiet) to red (loud), or a new colour on every beat for the flash style
            if p.music_style == "Beat flash" {
                hsv(inp.audio.beats as f64 * 47.0, 1.0, 1.0)
            } else {
                hsv(120.0 - 120.0 * inp.audio.level as f64, 1.0, 1.0)
            }
        } else {
            match p.color_mode.as_str() {
                "By temperature" => thresh((if row_cpu { inp.cpu_t } else { inp.gpu_t }).unwrap_or(0.0)),
                "By load" => thresh((if row_cpu { inp.cpu_l } else { inp.gpu_l }).unwrap_or(0.0)),
                "Rainbow" => pixel(&ep("Rainbow"), t, i, LEDS),
                "Color cycle" => pixel(&ep("Color Cycle"), t, i, LEDS),
                "Breathing" => pixel(&ep("Breathing"), t, i, LEDS),
                _ => p.color,
            }
        };
        lit[i] = if br <= 0.0 { None } else { Some(scale(c, br)) };
    }
    lit
}

/// Colour the RGB lighting would give this LED, using its left-to-right position on the display,
/// so effects like Rainbow, Wave or Music flow across the lights and the display alike.
fn same_as_lighting(led: usize, t: f64, inp: &DispIn) -> Option<Rgb> {
    let (ep, fxin) = inp.light.as_ref()?;
    if ep.mode == "Off" {
        return None;
    }
    static XS: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let xs = XS.get_or_init(|| {
        let mut v = vec![0f32; LEDS];
        for (l, r) in geometry() {
            v[l] = r[0] + r[2] / 2.0;
        }
        v
    });
    let pos = ((xs[led] / 280.0) * 99.0).clamp(0.0, 99.0) as usize;
    let mut full = ep.clone();
    full.bright = 100; // the display applies its own brightness
    Some(pixel_in(&full, t, pos, 100, fxin))
}

/// The 84x3 byte buffer the device expects.
pub fn bytes(lit: &[Option<Rgb>; LEDS]) -> Vec<u8> {
    let mut data = vec![0u8; LEDS * 3];
    for (i, c) in lit.iter().enumerate() {
        if let Some(c) = c {
            data[i * 3] = c.r;
            data[i * 3 + 1] = c.g;
            data[i * 3 + 2] = c.b;
        }
    }
    data
}

/// Design stored in settings as 84 "RRGGBB" values joined by commas ("" = all off).
pub fn design_to_string(d: &[Rgb]) -> String {
    d.iter().map(|c| format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)).collect::<Vec<_>>().join(",")
}
pub fn design_from_string(s: &str) -> Vec<Rgb> {
    let mut v: Vec<Rgb> = s.split(',').map(|h| Rgb::parse(&format!("#{}", h.trim())).unwrap_or(Rgb::BLACK)).collect();
    v.resize(LEDS, Rgb::BLACK);
    v
}

// ---------------------------------------------------------------- geometry for the on-screen preview / designer
/// Rectangles (x, y, w, h) of every LED in a 330 x 120 drawing of the display.
pub fn geometry() -> Vec<(usize, [f32; 4])> {
    let mut g = vec![];
    for row in 0..2 {
        let y0 = 8.0 + row as f32 * 58.0;
        // label: two LEDs drawn as a pill
        let l = label(row);
        g.push((l[0], [4.0, y0 + 10.0, 30.0, 10.0]));
        g.push((l[1], [4.0, y0 + 24.0, 30.0, 10.0]));
        let seg = |x: f32| -> [[f32; 4]; 7] {
            [
                [x, y0 + 3.0, 4.0, 18.0],         // left-top
                [x + 3.0, y0, 18.0, 4.0],         // top
                [x + 20.0, y0 + 3.0, 4.0, 18.0],  // right-top
                [x + 3.0, y0 + 20.0, 18.0, 4.0],  // middle
                [x, y0 + 23.0, 4.0, 18.0],        // left-bottom
                [x + 3.0, y0 + 40.0, 18.0, 4.0],  // bottom
                [x + 20.0, y0 + 23.0, 4.0, 18.0], // right-bottom
            ]
        };
        for (pos, x) in [(0, 44.0), (1, 76.0), (2, 108.0)] {
            let d = digit(row, pos);
            for (s, r) in seg(x).iter().enumerate() {
                g.push((d[s], *r));
            }
        }
        g.push((deg_c(row), [140.0, y0 + 2.0, 18.0, 14.0]));
        g.push((deg_f(row), [140.0, y0 + 26.0, 18.0, 14.0]));
        let o = one(row);
        g.push((o[0], [176.0, y0 + 3.0, 4.0, 18.0]));
        g.push((o[1], [176.0, y0 + 23.0, 4.0, 18.0]));
        for (pos, x) in [(3, 188.0), (4, 220.0)] {
            let d = digit(row, pos);
            for (s, r) in seg(x).iter().enumerate() {
                g.push((d[s], *r));
            }
        }
        g.push((pct(row), [254.0, y0 + 14.0, 16.0, 16.0]));
    }
    g
}
