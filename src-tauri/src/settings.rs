//! Everything the user sets, saved as JSON in %APPDATA%\Deck.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Pin {
    pub id: String,
    /// "app" (Start menu id), "path" (file/folder), "url"
    pub kind: String,
    pub target: String,
    pub name: String,
}

impl Default for Pin {
    fn default() -> Self {
        Self { id: String::new(), kind: "app".into(), target: String::new(), name: String::new() }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub updated: u64,
    pub pinned: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct WsWindow {
    pub exe: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// "normal" | "max" | "min"
    pub state: String,
}

/// A saved scene: windows in their places, plus apps to close and an audio
/// output to switch to.
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub windows: Vec<WsWindow>,
    /// Process names (e.g. "EpicGamesLauncher.exe") to close.
    pub close: Vec<String>,
    /// Output device name to switch to ("" = leave it).
    pub audio_out: String,
    pub awake: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Zone {
    /// Fractions of the work area, 0..1.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for Zone {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, w: 0.5, h: 1.0 }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct Layout {
    pub id: String,
    pub name: String,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub version: u32,

    // ---- how Deck opens
    /// "top-left" | "top-right" | "bottom-left" | "bottom-right" | "off"
    pub hot_corner: String,
    pub hot_corner_ms: u32,
    /// Allow the hot corner while a fullscreen game or video is in front.
    pub hot_corner_fullscreen: bool,
    pub hide_on_blur: bool,
    pub autostart: bool,
    /// "panel": a quick panel opened by the hot corner or the shortcut, gone when you click away |
    /// "app": a normal window with a taskbar button that stays open
    pub mode: String,
    /// keyboard shortcut that opens / closes Deck ("" = none)
    pub open_hotkey: String,
    /// where the app window was (physical x, y, w, h) and whether it was maximized
    pub app_rect: Option<[i32; 4]>,
    pub app_max: bool,
    /// sidebar groups folded shut
    pub nav_folded: Vec<String>,
    /// the user's own sidebar order: { groups: [name…], items: { group: ["page/sub"…] } } (null = default)
    pub nav_order: serde_json::Value,
    /// the sidebar's width in CSS pixels (0 = the default)
    pub nav_width: u32,
    /// how the user arranged a page's panels: page -> { order, hidden, size: { id: [columns, height] } }
    pub panels: HashMap<String, serde_json::Value>,

    // ---- home
    pub pinned: Vec<Pin>,
    /// Home tiles in order; missing ones are hidden.
    pub tiles: Vec<String>,
    pub accent: String,
    /// the app's colour theme: midnight (default), graphite, oled, black, nord, forest, plum, ember
    pub theme: String,
    /// the theme picker was shown (first start, or the first start after the update that added it)
    pub theme_picked: bool,
    /// the look picker (theme, accent, opacity) was shown: first start, or once after the update that added opacity
    pub look_picked: bool,
    /// the first-start picker with the opening animation and low-spec mode too (shown once after that update)
    pub setup_picked: bool,
    /// Deck's opacity, %, with the pin on and off (30-100)
    pub opacity_pinned: u32,
    pub opacity_unpinned: u32,
    /// how Deck appears when it opens: zoom (default), fade, pop, rise, drop, flip, swirl, blur, none
    pub open_anim: String,
    /// low-spec mode: no animations or soft shadows, live readings every 3 s instead of every second
    pub low_spec: bool,
    /// Home's PC tile: its cards in order (cpu, ram, gpu, vram, net) and whether the drives show
    pub pc_cards: Vec<String>,
    pub pc_drives: bool,

    // ---- search
    /// id -> (launch count, last launch unix seconds)
    pub frecency: HashMap<String, (u32, u64)>,
    pub index_roots: Vec<String>,
    pub index_on: bool,
    pub web_search: String,

    // ---- clipboard
    pub clip_on: bool,
    pub clip_max: usize,
    pub clip_images: bool,
    /// Clicking a clipboard item pastes it into the app you were in.
    pub clip_paste: bool,

    // ---- windows
    pub zones_on: bool,
    /// "rbutton" (tap/hold right mouse button while dragging) | "always"
    pub zones_trigger: String,
    pub layout: String,
    pub layouts: Vec<Layout>,
    pub workspaces: Vec<Workspace>,

    // ---- power
    pub breaks_on: bool,
    pub breaks_every_min: u32,
    pub breaks_len_s: u32,

    // ---- display / theme
    pub theme_auto: bool,
    /// "times" | "sun"
    pub theme_mode: String,
    pub light_at: String,
    pub dark_at: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub place: String,

    // ---- network
    pub ping_hosts: Vec<String>,

    // ---- notes (no longer shown; kept so saved notes stay in the settings file)
    pub notes: Vec<Note>,

    // ---- screen tools
    pub colors: Vec<String>,

    // ---- your other PCs
    /// Transfer: send files, folders, installers, apps and setups between your PCs (Deck on both)
    pub transfer_on: bool,
    /// Claude Sync: Claude Code's sessions, memory and settings kept the same on your PCs (off by default)
    pub claude_sync_on: bool,
    /// Claude Code sync shows in the sidebar (hidden by default; always shown while it is on)
    pub claude_sync_nav: bool,

    // ---- phone
    pub remote_on: bool,
    /// the phone remote's Screen view: off (default), view, or control (taps and typing reach the PC)
    pub remote_screen: String,
    /// its largest picture: "sharp" = Full (the screen's own size, the default), "balanced" = Medium (1280 px), "fast" = Small (960 px)
    pub remote_screen_quality: String,
    /// its pictures a second at most: 15, 24, 30 (default) or 60
    pub remote_screen_fps: u32,
    /// its bitrate in kb/s: 0 = Auto (measures the connection and follows it), else a fixed limit
    pub remote_screen_kbps: u32,
    /// under a bitrate limit: "smooth" keeps the frame rate (blurrier), "sharp" keeps the picture clear (fewer frames)
    pub remote_screen_priority: String,
    /// "auto" (H.264 where the phone can decode it) or "jpeg"
    pub remote_screen_codec: String,
    pub remote_port: u16,
    pub remote_token: String,
    /// phones at a time on the home network / away (Tailscale or any address that isn't this
    /// network); 0 = no limit. A phone counts once, however many pages or views it has open.
    pub remote_max_local: u32,
    pub remote_max_online: u32,
    /// while a phone at home is connected, phones away can't connect (and the other way around)
    pub remote_local_blocks_online: bool,
    pub remote_online_blocks_local: bool,

    // ---- game boost
    /// App (lower-case exe name) -> close it for gaming. Missing = Deck's suggestion.
    pub boost_close: HashMap<String, bool>,
    /// Switch to the fastest power plan while Full power is on.
    pub boost_power: bool,
    /// Background apps run at lower priority while Full power is on.
    pub boost_priority: bool,
    /// Free memory from background apps.
    pub boost_trim: bool,
    /// Turn Full power off by itself when the game closes.
    pub boost_auto_end: bool,
    /// While Full power is on use these profiles instead of the fastest power plan ("" = don't).
    pub boost_cpu: String,
    pub boost_gpu: String,

    // ---- hardware profiles
    pub cpu_profiles: Vec<crate::profiles::CpuProfile>,
    pub gpu_profiles: Vec<crate::profiles::GpuProfile>,
    /// id of the profile last switched to ("" = none / something else switched)
    pub active_cpu: String,
    pub active_gpu: String,
    /// GPU profile applied when Deck starts ("" = none)
    pub gpu_startup: String,
    /// CPU profile switched to when the PC is unplugged / plugged in ("" = don't switch)
    pub cpu_battery_profile: String,
    pub cpu_plugged_profile: String,
    /// a CPU profile's values also apply on battery (false: switching leaves the plan's own
    /// battery values alone; edits made in Deck still reach them)
    pub cpu_battery_same: bool,
    /// small notification when a profile switches
    pub profile_toast: bool,
    /// keyboard shortcuts for profiles on/off
    pub hotkeys_on: bool,
    /// Windows Game Mode setting before a "hold in games" profile turned it off (-1 = untouched)
    pub game_mode_orig: i32,

    // ---- RTSS overlay (replaces Afterburner's on-screen display)
    pub osd_on: bool,
    /// item ids shown in the overlay, in order
    pub osd_items: Vec<String>,
    pub osd_ms: u32,
    /// hotkey that shows / hides the overlay
    pub osd_hotkey: String,
    /// turns RivaTuner's framerate limiter on and off (Afterburner's limiter key)
    pub limiter_hotkey: String,
    /// hotkey that starts the average, lows, lowest and highest over (like Afterburner's benchmark reset)
    pub osd_reset_hotkey: String,
    /// start RivaTuner Statistics Server together with Deck
    pub rtss_autostart: bool,
    /// RivaTuner keeps running without its tray icon (the Overlay page opens it)
    pub rtss_tray_hidden: bool,
    /// notifications when a drive gets worse (off unless asked for: no surprise pop-ups)
    pub drive_alerts: bool,
    /// publish Afterburner's monitoring shared memory for other tools
    pub mahm_on: bool,
    /// how the overlay is laid out and drawn
    pub osd_style: crate::osd::OsdStyle,
    /// units shown everywhere (pages, phone, overlay): "MHz" / "GHz"
    pub unit_clock: String,
    /// "Mb/s" (bits) / "MB/s" (bytes)
    pub unit_net: String,
    /// "C" / "F"
    pub unit_temp: String,
}

/// Every Home tile, in the default order (the default places and sizes are in ui/js/layout.js, HOME_DEFAULT).
pub const ALL_TILES: &[&str] =
    &["clock", "media", "audio", "system", "profiles", "clipboard", "power", "pinned", "network", "quick"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            hot_corner: "top-left".into(),
            hot_corner_ms: 250,
            hot_corner_fullscreen: false,
            hide_on_blur: true,
            autostart: true,
            mode: "panel".into(),
            open_hotkey: String::new(),
            app_rect: None,
            app_max: false,
            // every sidebar group starts folded: a short menu, opened one group at a time
            nav_folded: ["Hardware", "Monitor", "Desktop", "Maintenance", "Power & time"].iter().map(|s| s.to_string()).collect(),
            nav_order: serde_json::Value::Null,
            nav_width: 0,
            panels: HashMap::new(),
            pinned: Vec::new(),
            tiles: ALL_TILES.iter().map(|s| s.to_string()).collect(),
            accent: "#4f8cff".into(),
            theme: "midnight".into(),
            theme_picked: false,
            look_picked: false,
            setup_picked: false,
            opacity_pinned: 100,
            opacity_unpinned: 100,
            open_anim: "zoom".into(),
            low_spec: false,
            pc_cards: vec!["cpu".into(), "ram".into(), "gpu".into(), "vram".into()],
            pc_drives: true,
            frecency: HashMap::new(),
            index_roots: Vec::new(),
            index_on: true,
            web_search: "https://www.google.com/search?q=".into(),
            clip_on: true,
            clip_max: 400,
            clip_images: true,
            clip_paste: true,
            zones_on: true,
            zones_trigger: "rbutton".into(),
            layout: "focus".into(),
            layouts: builtin_layouts(),
            workspaces: Vec::new(),
            breaks_on: false,
            breaks_every_min: 50,
            breaks_len_s: 60,
            theme_auto: false,
            theme_mode: "times".into(),
            light_at: "07:30".into(),
            dark_at: "19:30".into(),
            lat: None,
            lon: None,
            place: String::new(),
            ping_hosts: vec!["1.1.1.1".into(), "8.8.8.8".into(), "discord.com".into()],
            notes: Vec::new(),
            colors: Vec::new(),
            transfer_on: true,
            claude_sync_on: false,
            claude_sync_nav: false,
            remote_on: true,
            remote_screen: "off".into(),
            remote_screen_quality: "sharp".into(),
            remote_screen_fps: 30,
            remote_screen_kbps: 0,
            remote_screen_priority: "smooth".into(),
            remote_screen_codec: "auto".into(),
            remote_port: 47830,
            remote_token: String::new(),
            remote_max_local: 0,
            remote_max_online: 0,
            remote_local_blocks_online: false,
            remote_online_blocks_local: false,
            boost_close: HashMap::new(),
            boost_power: true,
            boost_priority: true,
            boost_trim: true,
            boost_auto_end: true,
            boost_cpu: String::new(),
            boost_gpu: String::new(),
            cpu_profiles: Vec::new(),
            gpu_profiles: Vec::new(),
            active_cpu: String::new(),
            active_gpu: String::new(),
            gpu_startup: String::new(),
            cpu_battery_profile: String::new(),
            cpu_plugged_profile: String::new(),
            cpu_battery_same: true,
            profile_toast: true,
            hotkeys_on: true,
            game_mode_orig: -1,
            osd_on: false,
            osd_items: crate::osd::DEFAULT_ITEMS.iter().map(|s| s.to_string()).collect(),
            osd_ms: 1000,
            osd_hotkey: String::new(),
            limiter_hotkey: String::new(),
            osd_reset_hotkey: String::new(),
            rtss_autostart: false,
            rtss_tray_hidden: false,
            drive_alerts: false,
            mahm_on: true,
            osd_style: crate::osd::OsdStyle::default(),
            unit_clock: "MHz".into(),
            unit_net: "Mb/s".into(),
            unit_temp: "C".into(),
        }
    }
}

impl Settings {
    /// The overlay's look with the app-wide units filled in.
    pub fn osd_render_style(&self) -> crate::osd::OsdStyle {
        let mut st = self.osd_style.clone();
        st.ghz = self.unit_clock == "GHz";
        st.fahrenheit = self.unit_temp == "F";
        st.net_bytes = self.unit_net == "MB/s";
        st
    }
}

fn z(x: f32, y: f32, w: f32, h: f32) -> Zone {
    Zone { x, y, w, h }
}

pub fn builtin_layouts() -> Vec<Layout> {
    let l = |id: &str, name: &str, zones: Vec<Zone>| Layout { id: id.into(), name: name.into(), zones };
    vec![
        l("halves", "Halves", vec![z(0.0, 0.0, 0.5, 1.0), z(0.5, 0.0, 0.5, 1.0)]),
        l("focus", "Focus + side", vec![z(0.0, 0.0, 0.68, 1.0), z(0.68, 0.0, 0.32, 1.0)]),
        l("thirds", "Thirds", vec![z(0.0, 0.0, 1.0 / 3.0, 1.0), z(1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0), z(2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0)]),
        l("center", "Wide center", vec![z(0.0, 0.0, 0.2, 1.0), z(0.2, 0.0, 0.6, 1.0), z(0.8, 0.0, 0.2, 1.0)]),
        l(
            "quarters",
            "Quarters",
            vec![z(0.0, 0.0, 0.5, 0.5), z(0.5, 0.0, 0.5, 0.5), z(0.0, 0.5, 0.5, 0.5), z(0.5, 0.5, 0.5, 0.5)],
        ),
        l(
            "main-stack",
            "Main + stack",
            vec![z(0.0, 0.0, 0.6, 1.0), z(0.6, 0.0, 0.4, 0.5), z(0.6, 0.5, 0.4, 0.5)],
        ),
    ]
}

impl Settings {
    /// %APPDATA%\Deck, or %APPDATA%\Deck-<profile> when DECK_PROFILE is set
    /// (test runs must never touch the real settings).
    pub fn dir() -> PathBuf {
        let name = match std::env::var("DECK_PROFILE") {
            Ok(p) if !p.is_empty() => format!("Deck-{p}"),
            _ => "Deck".to_string(),
        };
        crate::util::appdata().join(name)
    }


    pub fn load() -> Self {
        Self::load_from(&Self::dir()).0
    }

    /// Loads `settings.json` from `dir`. The note, when there is one, says what was wrong with the
    /// file and what was done about it (shown to the user). A missing file is a new install, not
    /// damage. A file that is there but can't be read is kept aside, the backup is tried, and then
    /// as much as still parses is merged into the defaults, so one bad field or a cut-off save
    /// never costs every setting.
    pub fn load_from(dir: &Path) -> (Self, Option<String>) {
        let main = dir.join("settings.json");
        let raw = match std::fs::read_to_string(&main) {
            Ok(t) => t,
            Err(_) => return (Self::from_backup(dir).unwrap_or_default().0, Self::from_backup(dir).map(|b| b.1).unwrap_or(None)),
        };
        if let Ok(s) = serde_json::from_str::<Settings>(&raw) {
            let mut s = s;
            s.repair();
            return (s, None);
        }
        // the file is there but unreadable: keep it, so nothing is silently lost
        let kept = dir.join(format!("settings.json.corrupt-{}", crate::util::now_s()));
        let _ = std::fs::rename(&main, &kept);
        if let Some((s, _)) = Self::from_backup(dir) {
            return (s, Some("Your settings file was damaged; Deck restored the last good copy.".into()));
        }
        // no backup: rescue whatever fields still parse
        let (mut s, rescued) = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => (serde_json::from_value::<Settings>(Self::merge_into_default(v)).unwrap_or_default(), true),
            Err(_) => (Settings::default(), false),
        };
        s.repair();
        let note = if rescued {
            "Some settings were damaged; Deck kept the ones it could read and reset the rest."
        } else {
            "Your settings file was damaged and couldn't be read; Deck started with the defaults (the old file was kept)."
        };
        (s, Some(note.into()))
    }

    fn from_backup(dir: &Path) -> Option<(Self, Option<String>)> {
        let t = std::fs::read_to_string(dir.join("settings.json.bak")).ok()?;
        let mut s: Settings = serde_json::from_str(&t).ok()?;
        s.repair();
        Some((s, Some("Your settings file was damaged; Deck restored the last good copy.".into())))
    }

    /// Keeps only the fields of `v` that deserialize on their own, dropping the rest onto the
    /// defaults (one wrong-typed field doesn't take the others with it).
    fn merge_into_default(v: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::to_value(Settings::default()).unwrap_or_else(|_| serde_json::json!({}));
        if let (true, serde_json::Value::Object(fields)) = (base.is_object(), v) {
            for (k, val) in fields {
                let mut trial = base.clone();
                trial[&k] = val.clone();
                if serde_json::from_value::<Settings>(trial).is_ok() {
                    base[&k] = val;
                }
            }
        }
        base
    }

    fn repair(&mut self) {
        if self.remote_token.len() < 16 {
            self.remote_token = crate::util::random_token(22);
        }
        if !["top-left", "top-right", "bottom-left", "bottom-right", "off"].contains(&self.hot_corner.as_str()) {
            self.hot_corner = "top-left".into();
        }
        self.hot_corner_ms = self.hot_corner_ms.clamp(0, 2000);
        if !["panel", "app"].contains(&self.mode.as_str()) {
            self.mode = "panel".into();
        }
        self.clip_max = self.clip_max.clamp(20, 5000);
        self.breaks_every_min = self.breaks_every_min.clamp(5, 240);
        self.breaks_len_s = self.breaks_len_s.clamp(10, 1800);
        self.tiles.retain(|t| ALL_TILES.contains(&t.as_str()));
        self.tiles.dedup();
        for b in builtin_layouts() {
            if !self.layouts.iter().any(|l| l.id == b.id) {
                self.layouts.push(b);
            }
        }
        if !self.layouts.iter().any(|l| l.id == self.layout) {
            self.layout = "focus".into();
        }
        if self.index_roots.is_empty() {
            let h = crate::util::home();
            self.index_roots = ["Desktop", "Documents", "Downloads", "Pictures", "Videos", "Music", "OneDrive"]
                .iter()
                .map(|d| h.join(d))
                .filter(|p| p.is_dir())
                .map(|p| p.to_string_lossy().to_string())
                .collect();
        }
        if self.colors.len() > 40 {
            self.colors.truncate(40);
        }
    }

    /// Atomic save (write a temp file, then rename over the old one).
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::dir()).inspect_err(|e| crate::log::warn(format!("settings not saved: {e}")))?;
        // Deck saved its hardware settings itself: see crate::seal
        crate::seal::record("settings", &crate::seal::hardware_part(self));
        Ok(())
    }

    /// `save` into `dir`. The current good file is copied to `settings.json.bak` first, so a crash
    /// during the save still leaves a whole file to come back from.
    pub fn save_to(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let main = dir.join("settings.json");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // keep the previous version, but only if it parses (never back up a damaged file)
        if let Ok(old) = std::fs::read_to_string(&main) {
            if serde_json::from_str::<Settings>(&old).is_ok() {
                let _ = crate::util::write_atomic(&dir.join("settings.json.bak"), old.as_bytes());
            }
        }
        crate::util::write_atomic(&main, text.as_bytes()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod durability_tests {
    use super::Settings;
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deck-settings-{name}-{}", crate::util::now_ms()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn with_place(place: &str) -> Settings {
        Settings { place: place.into(), ..Default::default() }
    }

    fn corrupt_copies(d: &std::path::Path) -> usize {
        std::fs::read_dir(d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("settings.json.corrupt-")).count()
    }

    #[test]
    fn a_damaged_file_comes_back_from_the_backup() {
        let d = dir("bak");
        std::fs::write(d.join("settings.json.bak"), serde_json::to_string(&with_place("Backup town")).unwrap()).unwrap();
        std::fs::write(d.join("settings.json"), r#"{"place": "Ne"#).unwrap(); // cut off by a crash
        let (s, note) = Settings::load_from(&d);
        assert_eq!(s.place, "Backup town");
        assert!(note.is_some(), "the user is told");
        assert_eq!(corrupt_copies(&d), 1, "the damaged file is kept, not overwritten");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_bad_value_doesnt_cost_the_rest() {
        let d = dir("field");
        let mut v = serde_json::to_value(with_place("Keep me")).unwrap();
        v["clip_max"] = serde_json::json!("lots");
        std::fs::write(d.join("settings.json"), v.to_string()).unwrap();
        let (s, note) = Settings::load_from(&d);
        assert_eq!(s.place, "Keep me", "the other settings survive");
        assert_eq!(s.clip_max, Settings::default().clip_max);
        assert!(note.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_file_with_no_backup_is_kept_aside() {
        let d = dir("empty");
        std::fs::write(d.join("settings.json"), b"").unwrap();
        let (s, note) = Settings::load_from(&d);
        assert_eq!(s.place, Settings::default().place);
        assert!(note.is_some());
        assert_eq!(corrupt_copies(&d), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_keeps_the_previous_version_as_a_backup() {
        let d = dir("save");
        with_place("First").save_to(&d).unwrap();
        with_place("Second").save_to(&d).unwrap();
        assert_eq!(Settings::load_from(&d).0.place, "Second");
        let bak: Settings = serde_json::from_str(&std::fs::read_to_string(d.join("settings.json.bak")).unwrap()).unwrap();
        assert_eq!(bak.place, "First");
        // a missing settings.json (a crash between the two renames) comes back from the backup
        std::fs::remove_file(d.join("settings.json")).unwrap();
        assert_eq!(Settings::load_from(&d).0.place, "First");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_new_install_has_no_note() {
        let d = dir("new");
        let (_, note) = Settings::load_from(&d);
        assert!(note.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
