//! CPU and GPU profiles: what they hold, and the one switch path every surface uses
//! (profile cards, tray, hotkeys, desktop/Start shortcuts, phone, Game boost).

pub mod afterburner;
pub mod cpu;
pub mod hotkeys;
pub mod icons;
pub mod plans;
pub mod shortcuts;

use crate::app::{App, Toast};
use serde::{Deserialize, Serialize};

/// A CPU profile is a Windows power plan Deck manages, plus extras Deck applies itself.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct CpuProfile {
    pub id: String,
    pub name: String,
    /// icon file name from the built-in set, or a full path to the user's own .ico
    pub icon: String,
    /// "Ctrl+Alt+]" style, "" = none
    pub hotkey: String,
    pub desktop: bool,
    pub start_menu: bool,
    /// the Windows power plan GUID
    pub plan: String,
    /// max clock MHz for P-cores / E-cores (None = no cap)
    pub cap_p: Option<u32>,
    pub cap_e: Option<u32>,
    /// maximum / minimum processor state, %
    pub max_state: u32,
    pub min_state: u32,
    /// PERFBOOSTMODE: 0 off, 1 on, 2 aggressive, 3 efficient on, 4 efficient aggressive
    pub turbo: u32,
    /// core parking: minimum cores awake, %
    pub min_cores: u32,
    /// energy/performance preference 0 (speed) .. 100 (saving); None = leave the plan's
    pub epp: Option<u32>,
    /// CPU power limits (RAPL): None = leave; "saver" | "balanced" | "performance" presets or custom watts
    pub power: Option<CpuPower>,
    /// keep the clock at or above this MHz (heat guard releases it above 92 °C)
    pub min_lock: Option<u32>,
    /// turbo ratio override MHz (unlocked Intel CPUs only)
    pub oc: Option<u32>,
    /// turn Windows Game Mode off while this profile is on, so caps hold in games
    pub hold_in_games: bool,
}

impl CpuProfile {
    /// The profile without caps that don't cap: a clock cap at or above `top` (the CPU's top turbo)
    /// still holds the CPU at it and stops any overclock, so it means "no cap"; an overclock to
    /// `top` or below changes nothing.
    pub fn without_silent_caps(&self, top: u32) -> CpuProfile {
        let mut p = self.clone();
        if top > 0 {
            p.cap_p = p.cap_p.filter(|c| *c < top);
            p.cap_e = p.cap_e.filter(|c| *c < top);
            p.oc = p.oc.filter(|o| *o > top);
        }
        p
    }
}

impl Default for CpuProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            icon: "cpu-eco.ico".into(),
            hotkey: String::new(),
            desktop: false,
            start_menu: false,
            plan: String::new(),
            cap_p: None,
            cap_e: None,
            max_state: 100,
            min_state: 5,
            turbo: 2,
            min_cores: 10,
            epp: None,
            power: None,
            min_lock: None,
            oc: None,
            hold_in_games: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct CpuPower {
    /// "saver" | "balanced" | "performance" | "custom"
    pub mode: String,
    pub pl1: u32,
    pub pl2: u32,
}

impl Default for CpuPower {
    fn default() -> Self {
        Self { mode: "balanced".into(), pl1: 0, pl2: 0 }
    }
}

/// A GPU profile: what MSI Afterburner keeps in one profile slot.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct GpuProfile {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub hotkey: String,
    pub desktop: bool,
    pub start_menu: bool,
    /// power limit, % of the card's default (clamped to what the card allows)
    pub power_pct: u32,
    /// temperature limit °C (None = leave the card's)
    pub temp_limit: Option<u32>,
    /// core clock offset MHz (ignored when `curve` is set)
    pub core_mhz: i32,
    pub mem_mhz: i32,
    /// core voltage boost % (None = 0)
    pub volt_pct: Option<u32>,
    /// shaped voltage/frequency curve: (mV, delta MHz) per core point
    pub curve: Option<Vec<(f32, f32)>>,
    /// Afterburner's "L": the card stays at the curve point with this voltage (curve only)
    pub lock_mv: Option<f32>,
    pub fan: GpuFan,
}

impl GpuProfile {
    /// Same values on the card (name, icon and shortcuts aside).
    pub fn same_settings(&self, o: &GpuProfile) -> bool {
        let k = |p: &GpuProfile| GpuProfile { id: String::new(), name: String::new(), icon: String::new(), hotkey: String::new(), desktop: false, start_menu: false, ..p.clone() };
        k(self) == k(o)
    }
}

impl Default for GpuProfile {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), icon: "gpu-green.ico".into(), hotkey: String::new(), desktop: false, start_menu: false, power_pct: 100, temp_limit: None, core_mhz: 0, mem_mhz: 0, volt_pct: None, curve: None, lock_mv: None, fan: GpuFan::default() }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct GpuFan {
    /// "auto" | "fixed" | "curve"
    pub mode: String,
    pub pct: u32,
    /// (°C, %) points for "curve"
    pub points: Vec<(f32, f32)>,
}

impl Default for GpuFan {
    fn default() -> Self {
        Self { mode: "auto".into(), pct: 50, points: vec![(40.0, 30.0), (55.0, 40.0), (65.0, 55.0), (75.0, 75.0), (83.0, 100.0)] }
    }
}

/// Short line under a profile's name: "1500 MHz · turbo off · 35 W".
pub fn cpu_summary(p: &CpuProfile) -> String {
    let mut v = vec![];
    match (p.cap_p, p.cap_e) {
        (Some(a), Some(b)) if a != b => v.push(format!("P {a} / E {b} MHz")),
        (Some(a), _) | (_, Some(a)) => v.push(format!("{a} MHz")),
        _ if p.max_state < 100 => v.push(format!("max {}%", p.max_state)),
        _ => v.push("no cap".into()),
    }
    if p.turbo == 0 {
        v.push("turbo off".into());
    }
    if let Some(pw) = &p.power {
        v.push(match pw.mode.as_str() {
            "custom" => format!("{} W", pw.pl1),
            m => m.to_string(),
        });
    }
    if let Some(m) = p.min_lock {
        v.push(format!("min {m} MHz"));
    }
    if let Some(m) = p.oc {
        v.push(format!("OC {m} MHz"));
    }
    v.join(" · ")
}

/// "PL 100% · −493 MHz · mem −500 · fan 30%".
pub fn gpu_summary(p: &GpuProfile) -> String {
    let mut v = vec![format!("PL {}%", p.power_pct)];
    if p.curve.is_some() {
        v.push(match p.lock_mv {
            Some(mv) => format!("curve, locked at {} mV", mv.round()),
            None => "curve".into(),
        });
    } else if p.core_mhz != 0 {
        v.push(format!("{:+} MHz", p.core_mhz));
    }
    if p.mem_mhz != 0 {
        v.push(format!("mem {:+}", p.mem_mhz));
    }
    if let Some(vb) = p.volt_pct.filter(|v| *v > 0) {
        v.push(format!("+{vb}% V"));
    }
    v.push(match p.fan.mode.as_str() {
        "fixed" => format!("fan {}%", p.fan.pct),
        "curve" => "fan curve".into(),
        _ => "fan auto".into(),
    });
    v.join(" · ")
}

/// A profile id unique within this run (time + counter), e.g. "p4821930-3".
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("p{}-{}", crate::util::now_ms() % 100_000_000, N.fetch_add(1, Ordering::SeqCst))
}

/// Switches a CPU (`kind` = "cpu") or GPU ("gpu") profile by id. `source` names who asked
/// ("hotkey", "shortcut", "tray", "phone", "page", "boost") for the toast.
pub fn switch(app: &App, kind: &str, id: &str, source: &str) {
    // a shortcut can start Deck: let the engines come up first (power limits need the hardware
    // engine, GPU profiles the GPU engine)
    if crate::elevation::is_admin() && source != "startup" {
        use std::sync::atomic::Ordering::SeqCst;
        let flag = if kind == "gpu" { &app.gpu_done } else { &app.hw_done };
        let t0 = std::time::Instant::now();
        while !flag.load(SeqCst) && t0.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if !flag.load(SeqCst) {
            crate::log::warn(format!("profiles: the {} engine wasn't up after 20 s; switching anyway ({source})", if kind == "gpu" { "GPU" } else { "hardware" }));
        }
    }
    let op = crate::log::Op::start("profiles", format!("switch {kind} to {id} ({source})"));
    let r = match kind {
        "cpu" => {
            let p = app.settings.lock().cpu_profiles.iter().find(|p| p.id == id).cloned();
            match p {
                Some(p) => cpu::apply(app, &p).map(|m| (p.name.clone(), m)),
                None => Err("That CPU profile no longer exists.".into()),
            }
        }
        "gpu" => {
            let p = app.settings.lock().gpu_profiles.iter().find(|p| p.id == id).cloned();
            match (p, app.gpu_engine()) {
                // a refused field (shown in the toast) doesn't make it any less the profile on the
                // card; nothing applied at all does
                (Some(p), Some(g)) => match g.apply(&p) {
                    crate::gpu::engine::Applied::All(r) => Ok((p.name.clone(), r)),
                    crate::gpu::engine::Applied::Partly(e) => Ok((p.name.clone(), format!("Applied, except: {e}"))),
                    crate::gpu::engine::Applied::Nothing(e) => Err(format!("Nothing reached the card: {e}")),
                },
                (None, _) => Err("That GPU profile no longer exists.".into()),
                (_, None) => Err("No NVIDIA graphics card was found.".into()),
            }
        }
        _ => Err("Unknown profile kind.".into()),
    };
    match &r {
        Ok((name, note)) => op.ok(format!("{name}{}{note}", if note.is_empty() { "" } else { ": " })),
        Err(e) => op.fail(e),
    }
    match r {
        Ok((name, note)) => {
            {
                let mut s = app.settings.lock();
                if kind == "cpu" {
                    s.active_cpu = id.to_string();
                } else {
                    s.active_gpu = id.to_string();
                }
            }
            app.save_settings();
            if kind == "gpu" && source != "startup" && crate::profcmd::HELD.lock().take().is_some() {
                app.dismiss_toast("gpu-held"); // the user chose a profile: nothing is held any more
            }
            app.changed("profiles");
            if app.settings.lock().profile_toast && source != "startup" {
                let title = format!("{} → {}", kind.to_uppercase(), name);
                app.toast(Toast::simple(&format!("profile-{kind}"), &title, &note));
            }
        }
        Err(e) => {
            app.toast(Toast::simple(&format!("profile-{kind}"), &format!("Couldn't switch the {} profile ({source})", kind.to_uppercase()), &e));
            app.changed("profiles");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let mut c = CpuProfile { cap_p: Some(1500), cap_e: Some(1500), turbo: 0, ..Default::default() };
        c.power = Some(CpuPower { mode: "custom".into(), pl1: 35, pl2: 45 });
        assert_eq!(cpu_summary(&c), "1500 MHz · turbo off · 35 W");
        c = CpuProfile { max_state: 80, ..Default::default() };
        assert_eq!(cpu_summary(&c), "max 80%");
        let g = GpuProfile { power_pct: 41, core_mhz: -493, mem_mhz: 0, fan: GpuFan { mode: "fixed".into(), pct: 30, points: vec![] }, ..Default::default() };
        assert_eq!(gpu_summary(&g), "PL 41% · -493 MHz · fan 30%");
        // same values under another name / key: the same profile; a clock lock makes it different
        let a = GpuProfile { name: "A".into(), hotkey: "Ctrl+Alt+1".into(), curve: Some(vec![(800.0, 10.0)]), ..g.clone() };
        let b = GpuProfile { name: "B".into(), id: "x".into(), ..a.clone() };
        assert!(a.same_settings(&b));
        assert!(!a.same_settings(&GpuProfile { lock_mv: Some(800.0), ..b }));
    }

    /// A cap at the CPU's top turbo still caps (and stops an overclock): it means "no cap". An
    /// overclock to the top turbo or below is no overclock.
    #[test]
    fn a_cap_at_the_top_is_no_cap() {
        let c = CpuProfile { cap_p: Some(5300), cap_e: Some(5300), oc: Some(5300), ..Default::default() };
        let n = c.without_silent_caps(5300);
        assert_eq!((n.cap_p, n.cap_e, n.oc), (None, None, None));
        let c = CpuProfile { cap_p: Some(4600), cap_e: Some(5400), oc: Some(5600), ..Default::default() };
        let n = c.without_silent_caps(5300);
        assert_eq!((n.cap_p, n.cap_e, n.oc), (Some(4600), None, Some(5600)));
        assert_eq!(cpu_summary(&CpuProfile { cap_p: Some(5300), cap_e: Some(5300), ..Default::default() }.without_silent_caps(5300)), "no cap");
    }

    #[test]
    fn ids_unique_in_a_tight_loop() {
        let ids: std::collections::HashSet<String> = (0..1000).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn profiles_fill_missing_fields() {
        let g: GpuProfile = serde_json::from_str(r#"{"id":"a","name":"Low","core_mhz":-100}"#).unwrap();
        assert_eq!(g.power_pct, 100);
        assert_eq!(g.fan.mode, "auto");
        let c: CpuProfile = serde_json::from_str(r#"{"id":"b","plan":"x"}"#).unwrap();
        assert_eq!(c.max_state, 100);
        assert!(c.hold_in_games);
    }
}
