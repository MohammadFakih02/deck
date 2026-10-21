//! Tauri commands for the Display page (refresh rate, brightness, HDR, dark mode, location).
//! The monitor and location logic lives in `display`; these only translate JSON and touch settings.

use crate::app::App;
use crate::display;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub async fn display_state(app: A<'_>) -> R<Value> {
    Ok(display::state(&app))
}

#[tauri::command]
pub async fn set_refresh(device: String, hz: u32) -> R<()> {
    display::set_refresh(&device, hz)
}

#[tauri::command]
pub async fn set_brightness(device: String, pct: u32) -> R<()> {
    display::set_brightness(&device, pct)
}

#[tauri::command]
pub async fn set_hdr(device: String, on: bool) -> R<()> {
    display::set_hdr(&device, on)
}

#[tauri::command]
pub fn set_dark(app: A, dark: bool) -> R<()> {
    display::set_dark(dark)?;
    app.changed("display");
    Ok(())
}

/// Rough location from the internet address, for the sunset schedule.
#[tauri::command]
pub async fn locate(app: A<'_>) -> R<Value> {
    let v: Value = crate::log::http_agent().get("https://ipwho.is/")
        .timeout(std::time::Duration::from_secs(6))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let lat = v.get("latitude").and_then(Value::as_f64).ok_or("no location")?;
    let lon = v.get("longitude").and_then(Value::as_f64).ok_or("no location")?;
    let place = format!("{}, {}", v.get("city").and_then(Value::as_str).unwrap_or(""), v.get("country").and_then(Value::as_str).unwrap_or(""));
    {
        let mut s = app.settings.lock();
        s.lat = Some((lat * 100.0).round() / 100.0);
        s.lon = Some((lon * 100.0).round() / 100.0);
        s.place = place.clone();
    }
    app.save_settings();
    Ok(json!({ "lat": lat, "lon": lon, "place": place }))
}
