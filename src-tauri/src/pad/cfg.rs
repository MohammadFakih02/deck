//! The controller overlay's settings: the `pad` object in settings.json. The keys and their
//! checks are the standalone Controller Overlay's (`Settings.CHECKS`), plus Deck's own switches.
//! Reading is field by field: a bad value falls back to its default and the rest is kept.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

pub const THEMES: [&str; 5] = ["ds4", "midnight", "dualsense", "gold", "mono"];
pub const STYLES: [&str; 3] = ["glass", "solid", "minimal"];
pub const LAYOUTS: [&str; 2] = ["controller", "compact"];
pub const LABELS: [&str; 3] = ["playstation", "xbox", "auto"];
pub const MODES: [&str; 2] = ["controller", "kbm"];

/// The left-hand block of a keyboard: (name, label, virtual-key code, row, x and width in key
/// units). Staggered like a real keyboard so any selection still looks like the corner of one.
pub const KEYS: [(&str, &str, u16, u8, f32, f32); 27] = [
    ("esc", "Esc", 0x1B, 0, 0.0, 1.0),
    ("1", "1", 0x31, 0, 1.0, 1.0),
    ("2", "2", 0x32, 0, 2.0, 1.0),
    ("3", "3", 0x33, 0, 3.0, 1.0),
    ("4", "4", 0x34, 0, 4.0, 1.0),
    ("5", "5", 0x35, 0, 5.0, 1.0),
    ("tab", "Tab", 0x09, 1, 0.0, 1.5),
    ("q", "Q", 0x51, 1, 1.5, 1.0),
    ("w", "W", 0x57, 1, 2.5, 1.0),
    ("e", "E", 0x45, 1, 3.5, 1.0),
    ("r", "R", 0x52, 1, 4.5, 1.0),
    ("t", "T", 0x54, 1, 5.5, 1.0),
    ("caps", "Caps", 0x14, 2, 0.0, 1.75),
    ("a", "A", 0x41, 2, 1.75, 1.0),
    ("s", "S", 0x53, 2, 2.75, 1.0),
    ("d", "D", 0x44, 2, 3.75, 1.0),
    ("f", "F", 0x46, 2, 4.75, 1.0),
    ("g", "G", 0x47, 2, 5.75, 1.0),
    ("shift", "Shift", 0x10, 3, 0.0, 2.25),
    ("z", "Z", 0x5A, 3, 2.25, 1.0),
    ("x", "X", 0x58, 3, 3.25, 1.0),
    ("c", "C", 0x43, 3, 4.25, 1.0),
    ("v", "V", 0x56, 3, 5.25, 1.0),
    ("b", "B", 0x42, 3, 6.25, 1.0),
    ("ctrl", "Ctrl", 0x11, 4, 0.0, 1.5),
    ("alt", "Alt", 0x12, 4, 1.5, 1.25),
    ("space", "Space", 0x20, 4, 2.75, 4.5),
];

pub fn key_info(name: &str) -> Option<&'static (&'static str, &'static str, u16, u8, f32, f32)> {
    KEYS.iter().find(|k| k.0 == name)
}

const DEFAULT_KEYS: [&str; 16] = ["1", "2", "3", "4", "tab", "q", "w", "e", "r", "shift", "a", "s", "d", "f", "ctrl", "space"];

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PadSettings {
    /// the overlay is shown (off for new users; on after importing the old app's settings)
    pub on: bool,
    pub anchor_h: String,
    pub anchor_v: String,
    pub off_x: f64,
    pub off_y: f64,
    /// "auto" (the monitor the game is on) or a monitor index, leftmost first
    pub monitor: Value,
    pub scale: f64,
    pub opacity: f64,
    pub theme: String,
    pub style: String,
    pub layout: String,
    pub labels: String,
    pub fade: bool,
    pub hide_no_pad: bool,
    pub mode: String,
    pub keys: Vec<String>,
    pub mouse: bool,
    /// keep the overlay out of screenshots and recordings (WDA_EXCLUDEFROMCAPTURE)
    pub capture_hidden: bool,
    /// one tray icon per controller that reports its battery
    pub battery_tray: bool,
    /// a notification once per discharge when a controller drops to 15%
    pub battery_warn: bool,
}

impl Default for PadSettings {
    fn default() -> Self {
        Self {
            on: false,
            anchor_h: "right".into(),
            anchor_v: "bottom".into(),
            off_x: 24.0,
            off_y: 24.0,
            monitor: json!("auto"),
            scale: 1.0,
            opacity: 0.88,
            theme: "ds4".into(),
            style: "glass".into(),
            layout: "controller".into(),
            labels: "playstation".into(),
            fade: true,
            hide_no_pad: true,
            mode: "controller".into(),
            keys: DEFAULT_KEYS.iter().map(|s| s.to_string()).collect(),
            mouse: true,
            capture_hidden: true,
            battery_tray: true,
            battery_warn: true,
        }
    }
}

fn num(v: Option<&Value>, lo: f64, hi: f64) -> Option<f64> {
    v.filter(|v| !v.is_boolean()).and_then(Value::as_f64).filter(|n| (lo..=hi).contains(n))
}
fn pick(v: Option<&Value>, list: &[&str]) -> Option<String> {
    v.and_then(Value::as_str).filter(|s| list.contains(s)).map(str::to_string)
}

impl PadSettings {
    /// Each field from `v` when it passes its check, else the default.
    pub fn from_value(v: &Value) -> Self {
        let d = Self::default();
        let g = |k: &str| v.get(k);
        let b = |k: &str, def: bool| g(k).and_then(Value::as_bool).unwrap_or(def);
        let monitor = match g("monitor") {
            Some(Value::String(s)) if s == "auto" => json!("auto"),
            Some(m) => match m.as_f64() {
                Some(n) if (0.0..=15.0).contains(&n) && n.fract() == 0.0 => json!(n as u64),
                _ => d.monitor.clone(),
            },
            None => d.monitor.clone(),
        };
        let keys = match g("keys").and_then(Value::as_array) {
            Some(a) if a.iter().all(|k| k.as_str().is_some_and(|k| key_info(k).is_some())) => a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect(),
            _ => d.keys.clone(),
        };
        Self {
            on: b("on", d.on),
            anchor_h: pick(g("anchor_h"), &["left", "center", "right"]).unwrap_or(d.anchor_h),
            anchor_v: pick(g("anchor_v"), &["top", "bottom"]).unwrap_or(d.anchor_v),
            off_x: num(g("off_x"), -5000.0, 5000.0).unwrap_or(d.off_x),
            off_y: num(g("off_y"), -5000.0, 5000.0).unwrap_or(d.off_y),
            monitor,
            scale: num(g("scale"), 0.5, 3.0).unwrap_or(d.scale),
            opacity: num(g("opacity"), 0.15, 1.0).unwrap_or(d.opacity),
            theme: pick(g("theme"), &THEMES).unwrap_or(d.theme),
            style: pick(g("style"), &STYLES).unwrap_or(d.style),
            layout: pick(g("layout"), &LAYOUTS).unwrap_or(d.layout),
            labels: pick(g("labels"), &LABELS).unwrap_or(d.labels),
            fade: b("fade", d.fade),
            hide_no_pad: b("hide_no_pad", d.hide_no_pad),
            mode: pick(g("mode"), &MODES).unwrap_or(d.mode),
            keys,
            mouse: b("mouse", d.mouse),
            capture_hidden: b("capture_hidden", d.capture_hidden),
            battery_tray: b("battery_tray", d.battery_tray),
            battery_warn: b("battery_warn", d.battery_warn),
        }
    }

    /// The fixed monitor index, or None for "auto".
    pub fn monitor_index(&self) -> Option<usize> {
        self.monitor.as_u64().map(|n| n as usize)
    }
}

impl<'de> Deserialize<'de> for PadSettings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(&Value::deserialize(d)?))
    }
}

/// The standalone app's settings.json: its keys are the same; the overlay comes on.
pub fn from_old_app(v: &Value) -> PadSettings {
    let mut s = PadSettings::from_value(v);
    s.on = true;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_fields_fall_back() {
        let v = json!({ "scale": "big", "opacity": 0.5, "theme": "neon", "style": "solid", "monitor": 2.5,
                        "keys": ["w", "nope"], "off_x": 9999, "fade": "yes", "mode": "kbm", "on": true });
        let s = PadSettings::from_value(&v);
        let d = PadSettings::default();
        assert_eq!(s.scale, d.scale);
        assert_eq!(s.opacity, 0.5);
        assert_eq!(s.theme, "ds4");
        assert_eq!(s.style, "solid");
        assert_eq!(s.monitor, json!("auto"));
        assert_eq!(s.keys, d.keys);
        assert_eq!(s.off_x, 24.0);
        assert!(s.fade);
        assert_eq!(s.mode, "kbm");
        assert!(s.on);
    }

    #[test]
    fn monitor_index_or_auto() {
        assert_eq!(PadSettings::from_value(&json!({ "monitor": 1 })).monitor_index(), Some(1));
        assert_eq!(PadSettings::from_value(&json!({ "monitor": 16 })).monitor_index(), None);
        assert_eq!(PadSettings::default().monitor_index(), None);
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = PadSettings::default();
        s.keys = vec!["w".into(), "space".into()];
        s.monitor = json!(1);
        let back: PadSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn old_app_import_turns_it_on() {
        let s = from_old_app(&json!({ "theme": "gold", "anchor_h": "left" }));
        assert!(s.on);
        assert_eq!(s.theme, "gold");
        assert_eq!(s.anchor_h, "left");
    }
}
