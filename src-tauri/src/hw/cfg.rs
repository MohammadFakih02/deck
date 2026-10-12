//! What Deck's Fans / Lighting / Cooler display / Sensors pages edit, as one JSON-friendly
//! struct. `load` reads it from the hardware settings (MoboCtl's keys, so imported files work);
//! `apply` writes it back and hands it to the worker threads. Nothing reaches the hardware
//! until `apply` (the pages have an Apply button, like MoboCtl).

use super::fx::{self, Rgb};
use super::kv::Curve;
use super::lcd;
use super::{FanCfg, Shared};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering::Relaxed;

pub const CLOCK_BOTTOM: [&str; 5] = ["Date", "Seconds", "CPU temp", "GPU temp", "Off"];
pub const DMODES: [&str; 7] = ["Custom", "Rainbow", "Color cycle", "Breathing", "By temperature", "By load", "Same as lighting"];
pub const FIXED: [&str; 5] = ["Auto (BIOS)", "Fixed 30%", "Fixed 50%", "Fixed 75%", "Fixed 100%"];

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Lighting {
    pub mode: String,
    pub bright: i32,
    pub speed: i32,
    pub n1: i32,
    pub n2: i32,
    /// 12V, ARGB 1, ARGB 2, RAM
    pub zones: [bool; 4],
    pub color: String,
    pub music_style: String,
    pub music_rainbow: bool,
    pub music_gain: i32,
    pub game_sound: String,
    pub game_only: bool,
    pub auto_game: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self { mode: "Rainbow".into(), bright: 100, speed: 40, n1: 60, n2: 60, zones: [true; 4], color: "#009BDE".into(), music_style: "Level meter".into(), music_rainbow: true, music_gain: 100, game_sound: "Pulse".into(), game_only: true, auto_game: false }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Display {
    /// CPU °C, CPU %, GPU °C, GPU %
    pub show: [bool; 4],
    pub off: bool,
    pub fahrenheit: bool,
    pub color_mode: String,
    pub bright: i32,
    pub speed: i32,
    pub color: String,
    pub content: String,
    pub text_top: String,
    pub text_bottom: String,
    pub scroll: bool,
    pub clock24: bool,
    pub clock_bottom: String,
    /// 84 "#RRGGBB" colours (black = off)
    pub design: Vec<String>,
    pub anim: String,
    pub rotate: Vec<String>,
    pub rotate_secs: i32,
    pub night: bool,
    pub night_from: i32,
    pub night_to: i32,
    pub night_bright: i32,
    pub flash: bool,
    pub flash_at: i32,
    pub music: String,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            show: [true; 4],
            off: false,
            fahrenheit: false,
            color_mode: "By temperature".into(),
            bright: 100,
            speed: 40,
            color: "#009BDE".into(),
            content: "Sensors".into(),
            text_top: "HELLO".into(),
            text_bottom: String::new(),
            scroll: false,
            clock24: true,
            clock_bottom: "Date".into(),
            design: vec!["#000000".into(); lcd::LEDS],
            anim: "Spinner".into(),
            rotate: vec!["Sensors".into(), "Clock".into()],
            rotate_secs: 8,
            night: false,
            night_from: 23,
            night_to: 7,
            night_bright: 20,
            flash: false,
            flash_at: 90,
            music: "Bars".into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct FanCh {
    pub name: String,
    /// "Auto (BIOS)" | "Fixed 50%" | "Curve: <name>"
    pub mode: String,
    /// index into `sources`
    pub src: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Fans {
    pub channels: Vec<FanCh>,
    pub hyst: i32,
    pub delay: i32,
    pub stop: i32,
    pub allow_experimental: bool,
    /// name -> [(°C, %)]
    pub curves: Vec<(String, Vec<(f32, f32)>)>,
}

impl Default for Fans {
    fn default() -> Self {
        Self { channels: vec![], hyst: 2, delay: 3, stop: 0, allow_experimental: false, curves: vec![] }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Alerts {
    pub on: bool,
    pub cpu: i32,
    pub gpu: i32,
    pub log_csv: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Self { on: false, cpu: 90, gpu: 85, log_csv: false }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct HwCfg {
    pub lighting: Lighting,
    pub display: Display,
    pub fans: Fans,
    pub alerts: Alerts,
}

fn pick(list: &[&str], v: &str, def: &str) -> String {
    if list.contains(&v) { v } else { def }.to_string()
}

fn col(s: &str) -> Rgb {
    Rgb::parse(s).unwrap_or(Rgb::new(0, 155, 222))
}

/// Reads the applied configuration.
pub fn load(sh: &Shared) -> HwCfg {
    let s = &sh.s;
    let lighting = Lighting {
        mode: pick(&fx::MODES, &s.get("Mode", "Rainbow"), "Rainbow"),
        bright: s.int("Bright", 100).clamp(0, 100),
        speed: s.int("Speed", 40).clamp(1, 100),
        n1: s.int("N1", 60).clamp(1, 120),
        n2: s.int("N2", 60).clamp(1, 120),
        zones: [s.bool("Z0", true), s.bool("Z1", true), s.bool("Z2", true), s.bool("Z3", true)],
        color: s.col("Color", Rgb::new(0, 155, 222)).hex(),
        music_style: pick(&fx::MUSIC_STYLES, &s.get("L_MusicStyle", "Level meter"), "Level meter"),
        music_rainbow: s.get("L_MusicColors", "Rainbow") == "Rainbow",
        music_gain: s.int("L_MusicGain", 100).clamp(25, 400),
        game_sound: pick(&fx::GAME_SOUNDS, &s.get("G_Sound", "Pulse"), "Pulse"),
        game_only: s.bool("G_AudioOnly", true),
        auto_game: s.bool("L_AutoGame", false),
    };
    let design = lcd::design_from_string(&s.get("D_Design", ""));
    let display = Display {
        show: [s.bool("D_CpuT", true), s.bool("D_CpuL", true), s.bool("D_GpuT", true), s.bool("D_GpuL", true)],
        off: s.bool("D_Off", false),
        fahrenheit: s.bool("D_F", false),
        color_mode: pick(&DMODES, &s.get("D_Mode", "By temperature"), "By temperature"),
        bright: s.int("D_Bright", 100).clamp(0, 100),
        speed: s.int("D_Speed", 40).clamp(1, 100),
        color: s.col("D_Color", Rgb::new(0, 155, 222)).hex(),
        content: pick(&lcd::CONTENTS, &s.get("D_Content", "Sensors"), "Sensors"),
        text_top: s.get("D_TextTop", "HELLO"),
        text_bottom: s.get("D_TextBottom", ""),
        scroll: s.bool("D_Scroll", false),
        clock24: s.bool("D_Clock24", true),
        clock_bottom: pick(&CLOCK_BOTTOM, &s.get("D_ClockBottom", "Date"), "Date"),
        design: (0..lcd::LEDS).map(|i| design.get(i).copied().unwrap_or(Rgb::BLACK).hex()).collect(),
        anim: pick(&lcd::ANIMS, &s.get("D_Anim", "Spinner"), "Spinner"),
        rotate: s.get("D_Rotate", "Sensors,Clock").split(',').filter(|r| lcd::CONTENTS.contains(r)).map(String::from).collect(),
        rotate_secs: s.int("D_RotSecs", 8).clamp(2, 120),
        night: s.bool("D_Night", false),
        night_from: s.int("D_NightFrom", 23).clamp(0, 23),
        night_to: s.int("D_NightTo", 7).clamp(0, 23),
        night_bright: s.int("D_NightBright", 20).clamp(0, 100),
        flash: s.bool("D_Flash", false),
        flash_at: s.int("D_FlashAt", 90).clamp(50, 105),
        music: pick(&lcd::MUSIC_DISP, &s.get("D_MusicStyle", "Bars"), "Bars"),
    };
    let hw = sh.hw.lock().unwrap().clone();
    let curves: Vec<Curve> = sh.curves.lock().unwrap().clone();
    let modes = mode_items(&curves);
    let nsrc = sh.sources().len();
    let channels = hw
        .fan_names
        .iter()
        .map(|name| {
            let m = s.get(&format!("Ch:{name}"), "Auto (BIOS)");
            FanCh { name: name.clone(), mode: if modes.contains(&m) { m } else { "Auto (BIOS)".into() }, src: (s.int(&format!("Src:{name}"), 0).max(0) as usize).min(nsrc.saturating_sub(1)) }
        })
        .collect();
    let fans = Fans {
        channels,
        hyst: s.int("FanHyst", 2).clamp(0, 10),
        delay: s.int("FanDelay", 3).clamp(0, 30),
        stop: s.int("FanStop", 0).clamp(0, 70),
        allow_experimental: s.bool(&format!("FanExperimental:{}", hw.chip), false),
        curves: curves.iter().map(|c| (c.name.clone(), c.pts.clone())).collect(),
    };
    let alerts = Alerts { on: s.bool("AlertOn", false), cpu: s.int("AlertCpu", 90).clamp(60, 105), gpu: s.int("AlertGpu", 85).clamp(60, 100), log_csv: s.bool("LogCsv", false) };
    HwCfg { lighting, display, fans, alerts }
}

/// Every fan mode a channel can use with these curves.
pub fn mode_items(curves: &[Curve]) -> Vec<String> {
    let mut l: Vec<String> = FIXED.iter().map(|s| s.to_string()).collect();
    l.extend(curves.iter().map(|c| format!("Curve: {}", c.name)));
    l
}

pub fn effect_params(l: &Lighting) -> fx::EffectParams {
    fx::EffectParams {
        mode: l.mode.clone(),
        color: col(&l.color),
        bright: l.bright,
        speed: l.speed,
        ram: l.zones[3],
        music_style: l.music_style.clone(),
        music_rainbow: l.music_rainbow,
        music_gain: l.music_gain as f32 / 100.0,
        game_sound: l.game_sound.clone(),
        game_audio_only: l.game_only,
        auto_game: l.auto_game,
    }
}

pub fn disp_params(d: &Display) -> lcd::DisplayParams {
    lcd::DisplayParams {
        color_mode: d.color_mode.clone(),
        color: col(&d.color),
        bright: d.bright,
        speed: d.speed,
        fahrenheit: d.fahrenheit,
        cpu_temp: d.show[0],
        cpu_load: d.show[1],
        gpu_temp: d.show[2],
        gpu_load: d.show[3],
        off: d.off,
        content: d.content.clone(),
        text_top: d.text_top.clone(),
        text_bottom: d.text_bottom.clone(),
        scroll: d.scroll,
        clock24: d.clock24,
        clock_bottom: d.clock_bottom.clone(),
        design: (0..lcd::LEDS).map(|i| d.design.get(i).and_then(|c| Rgb::parse(c)).unwrap_or(Rgb::BLACK)).collect(),
        anim: d.anim.clone(),
        rotate: d.rotate.clone(),
        rotate_secs: d.rotate_secs,
        night: d.night,
        night_from: d.night_from,
        night_to: d.night_to,
        night_bright: d.night_bright,
        alert_flash: d.flash,
        alert_at: d.flash_at,
        music_style: d.music.clone(),
    }
}

/// Makes `c` live: settings file, curves file and the worker threads' snapshots.
pub fn apply(sh: &Shared, c: &HwCfg) {
    let s = &sh.s;
    let l = &c.lighting;
    let old_leds = (s.int("N1", 60), s.int("N2", 60));
    s.set("Mode", &pick(&fx::MODES, &l.mode, "Rainbow"));
    s.set_int("Bright", l.bright.clamp(0, 100));
    s.set_int("Speed", l.speed.clamp(1, 100));
    s.set_int("N1", l.n1.clamp(1, 120));
    s.set_int("N2", l.n2.clamp(1, 120));
    for i in 0..4 {
        s.set_bool(&format!("Z{i}"), l.zones[i]);
    }
    s.set_col("Color", col(&l.color));
    s.set("L_MusicStyle", &l.music_style);
    s.set("L_MusicColors", if l.music_rainbow { "Rainbow" } else { "My colour" });
    s.set_int("L_MusicGain", l.music_gain.clamp(25, 400));
    s.set("G_Sound", &l.game_sound);
    s.set_bool("G_AudioOnly", l.game_only);
    s.set_bool("L_AutoGame", l.auto_game);

    let d = &c.display;
    s.set_bool("D_CpuT", d.show[0]);
    s.set_bool("D_CpuL", d.show[1]);
    s.set_bool("D_GpuT", d.show[2]);
    s.set_bool("D_GpuL", d.show[3]);
    s.set_bool("D_Off", d.off);
    s.set_bool("D_F", d.fahrenheit);
    s.set("D_Mode", &d.color_mode);
    s.set_int("D_Bright", d.bright.clamp(0, 100));
    s.set_int("D_Speed", d.speed.clamp(1, 100));
    s.set_col("D_Color", col(&d.color));
    s.set("D_Content", &d.content);
    s.set("D_TextTop", &d.text_top);
    s.set("D_TextBottom", &d.text_bottom);
    s.set_bool("D_Scroll", d.scroll);
    s.set_bool("D_Clock24", d.clock24);
    s.set("D_ClockBottom", &d.clock_bottom);
    let dp = disp_params(d);
    s.set("D_Design", &lcd::design_to_string(&dp.design));
    s.set("D_Anim", &d.anim);
    s.set("D_Rotate", &d.rotate.join(","));
    s.set_int("D_RotSecs", d.rotate_secs.clamp(2, 120));
    s.set_bool("D_Night", d.night);
    s.set_int("D_NightFrom", d.night_from.clamp(0, 23));
    s.set_int("D_NightTo", d.night_to.clamp(0, 23));
    s.set_int("D_NightBright", d.night_bright.clamp(0, 100));
    s.set_bool("D_Flash", d.flash);
    s.set_int("D_FlashAt", d.flash_at.clamp(50, 105));
    s.set("D_MusicStyle", &d.music);

    // fan curves first: channel modes refer to them by name
    let f = &c.fans;
    let mut curves: Vec<Curve> = f.curves.iter().filter(|(n, p)| !n.trim().is_empty() && p.len() >= 2).map(|(n, p)| Curve { name: n.trim().to_string(), pts: p.clone() }).collect();
    if curves.is_empty() {
        curves = Curve::defaults();
    }
    Curve::save_all(&curves);
    let items = mode_items(&curves);
    let names = sh.hw.lock().unwrap().fan_names.clone();
    let chip = sh.hw.lock().unwrap().chip.clone();
    let mut modes = vec!["Auto (BIOS)".to_string(); names.len()];
    let mut src = vec![0usize; names.len()];
    for (i, name) in names.iter().enumerate() {
        if let Some(ch) = f.channels.iter().find(|x| &x.name == name) {
            modes[i] = if items.contains(&ch.mode) { ch.mode.clone() } else { "Auto (BIOS)".into() };
            src[i] = ch.src.min(sh.sources().len().saturating_sub(1));
        }
        s.set(&format!("Ch:{name}"), &modes[i]);
        s.set_int(&format!("Src:{name}"), src[i] as i32);
    }
    s.set_bool(&format!("FanExperimental:{chip}"), f.allow_experimental);
    s.set_int("FanHyst", f.hyst.clamp(0, 10));
    s.set_int("FanDelay", f.delay.clamp(0, 30));
    s.set_int("FanStop", f.stop.clamp(0, 70));

    let a = &c.alerts;
    s.set_bool("AlertOn", a.on);
    s.set_int("AlertCpu", a.cpu.clamp(60, 105));
    s.set_int("AlertGpu", a.gpu.clamp(60, 100));
    s.set_bool("LogCsv", a.log_csv);
    s.save();

    *sh.curves.lock().unwrap() = curves;
    *sh.modes.lock().unwrap() = modes;
    *sh.fancfg.lock().unwrap() = FanCfg { src, hyst: f.hyst as f32, delay: f.delay as f32, stop: f.stop as f32, allow_experimental: f.allow_experimental };
    *sh.ep.lock().unwrap() = effect_params(l);
    *sh.dp.lock().unwrap() = dp;
    sh.zmask.store((l.zones[0] as usize) | (l.zones[1] as usize) << 1 | (l.zones[2] as usize) << 2, Relaxed);
    sh.n1.store(l.n1.clamp(1, 120) as usize, Relaxed);
    sh.n2.store(l.n2.clamp(1, 120) as usize, Relaxed);
    sh.dirty.store(true, Relaxed);
    if old_leds != (l.n1, l.n2) {
        sh.reconnect.store(true, Relaxed);
    }
}

/// Lights off / on at once (remembered, mode kept).
pub fn set_lights_off(sh: &Shared, off: bool) {
    sh.lights_off.store(off, Relaxed);
    sh.dirty.store(true, Relaxed);
    sh.s.set_bool("L_Off", off);
    sh.s.save();
}

/// Every fan to `mode` at once (the Fans page's "All fans" and the tray), applied straight away.
pub fn set_all_fans(sh: &Shared, mode: &str) -> Result<(), String> {
    if !sh.fan_control_allowed() {
        return Err("Fan control isn't available on this PC.".into());
    }
    let mut c = load(sh);
    let curves: Vec<Curve> = c.fans.curves.iter().map(|(n, p)| Curve { name: n.clone(), pts: p.clone() }).collect();
    if !mode_items(&curves).iter().any(|m| m == mode) {
        return Err(format!("There is no fan mode called \"{mode}\"."));
    }
    for ch in &mut c.fans.channels {
        ch.mode = mode.to_string();
    }
    sh.s.set_bool("UserApplied", true);
    apply(sh, &c);
    Ok(())
}

/// The mode every fan is on, if they all share one (the tray ticks it); and the list to offer.
pub fn fan_modes(sh: &Shared) -> (Option<String>, Vec<String>) {
    let modes = sh.modes.lock().unwrap().clone();
    let common = modes.first().filter(|m| modes.iter().all(|x| x == *m)).cloned();
    (common, mode_items(&sh.curves.lock().unwrap()))
}

/// Cooler display off / on at once (remembered).
pub fn set_display_off(sh: &Shared, off: bool) {
    sh.dp.lock().unwrap().off = off;
    sh.s.set_bool("D_Off", off);
    sh.s.save();
}

/// Option lists the pages show.
pub fn lists() -> serde_json::Value {
    serde_json::json!({
        "modes": fx::MODES, "music_styles": fx::MUSIC_STYLES, "game_sounds": fx::GAME_SOUNDS,
        "contents": lcd::CONTENTS, "anims": lcd::ANIMS, "music_disp": lcd::MUSIC_DISP,
        "clock_bottom": CLOCK_BOTTOM, "dmodes": DMODES, "fixed": FIXED,
        "default_curves": Curve::defaults().into_iter().map(|c| (c.name, c.pts)).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_json() {
        let c = HwCfg::default();
        let j = serde_json::to_string(&c).unwrap();
        let back: HwCfg = serde_json::from_str(&j).unwrap();
        assert_eq!(back.lighting.mode, "Rainbow");
        assert_eq!(back.display.design.len(), lcd::LEDS);
        // missing sections fall back to defaults
        let partial: HwCfg = serde_json::from_str(r#"{"alerts":{"on":true}}"#).unwrap();
        assert!(partial.alerts.on);
        assert_eq!(partial.alerts.cpu, 90);
    }

    #[test]
    fn mode_items_lists_curves() {
        let items = mode_items(&Curve::defaults());
        assert_eq!(items[0], "Auto (BIOS)");
        assert!(items.contains(&"Curve: Silent".to_string()));
    }

    #[test]
    fn effect_params_follow_lighting() {
        let mut l = Lighting::default();
        l.color = "#FF0000".into();
        l.zones[3] = false;
        let e = effect_params(&l);
        assert_eq!(e.color, Rgb::new(255, 0, 0));
        assert!(!e.ram);
    }
}
