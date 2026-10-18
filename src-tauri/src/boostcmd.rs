//! Tauri commands for the Game boost page (status, the app list, run on/off/clean, the close list).
//! The boost logic lives in `app.boost` and `boost`; these only translate JSON and touch settings.

use crate::app::App;
use crate::boost;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub async fn boost_state(app: A<'_>) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || app.boost.state()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn boost_apps(app: A<'_>) -> R<Vec<Value>> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || boost::apps(&app)).await.map_err(|e| e.to_string())
}

/// "on" | "off" | "clean"
#[tauri::command]
pub async fn boost_run(app: A<'_>, what: String) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "on" => boost::start(&app),
        "off" => boost::stop(&app, ""),
        _ => boost::clean(&app),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Tick or untick an app in the "close for gaming" list.
#[tauri::command]
pub fn boost_pick(app: A, key: String, on: bool) {
    app.settings.lock().boost_close.insert(key, on);
    app.save_settings();
}
