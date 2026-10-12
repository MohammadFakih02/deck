//! Hardware settings (fans, lighting, cooler display, alerts): settings.ini / curves.ini in
//! %APPDATA%\Deck\hardware, the same formats MoboCtl used so its files can be imported as-is.
use crate::hw::fx::Rgb;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn dir() -> PathBuf {
    let d = crate::settings::Settings::dir().join("hardware");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// MoboCtl's install folder, if its settings are still there (for the one-time import).
pub fn moboctl_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into())).join("MoboCtl");
    d.join("settings.ini").exists().then_some(d)
}

/// MoboCtl keys that are not carried over: its CPU limits and profiles (CPU profiles replace
/// them) and its window state.
fn import_skip(k: &str) -> bool {
    k.starts_with("Cpu") || k.starts_with("Plan") || k.starts_with("Profile") || k == "PowerMode" || k == "PowerTouched" || k == "Page" || k == "EditCurve"
}

/// Copies MoboCtl's fan, lighting, display and alert settings and its fan curves over Deck's
/// (Deck's own keys stay). Returns what was imported.
pub fn import_moboctl() -> Result<String, String> {
    let src = moboctl_dir().ok_or("MoboCtl's settings were not found.")?;
    let txt = std::fs::read_to_string(src.join("settings.ini")).map_err(|e| e.to_string())?;
    let cur = Settings::load();
    let mut n = 0;
    for l in txt.lines() {
        if let Some((k, v)) = l.split_once('=') {
            if !k.is_empty() && !import_skip(k) {
                cur.set(k, v);
                n += 1;
            }
        }
    }
    cur.set_bool("MoboImported", true);
    cur.save();
    let mut n_curves = 0;
    if let Ok(c) = std::fs::read_to_string(src.join("curves.ini")) {
        n_curves = c.lines().filter(|l| l.contains('|')).count();
        if n_curves > 0 {
            std::fs::write(dir().join("curves.ini"), c).map_err(|e| e.to_string())?;
        }
    }
    Ok(format!("Imported {} settings and {} fan curves from MoboCtl.", n, n_curves))
}

/// At engine start: imports MoboCtl's settings once, unless the hardware pages were already
/// used in Deck (their Apply sets UserApplied).
pub fn auto_import() -> Option<String> {
    moboctl_dir()?;
    let cur = Settings::load();
    if cur.bool("MoboImported", false) || cur.bool("UserApplied", false) {
        return None;
    }
    import_moboctl().ok()
}

pub struct Settings {
    d: Mutex<BTreeMap<String, String>>,
    order: Mutex<Vec<String>>,
}

impl Settings {
    pub fn load() -> Settings {
        let s = Settings { d: Mutex::new(BTreeMap::new()), order: Mutex::new(vec![]) };
        if let Ok(txt) = std::fs::read_to_string(dir().join("settings.ini")) {
            for l in txt.lines() {
                if let Some(i) = l.find('=') {
                    if i > 0 {
                        s.set(&l[..i], &l[i + 1..]);
                    }
                }
            }
        }
        s
    }

    pub fn save(&self) {
        static SAVING: Mutex<()> = Mutex::new(());
        let _one_at_a_time = SAVING.lock().unwrap_or_else(|e| e.into_inner());
        let lines: Vec<String> = {
            let d = self.d.lock().unwrap();
            self.order.lock().unwrap().iter().filter_map(|k| d.get(k).map(|v| format!("{}={}", k, v))).collect()
        };
        let p = dir().join("settings.ini");
        let tmp = dir().join("settings.ini.tmp");
        if std::fs::write(&tmp, lines.join("\r\n") + "\r\n").is_ok() && std::fs::rename(&tmp, &p).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Every key/value, in file order.
    pub fn all(&self) -> Vec<(String, String)> {
        let d = self.d.lock().unwrap();
        self.order.lock().unwrap().iter().filter_map(|k| d.get(k).map(|v| (k.clone(), v.clone()))).collect()
    }
    /// Puts back a snapshot taken with `all()` (keys added since are removed again).
    pub fn restore(&self, snap: &[(String, String)]) {
        let mut d = self.d.lock().unwrap();
        let mut o = self.order.lock().unwrap();
        d.clear();
        o.clear();
        for (k, v) in snap {
            d.insert(k.clone(), v.clone());
            o.push(k.clone());
        }
    }

    pub fn get(&self, k: &str, def: &str) -> String {
        self.d.lock().unwrap().get(k).cloned().unwrap_or_else(|| def.to_string())
    }
    pub fn int(&self, k: &str, def: i32) -> i32 {
        self.d.lock().unwrap().get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(def)
    }
    pub fn bool(&self, k: &str, def: bool) -> bool {
        self.d.lock().unwrap().get(k).map(|v| v == "1").unwrap_or(def)
    }
    pub fn col(&self, k: &str, def: Rgb) -> Rgb {
        self.d.lock().unwrap().get(k).and_then(|v| Rgb::parse(v)).unwrap_or(def)
    }
    pub fn set(&self, k: &str, v: &str) {
        let mut d = self.d.lock().unwrap();
        if d.insert(k.to_string(), v.to_string()).is_none() {
            self.order.lock().unwrap().push(k.to_string());
        }
    }
    pub fn set_int(&self, k: &str, v: i32) {
        self.set(k, &v.to_string());
    }
    pub fn set_bool(&self, k: &str, v: bool) {
        self.set(k, if v { "1" } else { "0" });
    }
    pub fn set_col(&self, k: &str, c: Rgb) {
        self.set(k, &c.hex());
    }
}

// ---------------------------------------------------------------- fan curves
#[derive(Clone, Debug)]
pub struct Curve {
    pub name: String,
    pub pts: Vec<(f32, f32)>, // temperature C, fan %
}

impl Curve {
    pub fn eval(&self, t: f32) -> f32 {
        if self.pts.is_empty() {
            return 100.0;
        }
        let mut p = self.pts.clone();
        p.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        if t <= p[0].0 {
            return p[0].1;
        }
        for i in 1..p.len() {
            if t <= p[i].0 {
                let dx = p[i].0 - p[i - 1].0;
                return if dx <= 0.0 { p[i].1 } else { p[i - 1].1 + (p[i].1 - p[i - 1].1) * (t - p[i - 1].0) / dx };
            }
        }
        p[p.len() - 1].1
    }

    pub fn defaults() -> Vec<Curve> {
        let mk = |n: &str, v: &[(f32, f32)]| Curve { name: n.into(), pts: v.to_vec() };
        vec![
            mk("Silent", &[(30., 25.), (50., 30.), (70., 45.), (85., 70.), (95., 100.)]),
            mk("Balanced", &[(30., 30.), (50., 40.), (65., 60.), (80., 85.), (90., 100.)]),
            mk("Performance", &[(30., 40.), (45., 55.), (60., 75.), (75., 100.), (100., 100.)]),
        ]
    }

    pub fn load_all() -> Vec<Curve> {
        let mut list = vec![];
        if let Ok(txt) = std::fs::read_to_string(dir().join("curves.ini")) {
            for l in txt.lines() {
                let Some(i) = l.find('|') else { continue };
                if i == 0 {
                    continue;
                }
                let mut c = Curve { name: l[..i].to_string(), pts: vec![] };
                for pr in l[i + 1..].split(',') {
                    let mut it = pr.split(':');
                    if let (Some(x), Some(y)) = (it.next().and_then(|v| v.trim().parse().ok()), it.next().and_then(|v| v.trim().parse().ok())) {
                        c.pts.push((x, y));
                    }
                }
                if c.pts.len() >= 2 {
                    list.push(c);
                }
            }
        }
        if list.is_empty() {
            list = Curve::defaults();
            Curve::save_all(&list);
        }
        list
    }

    pub fn save_all(list: &[Curve]) {
        let lines: Vec<String> = list
            .iter()
            .map(|c| {
                let mut p = c.pts.clone();
                p.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                format!("{}|{}", c.name, p.iter().map(|(x, y)| format!("{}:{}", *x as i32, *y as i32)).collect::<Vec<_>>().join(","))
            })
            .collect();
        let (p, tmp) = (dir().join("curves.ini"), dir().join("curves.ini.tmp"));
        if std::fs::write(&tmp, lines.join("\r\n") + "\r\n").is_ok() && std::fs::rename(&tmp, &p).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}
