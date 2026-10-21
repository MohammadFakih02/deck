//! Applies GPU profiles and keeps them applied: a profile's power limit, clocks (through the
//! voltage/frequency table, like Afterburner), voltage boost, temperature limit and fan. A
//! background loop runs fan curves, notices a driver reset (which drops everything back to
//! stock) and re-applies once, and records what is applied for the crash guard.

use super::curve::{self, Curve};
use super::nvapi::NvApi;
use super::nvml::{self, Nvml};
use crate::profiles::GpuProfile;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Serialize, Debug, Default)]
pub struct Caps {
    pub name: String,
    pub driver: String,
    pub pci: String,
    /// watts: min, max, default
    pub power: (u32, u32, u32),
    /// % of default the slider allows
    pub power_pct: (u32, u32),
    pub core: (i32, i32),
    pub mem: (i32, i32),
    pub fans: u32,
    pub fan_min: u32,
    /// temperature limit range °C (None = not adjustable)
    pub temp: Option<(u32, u32)>,
    pub curve: bool,
    pub volt: bool,
}

/// What is applied right now (read from the card).
#[derive(Clone, Serialize, Debug, Default)]
pub struct Current {
    pub power_w: f32,
    pub power_pct: u32,
    /// uniform core offset, or None when the curve is shaped
    pub core_mhz: Option<i32>,
    pub mem_mhz: i32,
    pub volt_pct: u32,
    pub temp_limit: Option<u32>,
}

struct State {
    /// the profile last applied, to re-apply after a driver reset
    applied: Option<GpuProfile>,
    /// fan curve the loop follows (None = not driving the fans by curve)
    fan_curve: Option<Vec<(f32, f32)>>,
    fan_fixed: Option<u32>,
    fan_last: Option<(u32, Instant)>,
    fan_used_t: Option<(f32, Instant)>,
    reset_reapplied: Option<Instant>,
    /// the curve Deck last wrote (to notice a driver reset)
    expected: Option<Curve>,
    /// (mV, stock MHz) per core point, seen while the card was at stock
    stock_curve: Option<Vec<(f32, f32)>>,
    /// the card's temperature limit before a profile first changed it
    temp_before: Option<u32>,
}

pub struct Gpu {
    pub nvml: &'static Nvml,
    pub api: Option<NvApi>,
    pub caps: Caps,
    /// the card's own temperature target, when known (a profile without a limit goes back to it)
    temp_default: Option<u32>,
    st: Mutex<State>,
}

unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

/// What a profile apply did.
#[derive(Debug, Clone, PartialEq)]
pub enum Applied {
    /// every field is on the card (a one-line summary)
    All(String),
    /// some fields were refused (which and why); the rest is on the card
    Partly(String),
    /// nothing reached the card (why)
    Nothing(String),
}

impl Applied {
    /// The refused fields, if any.
    pub fn error(&self) -> Option<String> {
        match self {
            Applied::All(_) => None,
            Applied::Partly(e) | Applied::Nothing(e) => Some(e.clone()),
        }
    }
}

#[derive(Clone, Serialize, Debug)]
pub struct Field {
    pub what: String,
    pub ok: bool,
    pub note: String,
}

impl Gpu {
    pub fn new() -> Option<Arc<Gpu>> {
        let nvml = Nvml::shared()?;
        let api = NvApi::load();
        let mut caps = Caps { name: nvml.name.clone(), driver: nvml.driver.clone(), pci: nvml.pci_id().unwrap_or_default(), ..Default::default() };
        if let Ok((_, lo, hi, def)) = nvml.power_limit() {
            caps.power = (lo / 1000, hi / 1000, def / 1000);
            caps.power_pct = ((lo * 100).div_ceil(def.max(1)), hi * 100 / def.max(1));
        }
        caps.core = nvml.offset(nvml::CLOCK_GRAPHICS).map(|o| (o.1, o.2)).unwrap_or((-500, 500));
        caps.mem = nvml.offset(nvml::CLOCK_MEM).map(|o| (o.1.max(-1000), o.2.min(2000))).unwrap_or((-1000, 1500));
        caps.fans = nvml.fans();
        caps.fan_min = nvml.fan_range().map(|r| r.0).unwrap_or(30);
        if let (Ok(lo), Ok(hi)) = (nvml.temp_threshold(nvml::TEMP_ACOUSTIC_MIN), nvml.temp_threshold(nvml::TEMP_ACOUSTIC_MAX)) {
            if hi > lo {
                caps.temp = Some((lo, hi));
            }
        }
        // a clock lock left behind by a Deck that was killed (it would keep the card from idling)
        if !super::afterburner_running() {
            let _ = nvml.unlock_clocks();
        }
        if let Some(a) = &api {
            caps.curve = curve::read(a).map(|c| !c.core.is_empty()).unwrap_or(false);
            caps.volt = a.volt_boost().is_ok();
        }
        let temp_default = caps.temp.and(crate::profiles::afterburner::default_temp_saved(&caps.pci));
        crate::log::info(format!(
            "gpu: {} · driver {} · power {}-{} W · {} fan(s) · NvAPI {} · curve editing {}",
            caps.name, caps.driver, caps.power.0, caps.power.1, caps.fans,
            if api.is_some() { "loaded" } else { "not available" },
            if caps.curve { "yes" } else { "no" }
        ));
        Some(Arc::new(Gpu {
            nvml,
            api,
            caps,
            temp_default,
            st: Mutex::new(State { applied: None, fan_curve: None, fan_fixed: None, fan_last: None, fan_used_t: None, reset_reapplied: None, expected: None, stock_curve: None, temp_before: None }),
        }))
    }

    /// The curve now, with each point's real stock clock: remembered from a time the card was
    /// at stock (per card and driver), else reconstructed.
    pub fn curve(&self) -> Option<Curve> {
        let mut c = self.api.as_ref().and_then(|a| curve::read(a).ok())?;
        let key = format!("{}|{}", self.caps.pci, self.caps.driver);
        let file = crate::settings::Settings::dir().join("gpu-stock-curve.json");
        if curve::at_stock(&c) {
            let pts: Vec<(f32, f32)> = c.core.iter().map(|p| (p.mv, p.base_mhz)).collect();
            let mut st = self.st.lock();
            if st.stock_curve.as_ref().map(|s| s != &pts).unwrap_or(true) {
                let _ = std::fs::write(&file, serde_json::to_string(&json!({ "key": key, "pts": pts })).unwrap_or_default());
                st.stock_curve = Some(pts);
            }
            return Some(c);
        }
        let known = {
            let mut st = self.st.lock();
            if st.stock_curve.is_none() {
                st.stock_curve = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(|v| v["key"] == key.as_str()).and_then(|v| serde_json::from_value(v["pts"].clone()).ok());
            }
            st.stock_curve.clone()
        };
        match known {
            Some(pts) if pts.len() == c.core.len() => {
                for (p, (mv, base)) in c.core.iter_mut().zip(pts) {
                    if (p.mv - mv).abs() < 1.0 {
                        p.base_mhz = base;
                    }
                }
            }
            _ => curve::repair_base(&mut c),
        }
        Some(c)
    }

    pub fn current(&self) -> Current {
        let mut c = Current::default();
        if let Ok((cur, _, _, def)) = self.nvml.power_limit() {
            c.power_w = cur as f32 / 1000.0;
            c.power_pct = ((cur as f32 / def.max(1) as f32) * 100.0).round() as u32;
        }
        if let Some(cv) = self.curve() {
            c.core_mhz = curve::uniform_offset(&cv).map(|v| v.round() as i32);
            c.mem_mhz = cv.mem.iter().find(|p| p.editable).map(|p| p.delta_mhz.round() as i32).unwrap_or(0);
        }
        c.volt_pct = self.api.as_ref().and_then(|a| a.volt_boost().ok()).unwrap_or(0);
        c.temp_limit = self.nvml.temp_threshold(nvml::TEMP_ACOUSTIC_CURR).ok();
        c
    }

    pub fn live(&self) -> Value {
        let l = self.nvml.live();
        json!({
            "core_mhz": l.core_mhz, "mem_mhz": l.mem_mhz, "power_w": l.power_w, "temp_c": l.temp_c, "util": l.util,
            "mem_used_mb": l.mem_used_mb, "mem_total_mb": l.mem_total_mb, "fan_pct": l.fan_pct, "limit": l.limit,
            "voltage_mv": self.api.as_ref().and_then(|a| a.voltage_mv()),
            "fan_manual": super::FAN_MANUAL.load(SeqCst),
        })
    }

    /// The profile last applied (by id), if Deck applied one this session.
    pub fn applied_id(&self) -> Option<String> {
        self.st.lock().applied.as_ref().map(|p| p.id.clone())
    }

    /// Applies a profile field by field and reads each back.
    pub fn apply(&self, p: &GpuProfile) -> Applied {
        let fields = self.apply_fields(p);
        let failed: Vec<&Field> = fields.iter().filter(|f| !f.ok).collect();
        let why = || failed.iter().map(|f| format!("{}: {}", f.what, f.note)).collect::<Vec<_>>().join(" · ");
        if !fields.is_empty() && failed.len() == fields.len() {
            // nothing reached the card (no admin rights, driver refused): not the profile on it
            return Applied::Nothing(why());
        }
        {
            let mut st = self.st.lock();
            st.applied = Some(p.clone());
            st.reset_reapplied = None;
        }
        super::guard::record(p, failed.is_empty());
        if failed.is_empty() {
            Applied::All(crate::profiles::gpu_summary(p))
        } else {
            Applied::Partly(why())
        }
    }

    pub fn apply_fields(&self, p: &GpuProfile) -> Vec<Field> {
        let mut out = vec![];
        let mut push = |what: &str, r: Result<String, String>| match r {
            Ok(n) => out.push(Field { what: what.into(), ok: true, note: n }),
            Err(e) => out.push(Field { what: what.into(), ok: false, note: e }),
        };
        let e = |c: i32| nvml::err_text(c).to_string();
        let mut clocks_written = false;

        // power limit
        if let Ok((cur, lo, hi, def)) = self.nvml.power_limit() {
            let want = ((def as u64 * p.power_pct as u64 / 100) as u32).clamp(lo, hi);
            if (want as i64 - cur as i64).abs() > 500 {
                push("Power limit", self.nvml.set_power_limit(want).map_err(e).and_then(|_| {
                    let back = self.nvml.power_limit().map(|x| x.0).unwrap_or(0);
                    if (back as i64 - want as i64).abs() <= 1000 { Ok(format!("{} W", want / 1000)) } else { Err(format!("asked {} W, card says {} W", want / 1000, back / 1000)) }
                }));
            }
        }

        // temperature limit: a profile's own, else whatever the card had before Deck changed it, else
        // the card's default (a limit Afterburner left behind would otherwise stay forever)
        let cur_t = self.nvml.temp_threshold(nvml::TEMP_ACOUSTIC_CURR).ok();
        let want_t = match p.temp_limit {
            Some(t) => {
                let mut st = self.st.lock();
                if st.temp_before.is_none() {
                    st.temp_before = cur_t;
                }
                Some(t)
            }
            None => self.st.lock().temp_before.take().or(self.temp_default),
        };
        if let Some(want) = want_t {
            if self.caps.temp.is_some() && self.nvml.temp_threshold(nvml::TEMP_ACOUSTIC_CURR).ok() != Some(want) {
                push("Temperature limit", self.nvml.set_temp_limit(want as i32).map(|_| format!("{want} °C")).map_err(e));
            }
        }

        // clocks: one mechanism (the VF table); NVML's own offsets are kept at 0 so nothing stacks
        for clk in [nvml::CLOCK_GRAPHICS, nvml::CLOCK_MEM] {
            if self.nvml.offset(clk).map(|o| o.0 != 0).unwrap_or(false) {
                let _ = self.nvml.set_offset(clk, 0);
            }
        }
        match &self.api {
            Some(a) => match curve::read(a) {
                Ok(mut c) => {
                    match &p.curve {
                        Some(pts) => {
                            let (lo, hi) = (self.caps.core.0 as f32, self.caps.core.1 as f32);
                            let ab: Vec<curve::AbPt> = pts.iter().map(|(mv, d)| curve::AbPt { mv: *mv, mhz: 0.0, delta: d.clamp(lo, hi) }).collect();
                            curve::set_offset(&mut c, 0.0);
                            curve::apply_ab(&mut c, &ab);
                        }
                        None => curve::set_offset(&mut c, p.core_mhz.clamp(self.caps.core.0, self.caps.core.1) as f32),
                    }
                    curve::set_mem_offset(&mut c, p.mem_mhz.clamp(self.caps.mem.0, self.caps.mem.1) as f32);
                    let what = if p.curve.is_some() { "Curve".to_string() } else { format!("Core {:+} MHz, memory {:+} MHz", p.core_mhz, p.mem_mhz) };
                    let r = curve::write(a, &c);
                    clocks_written = r.is_ok();
                    self.st.lock().expected = r.is_ok().then(|| c.clone());
                    push("Clocks", r.map(|_| what));
                }
                Err(err) => push("Clocks", Err(err)),
            },
            None => {
                if p.core_mhz != 0 || p.mem_mhz != 0 || p.curve.is_some() {
                    push("Clocks", Err("NvAPI is not available on this driver.".into()));
                }
            }
        }

        // clock lock (Afterburner's "L"): the card stays at that point's clock, as the driver
        // runs it (a point is never faster than the points to its right)
        let was_locked = super::CLOCK_LOCKED.swap(false, SeqCst);
        match p.lock_mv.filter(|_| p.curve.is_some()) {
            Some(_) if !clocks_written => {
                if was_locked {
                    let _ = self.nvml.unlock_clocks();
                }
                push("Clock lock", Err("the curve wasn't written, so nothing was locked".into()));
            }
            Some(mv) => {
                let at = self.curve().and_then(|c| {
                    let k = c.core.iter().enumerate().min_by(|a, b| (a.1.mv - mv).abs().total_cmp(&(b.1.mv - mv).abs()))?.0;
                    c.core[k..].iter().map(|q| q.mhz()).reduce(f32::min)
                });
                match at {
                    Some(f) if f > 200.0 => {
                        let f = f.round() as u32;
                        let r = self.nvml.lock_clocks(f, f).map_err(e);
                        if r.is_err() && was_locked {
                            let _ = self.nvml.unlock_clocks(); // never keep the previous profile's lock
                        }
                        super::CLOCK_LOCKED.store(r.is_ok(), SeqCst);
                        push("Clock lock", r.map(|_| format!("{f} MHz at {} mV", mv.round())));
                    }
                    _ => {
                        if was_locked {
                            let _ = self.nvml.unlock_clocks();
                        }
                        push("Clock lock", Err("couldn't read the curve".into()));
                    }
                }
            }
            None => {
                if was_locked {
                    let _ = self.nvml.unlock_clocks();
                }
            }
        }

        // voltage boost
        if let Some(a) = &self.api {
            let want = p.volt_pct.unwrap_or(0).min(100);
            if a.volt_boost().ok() != Some(want) && (want > 0 || self.caps.volt) {
                push("Voltage", a.set_volt_boost(want).map(|_| format!("+{want}%")).map_err(|c| format!("NvAPI error {c}")));
            }
        }

        // fan
        {
            let mut st = self.st.lock();
            st.fan_last = None;
            st.fan_used_t = None;
            match p.fan.mode.as_str() {
                "fixed" => {
                    st.fan_curve = None;
                    st.fan_fixed = Some(p.fan.pct.clamp(self.caps.fan_min, 100));
                }
                "curve" if p.fan.points.len() >= 2 => {
                    st.fan_fixed = None;
                    st.fan_curve = Some(p.fan.points.clone());
                }
                _ => {
                    st.fan_fixed = None;
                    st.fan_curve = None;
                }
            }
        }
        push("Fan", self.fan_step(true));
        out
    }

    /// Puts everything back to the card's defaults.
    pub fn stock(&self) -> Result<String, String> {
        let p = GpuProfile { id: "stock".into(), name: "Stock".into(), ..Default::default() };
        let r = self.apply(&p);
        self.st.lock().applied = None;
        match r.error() {
            None => Ok("Back to stock".into()),
            Some(e) => Err(e),
        }
    }

    fn fan_target(&self, temp: Option<f32>) -> Option<u32> {
        let mut st = self.st.lock();
        if let Some(f) = st.fan_fixed {
            return Some(f);
        }
        let pts = st.fan_curve.clone()?;
        let raw = temp?;
        // falls only after 2 °C and 3 s, rises at once (like the case fans)
        let t = match st.fan_used_t {
            Some((u, at)) if raw < u && (u - raw < 2.0 || at.elapsed() < Duration::from_secs(3)) => u,
            _ => {
                st.fan_used_t = Some((raw, Instant::now()));
                raw
            }
        };
        let c = crate::hw::kv::Curve { name: String::new(), pts };
        Some((c.eval(t).round() as u32).clamp(self.caps.fan_min, 100))
    }

    /// One fan update: sets the speed when it changed (or every 10 s), or hands the fans back.
    fn fan_step(&self, force: bool) -> Result<String, String> {
        let temp = self.nvml.live().temp_c.map(|t| t as f32);
        let _gate = super::FAN_GATE.lock();
        if super::STOPPING.load(SeqCst) {
            return Ok("stopping".into());
        }
        match self.fan_target(temp) {
            Some(pct) => {
                let stale = {
                    let st = self.st.lock();
                    force || st.fan_last.map(|(v, at)| v != pct || at.elapsed() > Duration::from_secs(10)).unwrap_or(true)
                };
                if stale {
                    for i in 0..self.caps.fans {
                        self.nvml.set_fan(i, pct).map_err(|c| format!("fan: {}", nvml::err_text(c)))?;
                    }
                    super::FAN_MANUAL.store(true, SeqCst);
                    self.st.lock().fan_last = Some((pct, Instant::now()));
                }
                Ok(format!("{pct}%"))
            }
            None => {
                if super::FAN_MANUAL.swap(false, SeqCst) {
                    for i in 0..self.caps.fans {
                        let _ = self.nvml.fan_auto(i);
                    }
                }
                Ok("auto".into())
            }
        }
    }

    /// The clocks no longer match what Deck wrote (a driver reset puts the card back to stock).
    fn drifted(&self) -> bool {
        let Some(exp) = self.st.lock().expected.clone() else { return false };
        let Some(now) = self.curve() else { return false };
        exp.core.iter().chain(exp.mem.iter().filter(|p| p.editable)).any(|p| {
            now.core.iter().chain(now.mem.iter()).find(|q| q.index == p.index).map(|q| (q.delta_mhz - p.delta_mhz).abs() > 16.0).unwrap_or(false)
        })
    }

    /// Fan curve every second; drift check every 5 s (not while Afterburner runs: it would fight).
    pub fn start_loop(self: &Arc<Self>, app: Arc<crate::app::App>) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("gpu".into())
            .spawn(move || {
                let mut tick = 0u64;
                let mut resume_at: Option<u64> = None;
                super::watch_resume();
                while !super::STOPPING.load(SeqCst) {
                    // after sleep: give the driver a few seconds, then put the profile back the way
                    // Afterburner's "restore after suspended mode" did (limits aren't drift-checked)
                    if super::RESUMED.swap(false, SeqCst) {
                        resume_at = Some(tick + 5);
                    }
                    if resume_at.is_some_and(|t| tick >= t) {
                        resume_at = None;
                        let p = me.st.lock().applied.clone();
                        if let Some(p) = p.filter(|_| !super::afterburner_running()) {
                            let failed: Vec<String> = me.apply_fields(&p).into_iter().filter(|f| !f.ok).map(|f| format!("{}: {}", f.what, f.note)).collect();
                            if failed.is_empty() {
                                crate::log::info(format!("gpu: {} applied again after waking from sleep", p.name));
                            } else {
                                crate::log::warn(format!("gpu: {} after waking from sleep, not applied: {}", p.name, failed.join(" · ")));
                            }
                        }
                    }
                    if let Err(e) = me.fan_step(false) {
                        // every second while it fails: said once in a while
                        if crate::log::every("gpu fan step", Duration::from_secs(600)) {
                            crate::log::warn(format!("gpu: the fan curve couldn't be applied: {e}"));
                        }
                    }
                    if tick % 5 == 4 && me.drifted() && !super::afterburner_running() {
                        let p = me.st.lock().applied.clone();
                        let first = me.st.lock().reset_reapplied.is_none();
                        // once per applied profile; an overclock that reset the driver is never
                        // pushed again (that would be a crash loop)
                        match p {
                            Some(p) if first && !super::guard::risky(&p) => {
                                me.st.lock().reset_reapplied = Some(Instant::now());
                                let ok = me.apply_fields(&p).iter().all(|f| f.ok);
                                crate::log::warn(format!("gpu: the card's clocks no longer match {} (a driver reset?); applied again: {}", p.name, if ok { "yes" } else { "not fully" }));
                                let body = format!("The card went back to stock (a driver reset?). {} {}", if ok { "Re-applied" } else { "Could not re-apply" }, p.name);
                                app.toast(crate::app::Toast::simple("gpu-reset", if ok { "GPU settings re-applied" } else { "GPU settings changed by something else" }, &body));
                            }
                            Some(p) => {
                                crate::log::warn(format!("gpu: the card's clocks changed again under {} (risky overclock: {}); left off", p.name, super::guard::risky(&p)));
                                me.st.lock().expected = None; // stop watching until the next apply
                                let body = if super::guard::risky(&p) {
                                    format!("The card went back to stock (a driver reset?) while the overclock {} was on. Deck did not apply it again.", p.name)
                                } else {
                                    format!("Something changed the card's clocks again. Deck left {} off.", p.name)
                                };
                                app.toast(crate::app::Toast::simple("gpu-reset", "GPU settings changed", &body));
                            }
                            None => {}
                        }
                    }
                    tick += 1;
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .ok();
    }
}
