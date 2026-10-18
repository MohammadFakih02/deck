//! Tauri commands for the Network page (info, public IP, ping, speed test, game ping).
//! The probing logic lives in `app.net`, `net` and `gameping`; these only translate JSON.

use crate::app::App;
use crate::{gameping, net};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub async fn net_info(app: A<'_>) -> R<Value> {
    Ok(app.net.info())
}

#[tauri::command]
pub async fn net_public_ip(app: A<'_>) -> R<Option<String>> {
    Ok(app.net.fetch_public_ip())
}

#[tauri::command]
pub async fn net_ping(hosts: Vec<String>) -> R<Vec<Value>> {
    Ok(net::ping_many(&hosts))
}

#[tauri::command]
pub fn net_speed(app: A) {
    app.net.speed_test(app.inner().clone());
}

#[tauri::command]
pub fn net_speed_cancel(app: A) {
    app.net.cancel_speed_test();
}

#[tauri::command]
pub async fn game_ping_games() -> R<Vec<Value>> {
    tauri::async_runtime::spawn_blocking(gameping::games).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn game_ping(game: String) -> R<Vec<Value>> {
    tauri::async_runtime::spawn_blocking(move || gameping::ping(&game)).await.map_err(|e| e.to_string())?
}
