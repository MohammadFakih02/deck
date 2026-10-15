//! Tauri commands for the Hardware pages (Fans, Lighting, Cooler display, Sensors, This PC).
//! The engine itself lives in `hw`; these only translate to and from JSON.

use crate::app::App;
use crate::hw::{self, cfg, Shared};
use serde_json::{json, Value};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

fn engine(app: &App) -> R<Arc<Shared>> {
    app.hw.lock().clone().ok_or_else(|| if crate::elevation::is_admin() { "The hardware engine is still starting.".into() } else { "Hardware control needs Deck to run as admin. Use Set up.".into() })
}

fn control_text(c: &hw::sio::Control) -> (String, String) {
    match c {
        hw::sio::Control::Verified => ("verified".into(), String::new()),
        hw::sio::Control::Experimental => ("experimental".into(), String::new()),
        hw::sio::Control::ReadOnly(why) => ("readonly".into(), why.clone()),
    }
}

/// Every drive's health (S.M.A.R.T.). `refresh` reads the drives again instead of using the
/// last reading (up to a minute old).
#[tauri::command]
pub async fn drive_health(refresh: Option<bool>) -> R<Value> {
    let max_age = if refresh.unwrap_or(false) { 0 } else { 60 };
    let v = tauri::async_runtime::spawn_blocking(move || hw::smart::cached(max_age)).await.map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(v).unwrap_or_default())
}

/// Everything the pages need once: what was found, options, the applied configuration.
#[tauri::command]
pub fn hw_state(app: A) -> Value {
    let admin = crate::elevation::is_admin();
    let gpu = app.gpu.lock().is_some();
    let Some(sh) = app.hw.lock().clone() else {
        return json!({ "admin": admin, "ready": false, "pages": hw::pages(None, gpu), "moboctl": hw::kv::moboctl_dir().is_some() });
    };
    let h = sh.hw.lock().unwrap().clone();
    let st = sh.status.lock().unwrap().clone();
    let (control, why) = control_text(&h.control);
    json!({
        "admin": admin,
        "ready": true,
        "pages": hw::pages(Some(&sh), gpu),
        "moboctl": hw::kv::moboctl_dir().is_some(),
        "info": {
            "board_vendor": h.board.board_vendor, "board_name": h.board.board_name, "bios": h.board.bios,
            "dimms": h.board.dimms, "chip": h.chip, "control": control, "control_why": why,
            "fan_names": h.fan_names, "temp_names": h.temp_names, "cpu": h.cpu_brand, "cpu_note": h.cpu_temp_note, "gpu": h.gpu_name,
        },
        "status": { "rgb": st.rgb.0, "rgb_ok": st.rgb.1, "ram": st.ram, "ram_ok": st.ram.contains("stick(s) at"), "disp": st.disp.0, "disp_ok": st.disp.1, "fans_error": st.fans },
        "sources": sh.sources(),
        "cfg": cfg::load(&sh),
        "lists": cfg::lists(),
        "lights_off": sh.lights_off.load(Relaxed),
        "geometry": hw::lcd::geometry(),
    })
}

/// Live readings (once a second while a Hardware page is open).
#[tauri::command]
pub fn hw_live(app: A) -> R<Value> {
    let sh = engine(&app)?;
    let l = sh.live.lock().unwrap().clone();
    let st = sh.status.lock().unwrap().clone();
    let status = json!({ "rgb": st.rgb.0, "rgb_ok": st.rgb.1, "ram": st.ram, "disp": st.disp.0, "disp_ok": st.disp.1, "fans_error": st.fans });
    Ok(json!({
        "cpu_t": l.cpu_t, "cpu_l": l.cpu_l, "cpu_mhz": l.cpu_mhz, "gpu_t": l.gpu_t, "gpu_l": l.gpu_l,
        "fan_pct": l.fan_pct, "fan_rpm": l.fan_rpm, "board_t": l.board_t, "volts": l.volts,
        "source_t": (0..sh.sources().len()).map(|i| l.source(i)).collect::<Vec<_>>(),
        "in_game": sh.in_game.load(Relaxed), "lights_off": sh.lights_off.load(Relaxed),
        "lock_released": sh.lock_released.swap(false, Relaxed),
        "status": status,
    }))
}

/// Five minutes of history for the Sensors graphs (NaN = no reading, sent as null).
#[tauri::command]
pub fn hw_hist(app: A) -> R<Value> {
    let sh = engine(&app)?;
    let h = sh.hist.lock().unwrap().clone();
    let v = |q: &std::collections::VecDeque<f32>| q.iter().map(|x| if x.is_nan() { None } else { Some(*x) }).collect::<Vec<_>>();
    Ok(json!({
        "cpu_t": v(&h.cpu_t), "gpu_t": v(&h.gpu_t), "cpu_l": v(&h.cpu_l), "gpu_l": v(&h.gpu_l),
        "cpu_w": v(&h.cpu_w), "gpu_w": v(&h.gpu_w),
        "board": h.board.iter().map(v).collect::<Vec<_>>(), "rpm": h.rpm.iter().map(v).collect::<Vec<_>>(),
    }))
}

/// The HWiNFO-style sensor table: current / min / max / average.
#[tauri::command]
pub fn hw_table(app: A) -> R<Value> {
    let sh = engine(&app)?;
    let t = sh.table.lock().unwrap().clone();
    let since = t.since.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let rows: Vec<Value> = t
        .rows
        .iter()
        .map(|r| {
            let has = r.n > 0;
            json!({ "group": r.group, "name": r.name, "unit": r.unit, "cur": r.fmt(r.cur), "min": r.fmt(has.then_some(r.min)), "max": r.fmt(has.then_some(r.max)), "avg": r.fmt(r.avg()) })
        })
        .collect();
    Ok(json!({ "since": since, "rows": rows }))
}

#[tauri::command]
pub fn hw_table_reset(app: A) -> R<()> {
    engine(&app)?.table.lock().unwrap().reset();
    Ok(())
}

/// Makes the edited configuration live (the pages' Apply button).
#[tauri::command]
pub fn hw_apply(app: A, cfg: cfg::HwCfg) -> R<()> {
    let sh = engine(&app)?;
    sh.s.set_bool("UserApplied", true); // MoboCtl's settings are never imported over these
    cfg::apply(&sh, &cfg);
    app.changed("hw");
    Ok(())
}

#[tauri::command]
pub fn hw_lights_off(app: A, on: bool) -> R<()> {
    let sh = engine(&app)?;
    cfg::set_lights_off(&sh, on);
    app.changed("hw");
    Ok(())
}

#[tauri::command]
pub fn hw_display_off(app: A, on: bool) -> R<()> {
    let sh = engine(&app)?;
    cfg::set_display_off(&sh, on);
    app.changed("hw");
    Ok(())
}

/// Plain-text report of this PC. `save` writes it to the Deck folder and shows it in Explorer.
#[tauri::command]
pub fn hw_report(app: A, save: bool) -> R<String> {
    let sh = engine(&app)?;
    let text = hw::report(&sh);
    if save {
        let path = hw::kv::dir().join("deck-hardware-report.txt");
        std::fs::write(&path, &text).map_err(|e| e.to_string())?;
        crate::util::reveal(&path.to_string_lossy());
        return Ok(path.to_string_lossy().to_string());
    }
    Ok(text)
}

/// Red, green and blue through every light zone (about 15 s), then a result file.
#[tauri::command]
pub async fn hw_test_lights(app: A<'_>) -> R<String> {
    let sh = engine(&app)?;
    let r = tauri::async_runtime::spawn_blocking(move || {
        let r = hw::lights_test(&sh).ok_or_else(|| "The lights test is already running.".to_string())?;
        let path = hw::kv::dir().join("deck-lights-test.txt");
        let _ = std::fs::write(&path, format!("{}\r\n{}", hw::report(&sh), r));
        crate::util::reveal(&path.to_string_lossy());
        Ok::<String, String>(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(r)
}

/// Opens the folder with the sensor log (monitor.csv) and the reports.
#[tauri::command]
pub fn hw_open_folder() {
    crate::util::reveal(&hw::kv::dir().join("settings.ini").to_string_lossy());
}

/// Trims what programs aren't using and drops the standby cache.
#[tauri::command]
pub async fn hw_clear_ram() -> R<Value> {
    tauri::async_runtime::spawn_blocking(|| {
        let msg = hw::memclean::clear();
        let mut v = hw_mem();
        v["msg"] = json!(msg);
        v
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn hw_mem() -> Value {
    let m = hw::memclean::stat();
    json!({ "used": m.used_gb, "total": m.total_gb, "cached": m.standby_gb, "free": m.free_gb })
}

/// One-time import of MoboCtl's fan, lighting and display settings.
#[tauri::command]
pub fn hw_import_moboctl(app: A) -> R<String> {
    let sh = engine(&app)?;
    let msg = hw::kv::import_moboctl()?;
    // reload what was imported into the running engine
    let fresh = hw::kv::Settings::load();
    for (k, v) in fresh.all() {
        sh.s.set(&k, &v);
    }
    *sh.curves.lock().unwrap() = hw::kv::Curve::load_all();
    let c = cfg::load(&sh);
    cfg::apply(&sh, &c);
    app.changed("hw");
    Ok(msg)
}

/// What the cooler display shows for these (unapplied) settings right now: 84 colours or null.
#[tauri::command]
pub fn hw_lcd_preview(app: A, display: cfg::Display, lighting: cfg::Lighting) -> R<Vec<Option<String>>> {
    let sh = engine(&app)?;
    let l = sh.live.lock().unwrap().clone();
    let mut fin = sh.fx_in();
    fin.audio = l.audio;
    let inp = hw::lcd::DispIn { cpu_t: l.cpu_t, cpu_l: l.cpu_l, gpu_t: l.gpu_t, gpu_l: l.gpu_l, audio: l.audio, light: Some((cfg::effect_params(&lighting), fin)), ..Default::default() }.now();
    let mut p = cfg::disp_params(&display);
    if display.content == "Design" {
        p.night = false;
        p.off = false;
        p.alert_flash = false;
    }
    Ok(hw::lcd::frame(&p, sh.t(), &inp).iter().map(|c| c.map(|c| c.hex())).collect())
}

/// Turns the Sensors view into a design for the display painter.
#[tauri::command]
pub fn hw_lcd_grab(app: A, display: cfg::Display) -> R<Vec<String>> {
    let sh = engine(&app)?;
    let l = sh.live.lock().unwrap().clone();
    let mut p = cfg::disp_params(&display);
    p.content = "Sensors".into();
    p.color_mode = "Custom".into();
    p.night = false;
    p.alert_flash = false;
    p.bright = 100;
    let inp = hw::lcd::DispIn { cpu_t: l.cpu_t, cpu_l: l.cpu_l, gpu_t: l.gpu_t, gpu_l: l.gpu_l, ..Default::default() }.now();
    Ok(hw::lcd::frame(&p, 0.0, &inp).iter().map(|c| c.unwrap_or(hw::fx::Rgb::BLACK).hex()).collect())
}
