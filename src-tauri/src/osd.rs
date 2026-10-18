//! What the in-game overlay says and how it is laid out. Pure: readings in, lines out.
//! The same lines draw the preview on the Sensors page and become RTSS text tags, so what
//! the page shows is what the overlay writes. Nothing here names a particular PC: items
//! appear when the PC has the reading (fans and board sensors are listed by their own names).

use crate::rtss::Snap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct ItemDef {
    pub id: &'static str,
    /// the name on the Sensors page
    pub label: &'static str,
    /// the label in the one-item-per-line layout
    pub short: &'static str,
    /// the section on the Sensors page (see `CATS`)
    pub cat: &'static str,
    /// which line the item joins in the grouped layout (see `GROUPS`)
    pub group: &'static str,
}

pub const CATS: &[(&str, &str)] = &[
    ("gpu", "Graphics card"),
    ("cpu", "Processor"),
    ("mem", "Memory"),
    ("game", "Game and frames"),
    ("pc", "Fans, sensors and network"),
    ("info", "Info"),
];

/// (group, label in front of the line)
const GROUPS: &[(&str, &str)] = &[
    ("gpu", "GPU"), ("cpu", "CPU"), ("cores", "CPU%"), ("coreclk", "MHz"), ("coretmp", "°C"), ("coretab", ""), ("threadtab", ""), ("vram", "MEM"),
    ("ram", "RAM"), ("app", "APP"), ("fps", "FPS"), ("net", "NET"), ("fan", "FAN"), ("btemp", "TEMP"), ("title", ""), ("info", ""),
];

const fn d(id: &'static str, label: &'static str, short: &'static str, cat: &'static str, group: &'static str) -> ItemDef {
    ItemDef { id, label, short, cat, group }
}

/// Every fixed overlay item, in the order the Sensors page lists them.
pub const ITEMS: &[ItemDef] = &[
    d("gpu_temp", "Temperature", "GPU temp", "gpu", "gpu"),
    d("gpu_usage", "Usage", "GPU load", "gpu", "gpu"),
    d("gpu_mem", "Memory used", "VRAM", "gpu", "vram"),
    d("gpu_mem_pct", "Memory used (%)", "VRAM %", "gpu", "vram"),
    d("gpu_clock", "Core clock", "GPU clock", "gpu", "gpu"),
    d("gpu_memclock", "Memory clock", "VRAM clock", "gpu", "vram"),
    d("gpu_power", "Power", "GPU power", "gpu", "gpu"),
    d("gpu_power_pct", "Power (% of the default limit)", "GPU power %", "gpu", "gpu"),
    d("gpu_fan", "Fan", "GPU fan", "gpu", "gpu"),
    d("gpu_volt", "Voltage", "GPU volt", "gpu", "gpu"),
    d("gpu_limit", "What is holding the clock back", "GPU limit", "gpu", "gpu"),
    d("cpu_temp", "Temperature", "CPU temp", "cpu", "cpu"),
    d("cpu_usage", "Usage", "CPU load", "cpu", "cpu"),
    d("cpu_clock", "Clock", "CPU clock", "cpu", "cpu"),
    d("cpu_power", "Power", "CPU power", "cpu", "cpu"),
    d("cpu_volt", "Voltage (VID)", "CPU volt", "cpu", "cpu"),
    d("cpu_cores", "Usage of each thread (grid)", "Thread %", "cpu", "cores"),
    d("core_clock", "Clock of each core", "Core MHz", "cpu", "coreclk"),
    d("core_temp", "Temperature of each core", "Core temp", "cpu", "coretmp"),
    d("core_table", "Each core: usage, clock and temperature", "Cores", "cpu", "coretab"),
    d("thread_table", "Each thread: usage and clock (like Afterburner)", "Threads", "cpu", "threadtab"),
    d("ram", "RAM used", "RAM", "mem", "ram"),
    d("commit", "Commit charge", "Commit", "mem", "ram"),
    d("proc_ram", "RAM used by the game", "Game RAM", "mem", "ram"), // on the RAM line, like Afterburner
    d("fps", "Framerate", "FPS", "game", "fps"),
    d("frametime", "Frametime", "Frametime", "game", "fps"),
    d("fps_avg", "Average framerate", "FPS Avg", "game", "fps"),
    d("fps_1low", "1% low", "FPS 1% Low", "game", "fps"),
    d("fps_01low", "0.1% low", "FPS 0.1% Low", "game", "fps"),
    d("fps_min", "Lowest framerate", "FPS Min", "game", "fps"),
    d("fps_max", "Highest framerate", "FPS Max", "game", "fps"),
    d("net_down", "Download speed", "Down", "pc", "net"),
    d("net_up", "Upload speed", "Up", "pc", "net"),
    d("app", "Game name", "Game", "info", "title"),
    d("res", "Resolution", "Res", "info", "info"),
    d("api", "Graphics API", "API", "info", "info"),
    d("time", "Clock", "Time", "info", "info"),
];

/// What a new install shows.
pub const DEFAULT_ITEMS: &[&str] = &["gpu_temp", "gpu_usage", "gpu_mem", "gpu_clock", "gpu_memclock", "gpu_power", "gpu_fan", "cpu_temp", "cpu_usage", "cpu_clock", "cpu_power", "ram", "fps", "frametime"];

pub fn is_stat(id: &str) -> bool {
    matches!(id, "fps_avg" | "fps_1low" | "fps_01low" | "fps_min" | "fps_max")
}

/// (short label, group) for any item id, including this PC's own fans and sensors ("fan:0", "temp:2").
fn info(id: &str, s: &Snap) -> Option<(String, &'static str)> {
    if let Some(d) = ITEMS.iter().find(|d| d.id == id) {
        return Some((d.short.to_string(), d.group));
    }
    let dynamic = |prefix: &str, list: &[(String, f32)], group: &'static str| -> Option<(String, &'static str)> {
        let k: usize = id.strip_prefix(prefix)?.parse().ok()?;
        list.get(k).map(|(n, _)| (n.clone(), group))
    };
    dynamic("fan:", &s.fans, "fan").or_else(|| dynamic("temp:", &s.board_temps, "btemp"))
}

/// The catalogue for the Sensors page: fixed items, then this PC's fans and board sensors.
/// (id, label, short, category, needs frame data, has a reading right now)
pub fn catalogue(s: &Snap) -> Vec<(String, String, String, &'static str, bool, bool)> {
    let st = OsdStyle::default();
    let mut v: Vec<_> = ITEMS
        .iter()
        .map(|d| {
            let have = match d.id {
                "cpu_cores" => !s.cores.is_empty(),
                "core_clock" => s.core_reads.iter().any(|c| c.clock.is_some()),
                "core_temp" => s.core_reads.iter().any(|c| c.temp.is_some()),
                "core_table" => !s.core_reads.is_empty(),
                "thread_table" => !s.thread_reads.is_empty(),
                "res" | "api" | "time" | "app" | "fps" | "frametime" => true,
                id if is_stat(id) => true,
                id => value(id, s, &st, "").is_some(),
            };
            (d.id.to_string(), d.label.to_string(), d.short.to_string(), d.cat, is_stat(d.id), have)
        })
        .collect();
    for (i, (n, _)) in s.fans.iter().enumerate() {
        v.push((format!("fan:{i}"), format!("Fan speed: {n}"), n.clone(), "pc", false, true));
    }
    for (i, (n, _)) in s.board_temps.iter().enumerate() {
        v.push((format!("temp:{i}"), format!("Temperature: {n}"), n.clone(), "pc", false, true));
    }
    v
}

// ---------------------------------------------------------------------------- the style

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct OsdStyle {
    /// "grouped" (a line per group), "list" (a line per item), "line" (everything on one line)
    pub layout: String,
    pub labels: bool,
    /// "small" (smaller), "normal", "off"
    pub units: String,
    /// pad the labels so the values start in one column
    pub align: bool,
    pub label_color: String,
    pub value_color: String,
    /// numbers in trouble
    pub warn_color: String,
    /// numbers getting there
    pub warm_color: String,
    /// colour a number by how loaded or hot it is
    pub heat: bool,
    /// colour the numbers by part too, not only the labels
    pub group_values: bool,
    /// give GPU, CPU, memory and framerate lines their own colours
    pub group_colors: bool,
    pub gpu_color: String,
    pub cpu_color: String,
    pub ram_color: String,
    pub fps_color: String,
    pub info_color: String,
    pub warn: bool,
    /// every framerate figure (min, average, lows…) on a line of its own in the grouped layout
    pub fps_lines: bool,
    /// line the numbers up in columns
    pub columns: bool,
    pub clock_12h: bool,
    pub clock_secs: bool,
    pub warn_gpu_temp: u32,
    pub warn_cpu_temp: u32,
    /// framerate below this turns red (0 = never)
    pub warn_fps: u32,
    pub cores_per_row: u32,
    /// "MB" or "GB"
    pub mem_unit: String,
    /// how far back the average, lows, minimum and maximum look
    pub stats_secs: u32,

    // ---- how RivaTuner draws it (written to its profiles)
    /// "tl" "tr" "bl" "br"
    pub corner: String,
    pub margin_x: u32,
    pub margin_y: u32,
    /// RivaTuner's text zoom, 1 to 8
    pub zoom: u32,
    /// dark outline behind the letters
    #[serde(alias = "bgnd")]
    pub shadow: bool,
    /// a translucent box behind all the text
    pub box_on: bool,
    pub box_opacity: u32,
    pub font: String,
    pub bold: bool,
    /// RivaTuner's own frametime graph under the text
    pub graph: bool,
    pub graph_w: u32,
    pub graph_h: u32,
    pub graph_style: u32,
    /// the graph's top in ms (0 = it picks)
    pub graph_max: u32,
    /// the graph as wide as the overlay's longest line (else `graph_w` characters)
    pub graph_fit: bool,
    /// "modern": a labelled line of its own on a background panel, the live frametime at its end,
    /// scaled to typical frames; "classic": the bare graph under the text, scaled to the slowest point
    pub graph_look: String,
    /// a coloured strip along the screen edge that shows each frame's time
    pub color_bar: bool,
    /// shrink the overlay when it would not fit the screen
    pub fit: bool,
    /// whole numbers for the framerate
    pub int_fps: bool,
    /// also change every game profile RivaTuner has made (a game with its own profile ignores the global one)
    pub all_games: bool,
    /// items the user put on a line of their own: item id -> that line's label
    pub groups: std::collections::BTreeMap<String, String>,

    // ---- the app-wide units (Settings > Units), copied in by `Settings::osd_render_style`
    #[serde(skip)]
    pub ghz: bool,
    #[serde(skip)]
    pub fahrenheit: bool,
    #[serde(skip)]
    pub net_bytes: bool,
}

impl Default for OsdStyle {
    fn default() -> Self {
        Self {
            layout: "grouped".into(),
            labels: true,
            units: "small".into(),
            align: true,
            label_color: "FF9A40".into(),
            value_color: "FFFFFF".into(),
            warn_color: "FF5A5A".into(),
            warm_color: "FFC85A".into(),
            heat: false,
            group_values: false,
            group_colors: false,
            gpu_color: "76B900".into(),
            cpu_color: "4FA3FF".into(),
            ram_color: "C77DFF".into(),
            fps_color: "FFD60A".into(),
            info_color: "9AA3B5".into(),
            warn: true,
            fps_lines: false,
            columns: false,
            clock_12h: false,
            clock_secs: false,
            warn_gpu_temp: 85,
            warn_cpu_temp: 90,
            warn_fps: 0,
            cores_per_row: 8,
            mem_unit: "MB".into(),
            stats_secs: 60,
            corner: "tr".into(),
            margin_x: 1,
            margin_y: 1,
            zoom: 2,
            shadow: true,
            box_on: false,
            box_opacity: 50,
            font: "Unispace".into(),
            bold: false,
            graph: false,
            graph_w: 32,
            graph_h: 4,
            graph_style: 0,
            graph_max: 0,
            graph_fit: true,
            graph_look: "modern".into(),
            color_bar: false,
            fit: false,
            int_fps: true,
            all_games: true,
            groups: Default::default(),
            ghz: false,
            fahrenheit: false,
            net_bytes: false,
        }
    }
}

fn hex(c: &str, fallback: &str) -> String {
    let c = c.trim_start_matches('#');
    if c.len() == 6 && c.chars().all(|x| x.is_ascii_hexdigit()) { c.to_uppercase() } else { fallback.to_string() }
}

// ---------------------------------------------------------------------------- the lines

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    /// the value as shown in the preview
    pub text: String,
    pub unit: String,
    /// the tag RivaTuner fills in itself (`<FR>`), used instead of `text` on screen
    pub rtss: Option<&'static str>,
    /// 0 normal, 1 getting there, 2 in trouble
    pub tone: u8,
    /// a label in front of this value (one-line layout)
    pub pre: String,
    /// where the frametime graph goes (the modern graph's line): `<OBJ>` on the overlay
    pub graph: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub label: String,
    pub group: &'static str,
    pub parts: Vec<Part>,
}

fn num(v: f32, dec: usize) -> String {
    format!("{:.*}", dec, v)
}

fn part(text: String, unit: &str) -> Part {
    Part { text, unit: unit.into(), ..Default::default() }
}

/// A clock speed (MHz in) in the chosen unit; `w` pads it to that width (tables), 0 = no padding.
fn clk(v: f32, st: &OsdStyle, w: usize) -> Part {
    if st.ghz { part(format!("{:>w$.2}", v / 1000.0), "GHz") } else { part(format!("{:>w$.0}", v), "MHz") }
}

/// A temperature (°C in) in the chosen unit. Warning limits stay in °C: only the text changes.
fn deg(v: f32, st: &OsdStyle, w: usize) -> Part {
    if st.fahrenheit { part(format!("{:>w$.0}", v * 9.0 / 5.0 + 32.0), "°F") } else { part(format!("{:>w$.0}", v), "°C") }
}

/// A network speed (MB/s in, 1 MB = 1024 KB) in the chosen unit: bytes or bits per second.
fn net(v: f32, st: &OsdStyle) -> Part {
    if st.net_bytes {
        if v >= 1.0 { part(num(v, 1), "MB/s") } else { part(num(v * 1024.0, 0), "KB/s") }
    } else {
        let mbit = v * 1_048_576.0 * 8.0 / 1e6;
        if mbit >= 1.0 { part(num(mbit, 1), "Mb/s") } else { part(num(mbit * 1000.0, 0), "kb/s") }
    }
}

/// How worried to be about a reading. `limit` is the temperature limit for temperatures.
fn tone(kind: &str, v: f32, limit: u32, st: &OsdStyle) -> u8 {
    let hot = |lim: f32, warm: f32| if v >= lim { 2 } else if st.heat && v >= warm { 1 } else { 0 };
    match kind {
        "temp" if st.warn || st.heat => {
            let lim = limit as f32;
            if st.warn && v >= lim { 2 } else if st.heat && v >= lim - 12.0 { if v >= lim { 2 } else { 1 } } else { 0 }
        }
        "load" if st.heat => hot(90.0, 70.0),
        "mem" if st.heat => hot(92.0, 78.0),
        _ => 0,
    }
}

/// The value of one single-number item, or None when this PC has no such reading right now.
fn value(id: &str, s: &Snap, st: &OsdStyle, app: &str) -> Option<Part> {
    let f = |v: Option<f32>| v.filter(|v| v.is_finite());
    let one = |v: Option<f32>, unit: &str| f(v).map(|v| part(num(v, 0), unit));
    let with = |mut p: Part, t: u8| {
        p.tone = t;
        p
    };
    let mem = |v: Option<f32>| f(v).map(|v| if st.mem_unit == "GB" { part(num(v / 1024.0, 1), "GB") } else { part(num(v, 0), "MB") });
    let fps_dec = if st.int_fps { 0 } else { 1 };
    let rate = |v: Option<f32>| f(v).map(|v| part(num(v, fps_dec), "FPS")).or_else(|| Some(part("–".into(), "FPS")));
    let speed = |v: Option<f32>| f(v).map(|v| net(v, st));
    let temp = |v: Option<f32>| f(v).map(|v| deg(v, st, 0));
    let mhz = |v: Option<f32>| f(v).map(|v| clk(v, st, 0));
    if let Some(k) = id.strip_prefix("fan:").and_then(|k| k.parse::<usize>().ok()) {
        return s.fans.get(k).map(|(_, v)| part(num(*v, 0), "RPM"));
    }
    if let Some(k) = id.strip_prefix("temp:").and_then(|k| k.parse::<usize>().ok()) {
        return s.board_temps.get(k).map(|(_, v)| with(deg(*v, st, 0), tone("temp", *v, st.warn_cpu_temp, st)));
    }
    Some(match id {
        "gpu_temp" => {
            let p = temp(s.gpu_temp)?;
            let t = tone("temp", s.gpu_temp?, st.warn_gpu_temp, st);
            with(p, t)
        }
        "gpu_usage" => with(one(s.gpu_usage, "%")?, tone("load", s.gpu_usage?, 0, st)),
        "gpu_mem" => mem(s.gpu_mem_mb)?,
        "gpu_mem_pct" => {
            let pct = s.gpu_mem_mb? / s.gpu_mem_total_mb.filter(|t| *t > 0.0)? * 100.0;
            with(part(num(pct, 0), "%"), tone("mem", pct, 0, st))
        }
        "gpu_clock" => mhz(s.gpu_clock)?,
        "gpu_memclock" => mhz(s.gpu_memclock)?,
        "gpu_power" => one(s.gpu_power_w, "W")?,
        "gpu_power_pct" => one(s.gpu_power_pct, "%")?,
        "gpu_fan" => one(s.gpu_fan, "%")?,
        "gpu_volt" => one(s.gpu_volt_mv, "mV")?,
        "gpu_limit" => {
            let l = s.gpu_limit.as_str();
            with(part(if l.is_empty() { "none".into() } else { l.to_string() }, ""), if l.is_empty() { 0 } else { 1 })
        }
        "cpu_temp" => with(temp(s.cpu_temp)?, tone("temp", s.cpu_temp?, st.warn_cpu_temp, st)),
        "cpu_usage" => with(one(s.cpu_usage, "%")?, tone("load", s.cpu_usage?, 0, st)),
        "cpu_clock" => mhz(s.cpu_clock)?,
        "cpu_power" => one(s.cpu_power, "W")?,
        "cpu_volt" => f(s.cpu_volt).map(|v| part(num(v, 3), "V"))?,
        "ram" => mem(s.ram_mb)?,
        "commit" => mem(s.commit_mb)?,
        "proc_ram" => mem(s.proc_ram_mb)?,
        "fps" => {
            let mut p = rate(s.fps)?;
            p.rtss = Some("<FR>");
            p.tone = if st.warn && st.warn_fps > 0 && s.fps.map(|v| v < st.warn_fps as f32).unwrap_or(false) { 2 } else { 0 };
            p
        }
        "frametime" => {
            let mut p = f(s.frametime).map(|v| part(num(v, 1), "ms")).unwrap_or_else(|| part("–".into(), "ms"));
            p.rtss = Some("<FT>");
            p
        }
        "fps_avg" => rate(s.fps_avg)?,
        "fps_1low" => rate(s.fps_1low)?,
        "fps_01low" => rate(s.fps_01low)?,
        "fps_min" => rate(s.fps_min)?,
        "fps_max" => rate(s.fps_max)?,
        "net_down" => speed(s.net_down)?,
        "net_up" => speed(s.net_up)?,
        "app" => {
            let mut p = part(app.trim_end_matches(".exe").to_string(), "");
            p.rtss = Some("<APP>");
            p
        }
        // RivaTuner fills these in on the overlay; the preview shows what it recorded for the game
        "res" => {
            let mut p = part(s.game_res.map(|(w, h)| format!("{w}x{h}")).unwrap_or_else(|| "–".into()), "");
            p.rtss = Some("<RES>");
            p
        }
        "api" => {
            let mut p = part(s.game_api.unwrap_or("–").to_string(), "");
            p.rtss = Some("<API>");
            p
        }
        "time" => part(clock(st), ""),
        _ => return None,
    })
}

fn clock(st: &OsdStyle) -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    let (h, ampm) = if st.clock_12h { (if t.wHour % 12 == 0 { 12 } else { t.wHour % 12 }, if t.wHour < 12 { " AM" } else { " PM" }) } else { (t.wHour, "") };
    if st.clock_secs { format!("{:02}:{:02}:{:02}{}", h, t.wMinute, t.wSecond, ampm) } else { format!("{:02}:{:02}{}", h, t.wMinute, ampm) }
}

/// Grid items (one number per thread or core), wrapped `cores_per_row` to a line.
fn grid(label: &str, group: &'static str, parts: Vec<Part>, per: usize) -> Vec<Line> {
    parts
        .chunks(per)
        .enumerate()
        .map(|(n, c)| Line { label: if n == 0 { label.to_string() } else { String::new() }, group, parts: c.to_vec() })
        .collect()
}

/// The lines of a multi-number item. `label` is what the first line is called.
fn multi(id: &str, label: &str, s: &Snap, st: &OsdStyle) -> Vec<Line> {
    let per = st.cores_per_row.clamp(2, 32) as usize;
    match id {
        "cpu_cores" => grid(
            label,
            "cores",
            s.cores.iter().map(|v| {
                let mut p = part(format!("{:>3.0}", v), "");
                p.tone = tone("load", *v, 0, st);
                p
            }).collect(),
            per,
        ),
        "core_clock" => grid(label, "coreclk", s.core_reads.iter().filter_map(|c| c.clock).map(|v| part(format!("{:>4.0}", v), "")).collect(), per),
        "core_temp" => grid(
            label,
            "coretmp",
            s.core_reads.iter().filter_map(|c| c.temp).map(|v| {
                let mut p = part(format!("{:>3.0}", v), "");
                p.tone = tone("temp", v, st.warn_cpu_temp, st);
                p
            }).collect(),
            per,
        ),
        "thread_table" => s
            .thread_reads
            .iter()
            .map(|c| {
                let mut parts = vec![];
                if let Some(v) = c.load {
                    let mut p = part(format!("{:>3.0}", v), "%");
                    p.tone = tone("load", v, 0, st);
                    parts.push(p);
                }
                if let Some(v) = c.clock {
                    parts.push(clk(v, st, 4));
                }
                Line { label: c.name.clone(), group: "threadtab", parts }
            })
            .filter(|l| !l.parts.is_empty())
            .collect(),
        "core_table" => s
            .core_reads
            .iter()
            .map(|c| {
                let mut parts = vec![];
                if let Some(v) = c.load {
                    let mut p = part(format!("{:>3.0}", v), "%");
                    p.tone = tone("load", v, 0, st);
                    parts.push(p);
                }
                if let Some(v) = c.clock {
                    parts.push(clk(v, st, 4));
                }
                if let Some(v) = c.temp {
                    let mut p = deg(v, st, 3);
                    p.tone = tone("temp", v, st.warn_cpu_temp, st);
                    parts.push(p);
                }
                Line { label: c.name.clone(), group: "coretab", parts }
            })
            .filter(|l| !l.parts.is_empty())
            .collect(),
        _ => vec![],
    }
}

fn is_multi(id: &str) -> bool {
    matches!(id, "cpu_cores" | "core_clock" | "core_temp" | "core_table" | "thread_table")
}

/// The lines for `items` (in the order the user put them) under `st`.
pub fn lines(items: &[String], st: &OsdStyle, s: &Snap, app: &str) -> Vec<Line> {
    let label_of = |g: &str| match g {
        "coreclk" if st.ghz => "GHz",
        "coretmp" if st.fahrenheit => "°F",
        _ => GROUPS.iter().find(|x| x.0 == g).map(|x| x.1).unwrap_or(""),
    };
    let mut seen: Vec<(&String, String, &'static str)> = vec![];
    for id in items {
        if let Some((short, group)) = info(id, s) {
            if !seen.iter().any(|x| x.0 == id) {
                seen.push((id, short, group));
            }
        }
    }
    // a line of the user's own (not for the per-core grids, which take several lines)
    let mine = |id: &str| st.groups.get(id).map(|l| l.trim()).filter(|l| !l.is_empty() && !is_multi(id));
    let key = |id: &str, g: &'static str| mine(id).map(|l| format!("u:{l}")).unwrap_or_else(|| g.to_string());
    let mut out: Vec<Line> = vec![];
    match st.layout.as_str() {
        "list" => {
            for (id, short, group) in &seen {
                if is_multi(id) {
                    out.extend(multi(id, short, s, st));
                } else if let Some(p) = value(id, s, st, app) {
                    out.push(Line { label: if *group == "title" { String::new() } else { short.clone() }, group, parts: vec![p] });
                }
            }
        }
        "line" => {
            let mut parts = vec![];
            let mut last = String::new();
            for (id, _, group) in &seen {
                if is_multi(id) {
                    continue;
                }
                if let Some(mut p) = value(id, s, st, app) {
                    let k = key(id, group);
                    if k != last {
                        p.pre = mine(id).unwrap_or(label_of(group)).to_string();
                        last = k;
                    }
                    parts.push(p);
                }
            }
            if !parts.is_empty() {
                out.push(Line { label: String::new(), group: "info", parts });
            }
        }
        _ => {
            // a line per group, in the order the groups first appear
            let mut order: Vec<String> = vec![];
            for (id, _, g) in &seen {
                let k = key(id, g);
                if !order.contains(&k) {
                    order.push(k);
                }
            }
            for k in order {
                if let Some(label) = k.strip_prefix("u:") {
                    // the user's own line: its items in their order, coloured like the first one's part
                    let mut g0 = None;
                    let parts: Vec<Part> = seen
                        .iter()
                        .filter(|x| mine(x.0) == Some(label))
                        .filter_map(|x| {
                            g0.get_or_insert(x.2);
                            value(x.0, s, st, app)
                        })
                        .collect();
                    if !parts.is_empty() {
                        out.push(Line { label: label.to_string(), group: g0.unwrap_or("info"), parts });
                    }
                    continue;
                }
                let g = seen.iter().map(|x| x.2).find(|g| *g == k.as_str()).unwrap_or("info");
                let seen: Vec<_> = seen.iter().filter(|x| mine(x.0).is_none()).cloned().collect();
                if matches!(g, "cores" | "coreclk" | "coretmp" | "coretab" | "threadtab") {
                    for (id, _, grp) in &seen {
                        if *grp == g {
                            out.extend(multi(id, label_of(g), s, st));
                        }
                    }
                    continue;
                }
                if g == "fps" && st.fps_lines {
                    for (id, short, grp) in &seen {
                        if *grp == g {
                            if let Some(p) = value(id, s, st, app) {
                                out.push(Line { label: short.clone(), group: g, parts: vec![p] });
                            }
                        }
                    }
                    continue;
                }
                let parts: Vec<Part> = seen.iter().filter(|x| x.2 == g).filter_map(|x| value(x.0, s, st, app)).collect();
                if !parts.is_empty() {
                    out.push(Line { label: label_of(g).to_string(), group: g, parts });
                }
            }
        }
    }
    // the modern frametime graph: a line built like the others, so it lines up with them: the live
    // frametime (unless the overlay already shows it) in the first value column, then the graph
    if st.graph && st.graph_look != "classic" && !out.is_empty() {
        let mut parts = vec![];
        if !items.iter().any(|i| i == "frametime") {
            parts.extend(value("frametime", s, st, app));
        }
        parts.push(Part { graph: true, ..Default::default() });
        out.push(Line { label: "FT".into(), group: "fps", parts });
    }
    if st.columns && st.layout != "line" {
        // numbers right-aligned, units left-aligned, one column per position (monospace fonts line up)
        let cols = out.iter().map(|l| l.parts.len()).max().unwrap_or(0);
        for c in 0..cols {
            let w = out.iter().filter_map(|l| l.parts.get(c)).map(|p| p.text.trim().chars().count()).max().unwrap_or(0);
            let uw = out.iter().filter_map(|l| l.parts.get(c)).map(|p| p.unit.chars().count()).max().unwrap_or(0);
            for l in &mut out {
                if let Some(p) = l.parts.get_mut(c) {
                    // a figure RivaTuner fills in itself gets the spaces its current value needs
                    p.text = format!("{:>w$}", p.text.trim(), w = w);
                    if !p.unit.is_empty() {
                        p.unit = format!("{:<uw$}", p.unit, uw = uw);
                    }
                }
            }
        }
    }
    if !st.labels {
        for l in &mut out {
            l.label.clear();
            for p in &mut l.parts {
                p.pre.clear();
            }
        }
    }
    if st.align && st.layout != "line" {
        let w = out.iter().map(|l| l.label.chars().count()).max().unwrap_or(0);
        for l in &mut out {
            if !l.label.is_empty() {
                let pad = w - l.label.chars().count();
                l.label.push_str(&" ".repeat(pad));
            }
        }
    }
    out
}

/// The colour slot of a group: C4 GPU, C5 CPU, C6 memory, C7 framerate, C8 the rest.
fn group_slot(g: &str) -> u8 {
    match g {
        "gpu" | "vram" => 4,
        "cpu" | "cores" | "coreclk" | "coretmp" | "coretab" | "threadtab" => 5,
        "ram" | "app" => 6,
        "fps" => 7,
        _ => 8,
    }
}

fn render_part(p: &Part, g: &str, st: &OsdStyle) -> String {
    if p.graph {
        // the graph object Deck puts at the start of its slot's object buffer (see graph_object),
        // in the line's label colour
        return format!("<C{}><OBJ=00000000><C>", if st.group_colors { group_slot(g) } else { 0 });
    }
    let mut o = String::new();
    let base = if st.group_colors && st.group_values { group_slot(g) } else { 1 };
    if !p.pre.is_empty() {
        o += &format!("<C0>{}<C> ", p.pre);
    }
    let c = match p.tone {
        2 => 2,
        1 => 3,
        _ => base,
    };
    let lead = p.text.len() - p.text.trim_start().len();
    let v = p.rtss.map(|t| format!("{}{t}", " ".repeat(lead))).unwrap_or_else(|| p.text.clone());
    o += &format!("<C{c}>{v}<C>");
    if !p.unit.is_empty() {
        match st.units.as_str() {
            "off" => {}
            "normal" => o += &format!("<C0> {}<C>", p.unit),
            // a small gap (a space at the unit's size) so the unit doesn't touch the number
            _ => o += &format!("<C0><S0> {}<S><C>", p.unit),
        }
    }
    o
}

/// RTSS text: colours C0 labels, C1 numbers, C2 trouble, C3 warm, C4 to C8 the group colours; S0 small units.
pub fn render(ls: &[Line], st: &OsdStyle) -> String {
    if ls.is_empty() {
        return String::new();
    }
    let head = format!(
        "<C0={}><C1={}><C2={}><C3={}><C4={}><C5={}><C6={}><C7={}><C8={}>{}",
        hex(&st.label_color, "FF9A40"),
        hex(&st.value_color, "FFFFFF"),
        hex(&st.warn_color, "FF5A5A"),
        hex(&st.warm_color, "FFC85A"),
        hex(&st.gpu_color, "76B900"),
        hex(&st.cpu_color, "4FA3FF"),
        hex(&st.ram_color, "C77DFF"),
        hex(&st.fps_color, "FFD60A"),
        hex(&st.info_color, "9AA3B5"),
        if st.units == "small" { "<S0=62>" } else { "" }
    );
    let body: Vec<String> = ls
        .iter()
        .map(|l| {
            let mut s = String::new();
            if !l.label.is_empty() {
                let c = if st.group_colors { group_slot(l.group) } else { 0 };
                s += &format!("<C{c}>{}<C>  ", l.label);
            }
            s += &l.parts.iter().map(|p| render_part(p, l.group, st)).collect::<Vec<_>>().join("  ");
            s
        })
        .collect();
    format!("{head}{}", body.join("\n"))
}

#[cfg(test)]
pub fn osd_text(items: &[String], st: &OsdStyle, s: &Snap, app: &str) -> String {
    osd_text_width(items, st, s, app).0
}

/// How many characters the longest line takes (labels, values, units and the gaps between them).
pub fn text_width(ls: &[Line], st: &OsdStyle) -> usize {
    let n = |s: &str| s.chars().count();
    ls.iter()
        .map(|l| {
            let label = if l.label.is_empty() { 0 } else { n(&l.label) + 2 };
            let parts: usize = l
                .parts
                .iter()
                .map(|p| if p.graph { 0 } else { (if p.pre.is_empty() { 0 } else { n(&p.pre) + 1 }) + n(&p.text) + if st.units == "off" || p.unit.is_empty() { 0 } else { n(&p.unit) + 1 } })
                .sum();
            label + parts + 2 * l.parts.len().saturating_sub(1)
        })
        .max()
        .unwrap_or(0)
}

/// The overlay text and the width of its longest line in characters.
pub fn osd_text_width(items: &[String], st: &OsdStyle, s: &Snap, app: &str) -> (String, usize) {
    let ls = lines(items, st, s, app);
    let width = text_width(&ls, st);
    let mut t = render(&ls, st);
    if let Some(gl) = ls.iter().position(|l| l.parts.iter().any(|p| p.graph)) {
        // modern: the graph takes what its line leaves of the overlay's widest other line
        let others: Vec<Line> = ls.iter().enumerate().filter(|(i, _)| *i != gl).map(|(_, l)| l.clone()).collect();
        return (t, text_width(&others, st).saturating_sub(text_width(&ls[gl..=gl], st)));
    }
    if st.graph && !t.is_empty() {
        // the frametime graph object Deck puts at the start of its slot's object buffer (see graph_object)
        let c = if st.group_colors { 7 } else { 0 };
        t += &format!("\n<C{c}><OBJ=00000000><C>");
    }
    (t, width)
}

/// RivaTuner's own frametime graph (RTSS_EMBEDDED_OBJECT_GRAPH with FLAG_FRAMETIME and no data,
/// shared memory v2.12+), the one MSI Afterburner shows: RivaTuner fills it from every frame and
/// redraws it with each one. Width and height are in characters (negative); `width` is the room
/// `osd_text_width` worked out for it, used when the graph fits the overlay. RivaTuner's
/// frametimes are in microseconds (the SDK sample's 0..50000).
pub fn graph_object(st: &OsdStyle, width: usize) -> Vec<u8> {
    const FILLED: u32 = 1;
    const FRAMETIME: u32 = 4;
    const BAR: u32 = 8;
    const AUTOSCALE: u32 = 128;
    let style = match st.graph_style {
        1 => FILLED,
        2 => BAR,
        _ => 0,
    };
    let flags = FRAMETIME | style | if st.graph_max == 0 { AUTOSCALE } else { 0 };
    let top = if st.graph_max > 0 { st.graph_max.min(1000) as f32 * 1000.0 } else { 50_000.0 };
    let modern = st.graph_look != "classic";
    let w = if st.graph_fit && (width > 0 || modern) { width.clamp(if modern { 12 } else { 8 }, 160) as i32 } else { st.graph_w.clamp(8, 128) as i32 };
    let mut b = Vec::with_capacity(36);
    b.extend(0x4752_3030u32.to_le_bytes()); // 'GR00'
    b.extend(36u32.to_le_bytes());
    b.extend((-w).to_le_bytes());
    b.extend((-(st.graph_h.clamp(1, 24) as i32)).to_le_bytes());
    b.extend(1i32.to_le_bytes()); // margin
    b.extend(flags.to_le_bytes());
    b.extend(0f32.to_le_bytes());
    b.extend(top.to_le_bytes());
    b.extend(0u32.to_le_bytes()); // no data: RivaTuner's own frametimes
    b
}

/// For the Sensors page's preview.
pub fn preview(items: &[String], st: &OsdStyle, s: &Snap, app: &str) -> Value {
    let ls = lines(items, st, s, app);
    json!(ls.iter().map(|l| json!({
        "label": l.label,
        "group": l.group,
        "slot": group_slot(l.group),
        "parts": l.parts.iter().map(|p| json!({"t": p.text, "u": if st.units == "off" { "" } else { p.unit.as_str() }, "tone": p.tone, "pre": p.pre, "g": p.graph})).collect::<Vec<_>>(),
    })).collect::<Vec<_>>())
}

// ---------------------------------------------------------------------------- frame statistics

/// The last few seconds of frametimes as RivaTuner streams them.
#[derive(Default)]
pub struct FrameWindow {
    frames: VecDeque<(Instant, f32)>,
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct FrameStats {
    pub avg: Option<f32>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub low1: Option<f32>,
    pub low01: Option<f32>,
}

impl FrameWindow {
    pub const fn new() -> Self {
        Self { frames: VecDeque::new() }
    }

    pub fn push(&mut self, ms: f32, now: Instant, window: Duration) {
        // a "frame" over a second is the game minimized or paused (RivaTuner counts the whole gap as
        // one frame): it would wreck the average and the lows, so the figures start over after it
        if ms > 1000.0 {
            self.frames.clear();
            return;
        }
        if ms.is_finite() && ms > 0.0 {
            self.frames.push_back((now, ms));
        }
        self.prune(now, window);
    }

    /// Drops what is older than `window`.
    pub fn prune(&mut self, now: Instant, window: Duration) {
        while self.frames.front().map(|f| now.duration_since(f.0) > window).unwrap_or(false) {
            self.frames.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// The last `n` frametimes (ms), oldest first.
    pub fn recent(&self, n: usize) -> Vec<f32> {
        self.frames.iter().skip(self.frames.len().saturating_sub(n)).map(|f| f.1).collect()
    }

    /// Lows are the average of the slowest 1 % (0.1 %) of frames, as benchmark tools report them.
    pub fn stats(&self) -> FrameStats {
        let n = self.frames.len();
        if n < 30 {
            return FrameStats::default();
        }
        let mut t: Vec<f32> = self.frames.iter().map(|f| f.1).collect();
        t.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let fps = |ms: f32| 1000.0 / ms;
        let worst = |frac: f32| {
            let k = ((n as f32 * frac).ceil() as usize).clamp(1, n);
            fps(t[..k].iter().sum::<f32>() / k as f32)
        };
        FrameStats {
            avg: Some(fps(t.iter().sum::<f32>() / n as f32)),
            min: Some(fps(t[0])),
            max: Some(fps(t[n - 1])),
            low1: Some(worst(0.01)),
            // a tenth of a percent of a few hundred frames is the single worst frame: wait for more
            low01: (n >= 1000).then(|| worst(0.001)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtss::CoreRead;

    fn snap() -> Snap {
        Snap {
            gpu_temp: Some(64.6),
            gpu_usage: Some(98.0),
            gpu_mem_mb: Some(7000.0),
            gpu_mem_total_mb: Some(8192.0),
            cpu_temp: Some(92.0),
            ram_mb: Some(12288.0),
            fps: Some(143.4),
            frametime: Some(6.9),
            cores: vec![5.0; 10],
            core_reads: vec![
                CoreRead { name: "P-core 1".into(), load: Some(40.0), clock: Some(5300.0), temp: Some(61.0) },
                CoreRead { name: "E-core 1".into(), load: Some(10.0), clock: Some(4200.0), temp: None },
            ],
            thread_reads: vec![
                CoreRead { name: "CPU1".into(), load: Some(49.0), clock: Some(3395.0), temp: None },
                CoreRead { name: "CPU2".into(), load: Some(2.0), clock: Some(3395.0), temp: None },
            ],
            fans: vec![("CPU fan".into(), 1200.0), ("Rear fan".into(), 800.0)],
            board_temps: vec![("VRM".into(), 48.0)],
            ..Default::default()
        }
    }

    fn items(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// The modern graph is a line built like the others: its label padded like theirs, the live
    /// frametime in the first value column, then the graph to the end of the overlay. The graph is
    /// RivaTuner's own frametime graph (MSI Afterburner's): no points from Deck, RivaTuner fills it
    /// from every frame and redraws it with each one, so it moves as smoothly as the game. No box
    /// behind it (RivaTuner tints that box in the graph's colour: a muddy red panel in a game).
    #[test]
    fn the_modern_graph_is_rivatuners_own_on_a_line_like_the_others() {
        let st = OsdStyle { graph: true, units: "normal".into(), ..Default::default() };
        assert_eq!(st.graph_look, "modern", "the default look");
        let (t, gw) = osd_text_width(&items(&["gpu_temp", "gpu_usage", "fps"]), &st, &snap(), "");
        assert_eq!(t.lines().last().unwrap(), "<C0>FT <C>  <C1><FT><C><C0> ms<C>  <C0><OBJ=00000000><C>");
        // the overlay is "GPU  65 °C  98 %" (16) wide: the label and its gap take 5, "6.9 ms" and a gap 8
        assert_eq!(gw, 16 - 5 - 8);
        let b = graph_object(&st, gw);
        assert_eq!(i32::from_le_bytes(b[8..12].try_into().unwrap()), -12, "never narrower than 12 characters");
        assert_eq!(u32::from_le_bytes(b[20..24].try_into().unwrap()), 4 | 128, "RivaTuner's frametimes (FLAG_FRAMETIME), autoscaled, a line");
        assert_eq!(u32::from_le_bytes(b[32..36].try_into().unwrap()), 0, "no points of Deck's");
        assert_eq!(b.len(), 36);
        let classic = graph_object(&OsdStyle { graph_look: "classic".into(), ..st.clone() }, gw);
        assert_eq!(u32::from_le_bytes(classic[20..24].try_into().unwrap()), 4 | 128, "classic: the same graph, under the text");
        let filled = graph_object(&OsdStyle { graph_style: 1, ..st.clone() }, gw);
        assert_eq!(u32::from_le_bytes(filled[20..24].try_into().unwrap()), 4 | 128 | 1);
        let bars = graph_object(&OsdStyle { graph_style: 2, ..st.clone() }, gw);
        assert_eq!(u32::from_le_bytes(bars[20..24].try_into().unwrap()), 4 | 128 | 8);
        // a top the user set: RivaTuner's frametimes are in microseconds (the SDK sample's 0..50000)
        let fixed = graph_object(&OsdStyle { graph_max: 25, ..st }, gw);
        assert_eq!(u32::from_le_bytes(fixed[20..24].try_into().unwrap()), 4);
        assert_eq!(f32::from_le_bytes(fixed[28..32].try_into().unwrap()), 25_000.0);
    }

    /// When the frametime is already on the overlay (the FPS line), the graph line doesn't repeat
    /// it: the graph starts in the first value column.
    #[test]
    fn the_frametime_is_shown_once() {
        let st = OsdStyle { graph: true, units: "normal".into(), ..Default::default() };
        let (t, gw) = osd_text_width(&items(&["gpu_temp", "gpu_usage", "fps", "frametime"]), &st, &snap(), "");
        assert_eq!(t.lines().last().unwrap(), "<C0>FT <C>  <C0><OBJ=00000000><C>");
        assert_eq!(t.matches("<FT>").count(), 1);
        assert!(gw >= 11, "the line's room after the label");
        let gc = osd_text(&items(&["fps"]), &OsdStyle { graph: true, group_colors: true, ..Default::default() }, &snap(), "");
        assert!(gc.lines().last().unwrap().starts_with("<C7>FT"), "the framerate's colour when parts have colours");
        assert!(gc.lines().last().unwrap().ends_with("<C7><OBJ=00000000><C>"));
        let bare = osd_text(&items(&["fps"]), &OsdStyle { graph: true, labels: false, ..Default::default() }, &snap(), "");
        assert!(bare.lines().last().unwrap().starts_with("<C1><FT>"), "no labels: no label in front of the graph line");
        let prev = preview(&items(&["fps"]), &st, &snap(), "");
        assert_eq!(prev.as_array().unwrap().last().unwrap()["parts"].as_array().unwrap().last().unwrap()["g"], true, "the preview knows where the graph goes");
    }

    #[test]
    fn frametime_graph_is_an_embedded_object() {
        let mut st = OsdStyle { graph: true, graph_w: 48, graph_h: 6, graph_fit: false, graph_look: "classic".into(), ..Default::default() };
        let (t, width) = osd_text_width(&items(&["gpu_temp", "gpu_usage", "fps"]), &st, &snap(), "");
        assert!(t.ends_with("<OBJ=00000000><C>"));
        let b = graph_object(&st, width);
        assert_eq!(&b[0..4], b"00RG", "'GR00' as a C multi-char constant");
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize, b.len());
        assert_eq!(i32::from_le_bytes(b[8..12].try_into().unwrap()), -48);
        assert_eq!(i32::from_le_bytes(b[12..16].try_into().unwrap()), -6);
        assert_eq!(u32::from_le_bytes(b[20..24].try_into().unwrap()), 4 | 128, "RivaTuner's frametimes, autoscaled");
        assert_eq!(u32::from_le_bytes(b[32..36].try_into().unwrap()), 0);
        // fitted: as wide as "GPU  65 °C  98 %" (the longest line; a unit and its gap count as characters)
        st.graph_fit = true;
        let b = graph_object(&st, width);
        assert_eq!(i32::from_le_bytes(b[8..12].try_into().unwrap()), -(width as i32));
        assert_eq!(width, "GPU  65 °C  98 %".chars().count(), "units count as characters");
        st.graph_style = 1;
        st.graph_max = 20;
        let b = graph_object(&st, width);
        assert_eq!(u32::from_le_bytes(b[20..24].try_into().unwrap()), 4 | 1);
        assert_eq!(f32::from_le_bytes(b[28..32].try_into().unwrap()), 20_000.0);
        st.graph = false;
        assert!(!osd_text(&items(&["fps"]), &st, &snap(), "").contains("<OBJ"));
    }

    #[test]
    fn a_pause_starts_the_figures_over() {
        let (now, win) = (Instant::now(), Duration::from_secs(60));
        let mut w = FrameWindow::new();
        for _ in 0..100 {
            w.push(1.0, now, win);
        }
        w.push(262_000.0, now, win); // back from being minimized
        assert_eq!(w.len(), 0);
        for _ in 0..100 {
            w.push(2.0, now, win);
        }
        let s = w.stats();
        assert_eq!(s.avg.map(|v| v.round()), Some(500.0));
        assert_eq!(s.min.map(|v| v.round()), Some(500.0), "the gap is not a 0.004 fps frame");
    }

    #[test]
    fn game_ram_shares_the_ram_line() {
        let s = Snap { proc_ram_mb: Some(5400.0), ..snap() };
        let ls = lines(&items(&["ram", "gpu_temp", "proc_ram"]), &OsdStyle::default(), &s, "");
        let ram = ls.iter().find(|l| l.label.trim() == "RAM").expect("a RAM line");
        assert_eq!(ram.parts.len(), 2, "{ls:?}");
        assert!(!ls.iter().any(|l| l.label.trim() == "APP"));
    }

    #[test]
    fn items_put_on_a_line_of_their_own() {
        let mut st = OsdStyle { align: false, ..Default::default() };
        st.groups.insert("gpu_temp".into(), "TEMPS".into());
        st.groups.insert("cpu_temp".into(), "TEMPS".into());
        st.groups.insert("cpu_cores".into(), "TEMPS".into()); // the grids can't join a line
        let ls = lines(&items(&["gpu_usage", "gpu_temp", "ram", "cpu_temp", "fps", "cpu_cores"]), &st, &snap(), "");
        let labels: Vec<_> = ls.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(&labels[..4], ["GPU", "TEMPS", "RAM", "FPS"], "{labels:?}");
        assert_eq!(ls[1].parts.iter().map(|p| p.unit.as_str()).collect::<Vec<_>>(), ["°C", "°C"]);
        assert_eq!(ls[1].group, "gpu", "coloured like its first item");
        assert_eq!(ls[0].parts.len(), 1, "the GPU line keeps only what wasn't moved");
        assert!(ls.iter().any(|l| l.group == "cores"));
        // one line: the label changes where the user's line starts
        st.layout = "line".into();
        let l = &lines(&items(&["gpu_usage", "gpu_temp", "cpu_temp", "ram"]), &st, &snap(), "")[0];
        assert_eq!(l.parts.iter().map(|p| p.pre.as_str()).collect::<Vec<_>>(), ["GPU", "TEMPS", "", "RAM"]);
        // a blank label means no line of its own
        st.layout = "grouped".into();
        st.groups.insert("gpu_temp".into(), "  ".into());
        let ls = lines(&items(&["gpu_usage", "gpu_temp"]), &st, &snap(), "");
        assert_eq!(ls.len(), 1);
    }

    #[test]
    fn grouped_follows_the_order_and_aligns_labels() {
        let st = OsdStyle::default();
        let ls = lines(&items(&["cpu_temp", "gpu_temp", "gpu_usage", "ram", "fps", "frametime"]), &st, &snap(), "game.exe");
        let labels: Vec<&str> = ls.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, ["CPU", "GPU", "RAM", "FPS"], "CPU was listed first, so its line is first");
        assert_eq!(ls[1].parts.len(), 2);
        let t = render(&ls, &st);
        assert!(t.starts_with("<C0=FF9A40><C1=FFFFFF><C2=FF5A5A><C3=FFC85A>"));
        assert!(t.contains("<C0>GPU<C>  <C1>65<C><C0><S0> °C<S><C>  <C1>98<C><C0><S0> %<S><C>"));
        assert!(t.contains("<C2>92<C>"), "92 °C is over the 90 limit");
        assert!(t.contains("<C1><FR><C><C0><S0> FPS<S><C>"));
    }

    #[test]
    fn list_layout_names_each_item() {
        let st = OsdStyle { layout: "list".into(), ..Default::default() };
        let ls = lines(&items(&["gpu_temp", "cpu_cores", "fps"]), &st, &snap(), "");
        assert_eq!(ls[0].label.trim_end(), "GPU temp");
        assert_eq!(ls.iter().filter(|l| l.label.trim() == "Thread %").count(), 1);
        assert_eq!(ls[1].parts.len(), 8, "10 threads wrap after 8");
        assert_eq!(ls[2].parts.len(), 2);
        assert_eq!(ls.len(), 4);
    }

    #[test]
    fn per_core_clock_temperature_and_table() {
        let st = OsdStyle::default();
        let ls = lines(&items(&["core_clock", "core_temp"]), &st, &snap(), "");
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].parts.len(), 2, "both cores have a clock");
        assert_eq!(ls[1].parts.len(), 1, "only one has a temperature");
        let ls = lines(&items(&["core_table"]), &st, &snap(), "");
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].label.trim_end(), "P-core 1");
        assert_eq!(ls[0].parts.iter().map(|p| p.unit.as_str()).collect::<Vec<_>>(), ["%", "MHz", "°C"]);
        assert_eq!(ls[1].parts.len(), 2, "no temperature on the E-core");
    }

    #[test]
    fn app_units_ghz_fahrenheit_and_network_bits() {
        let s = Snap { cpu_clock: Some(4800.0), cpu_temp: Some(50.0), net_down: Some(1.0), ..snap() };
        let st = OsdStyle { layout: "list".into(), ghz: true, fahrenheit: true, ..Default::default() };
        let ls = lines(&items(&["cpu_clock", "cpu_temp", "net_down", "core_table"]), &st, &s, "");
        let part = |id: usize| (ls[id].parts[0].text.trim().to_string(), ls[id].parts[0].unit.clone());
        assert_eq!(part(0), ("4.80".into(), "GHz".into()));
        assert_eq!(part(1), ("122".into(), "°F".into()));
        assert_eq!(part(2), ("8.4".into(), "Mb/s".into()), "1 MB/s (1024 KB) = 8.4 megabits");
        assert!(ls[3..].iter().flat_map(|l| &l.parts).all(|p| p.unit != "MHz" && p.unit != "°C"));
        let st = OsdStyle { net_bytes: true, ..st };
        assert_eq!(lines(&items(&["net_down"]), &st, &s, "")[0].parts[0].unit, "MB/s");
        // warnings still compare in °C: 50 °C is under a 90 °C limit
        assert_eq!(lines(&items(&["cpu_temp"]), &st, &s, "")[0].parts[0].tone, 0);
    }

    #[test]
    fn this_pcs_own_fans_and_sensors() {
        let st = OsdStyle { layout: "list".into(), ..Default::default() };
        let ls = lines(&items(&["fan:1", "temp:0", "fan:9"]), &st, &snap(), "");
        assert_eq!(ls.len(), 2, "a fan this PC doesn't have is skipped");
        assert_eq!(ls[0].label.trim_end(), "Rear fan");
        assert_eq!(ls[0].parts[0].unit, "RPM");
        let cat = catalogue(&snap());
        assert!(cat.iter().any(|c| c.0 == "fan:0" && c.2 == "CPU fan") && cat.iter().any(|c| c.0 == "temp:0"));
        assert!(cat.iter().find(|c| c.0 == "gpu_volt").map(|c| !c.5).unwrap(), "no voltage reading on this snapshot");
    }

    #[test]
    fn one_line_layout_and_options() {
        let st = OsdStyle { layout: "line".into(), units: "off".into(), labels: false, ..Default::default() };
        let t = osd_text(&items(&["gpu_temp", "gpu_usage", "fps"]), &st, &snap(), "");
        assert_eq!(t.lines().count(), 1);
        assert!(!t.contains("<S0>") && !t.contains("GPU"));
        let st = OsdStyle { mem_unit: "GB".into(), ..Default::default() };
        let t = osd_text(&items(&["gpu_mem"]), &st, &snap(), "");
        assert!(t.contains("<C1>6.8<C>") && t.contains("GB"));
        let st = OsdStyle { warn_fps: 144, ..Default::default() };
        assert!(osd_text(&items(&["fps"]), &st, &snap(), "").contains("<C2><FR><C>"), "143 is under 144");
        assert_eq!(osd_text(&[], &st, &snap(), ""), "");
        assert!(osd_text(&items(&["gpu_temp", "app", "res", "time"]), &OsdStyle::default(), &snap(), "cs2.exe").contains("<APP>"));
    }

    /// The preview shows the game's own API and resolution (RivaTuner fills the overlay's in);
    /// with no game it shows a dash, not the desktop's size or a made-up "D3D12".
    #[test]
    fn the_preview_never_makes_up_the_api_or_resolution() {
        let st = OsdStyle { layout: "list".into(), ..Default::default() };
        let text = |s: &Snap| lines(&items(&["api", "res"]), &st, s, "").iter().map(|l| l.parts[0].text.clone()).collect::<Vec<_>>();
        assert_eq!(text(&snap()), vec!["–", "–"]);
        let s = Snap { game_api: Some("D3D11"), game_res: Some((1920, 1080)), ..snap() };
        assert_eq!(text(&s), vec!["D3D11", "1920x1080"]);
        let t = osd_text(&items(&["api", "res"]), &st, &s, "");
        assert!(t.contains("<API>") && t.contains("<RES>"), "the overlay itself still gets RivaTuner's tags");
    }

    #[test]
    fn colours_by_group_and_by_heat() {
        let st = OsdStyle { group_colors: true, group_values: true, heat: true, ..Default::default() };
        let t = osd_text(&items(&["gpu_temp", "gpu_usage", "ram", "gpu_mem_pct"]), &st, &snap(), "");
        assert!(t.contains("<C4>GPU<C>") && t.contains("<C6>RAM<C>"), "{t}");
        assert!(t.contains("<C2>98<C>"), "98 % load is hot: {t}");
        assert!(t.contains("<C3>85<C>"), "7000 of 8192 MB is warm: {t}");
        assert!(t.contains("<C4>65<C>"), "a calm temperature keeps the group colour");
    }

    #[test]
    fn afterburner_style_lines() {
        let st = OsdStyle { fps_lines: true, columns: true, ..Default::default() };
        let ls = lines(&items(&["cpu_temp", "cpu_usage", "thread_table", "fps", "fps_min", "fps_avg", "gpu_temp", "gpu_usage", "gpu_mem"]), &st, &snap(), "");
        let labels: Vec<String> = ls.iter().map(|l| l.label.trim().to_string()).collect();
        assert_eq!(labels, ["CPU", "CPU1", "CPU2", "FPS", "FPS Min", "FPS Avg", "GPU", "MEM"], "{labels:?}");
        let w = ls[1].parts[0].text.chars().count();
        assert!(ls.iter().flat_map(|l| l.parts.first()).filter(|p| p.rtss.is_none()).all(|p| p.text.chars().count() == w), "numbers share a column width");
        assert_eq!(ls[1].parts[0].text.trim(), "49");
        assert_eq!(ls[0].parts[0].text.trim(), "92");
        let text = render(&ls, &st);
        assert!(text.contains("<C0>CPU1"), "{text}");
    }

    #[test]
    fn the_clock() {
        let st = OsdStyle { clock_12h: true, clock_secs: true, ..Default::default() };
        let c = clock(&st);
        assert!(c.ends_with(" AM") || c.ends_with(" PM"));
        assert_eq!(c.matches(':').count(), 2);
    }

    #[test]
    fn bad_colours_fall_back() {
        let st = OsdStyle { label_color: "red".into(), value_color: "#00ff00".into(), ..Default::default() };
        let t = osd_text(&items(&["gpu_temp"]), &st, &snap(), "");
        assert!(t.starts_with("<C0=FF9A40><C1=00FF00>"));
    }

    #[test]
    fn an_old_saved_style_still_loads() {
        let st: OsdStyle = serde_json::from_str(r#"{"corner":"bl","bgnd":false}"#).unwrap();
        assert_eq!(st.corner, "bl");
        assert!(!st.shadow, "the old name for the text shadow");
        assert_eq!(st.margin_x, 1);
    }

    #[test]
    fn lows_come_from_the_slowest_frames() {
        let mut w = FrameWindow::default();
        let t0 = Instant::now();
        for i in 0..1000 {
            w.push(if i % 100 == 0 { 40.0 } else { 10.0 }, t0, Duration::from_secs(60));
        }
        let s = w.stats();
        assert!((s.max.unwrap() - 100.0).abs() < 0.01 && (s.min.unwrap() - 25.0).abs() < 0.01);
        assert!((s.low1.unwrap() - 25.0).abs() < 0.01, "the slowest 10 frames are all 40 ms");
        assert!((s.low01.unwrap() - 25.0).abs() < 0.01);
        assert!(s.avg.unwrap() < 100.0 && s.avg.unwrap() > 70.0);
        let mut few = FrameWindow::default();
        for _ in 0..10 {
            few.push(10.0, t0, Duration::from_secs(60));
        }
        assert_eq!(few.stats(), FrameStats::default(), "not enough frames to say anything");
        w.push(10.0, t0 + Duration::from_secs(61), Duration::from_secs(60));
        assert_eq!(w.len(), 1, "older frames fall out of the window");
    }
}
