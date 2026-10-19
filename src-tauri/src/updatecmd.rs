//! Tauri commands for Settings > About > Updates. The logic lives in `update`.

use crate::app::App;
use crate::update;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

/// What the page shows: this version, the newest known, and what is happening.
#[tauri::command]
pub async fn update_state() -> R<Value> {
    Ok(update::state())
}

/// "Check now": asks GitHub once and returns the state.
#[tauri::command]
pub async fn update_check(app: A<'_>) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || update::check_now(&app)).await.map_err(|e| e.to_string())?
}

/// "Install now": downloads, checks and runs the setup (a UAC prompt when Deck has no admin rights).
#[tauri::command]
pub async fn update_install(app: A<'_>) -> R<()> {
    update::install_now(&app)
}
