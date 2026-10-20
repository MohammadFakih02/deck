//! Tauri commands for the Phone page, PC-to-PC (share links, Transfer) and Claude Code sync.
//! The logic lives in `remote`, `rdesk`, `share`, `beam` and `ccsync`; these only translate JSON.
//! `share_to_phone` stays in `main` because it owns the file-picker dialog.

use crate::app::App;
use crate::remote::{self, Remote};
use crate::{beam, ccsync, rdesk, share, util};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub fn remote_info(app: A, remote: State<Arc<Remote>>) -> Value {
    remote::info(&app, &remote)
}

/// The phone mic's VB-CABLE: ready, off (turned off in Device Manager), restart, or missing.
#[tauri::command]
pub async fn phone_mic_state() -> Value {
    tauri::async_runtime::spawn_blocking(crate::phonemic::cable_state).await.unwrap_or(Value::Null)
}

/// VB-CABLE for the phone mic: "enable" turns an installed one back on, "get" downloads VB-Audio's
/// package and opens its own setup.
#[tauri::command]
pub async fn phone_mic_do(what: String) -> R<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        match what.as_str() {
            "enable" => crate::phonemic::enable_cable(),
            "get" => crate::phonemic::get_cable(),
            _ => Err(format!("unknown action {what}")),
        }?;
        Ok(crate::phonemic::cable_state())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rdesk_status() -> Value {
    tauri::async_runtime::spawn_blocking(rdesk::status).await.unwrap_or(Value::Null)
}

/// RustDesk actions from the Phone page: install, on, password, direct, tray, open, tailscale.
#[tauri::command]
pub async fn rdesk_do(what: String, on: Option<bool>, text: Option<String>) -> R<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let on = on.unwrap_or(true);
        match what.as_str() {
            "install" => rdesk::install(),
            "on" => rdesk::set_on(on),
            "password" => rdesk::set_password(&text.unwrap_or_default()),
            "direct" => rdesk::set_direct(on),
            "tray" => rdesk::set_tray_hidden(on),
            "open" => rdesk::open_rustdesk(),
            "tailscale" => {
                rdesk::open_tailscale();
                Ok(())
            }
            _ => Err("Unknown action".into()),
        }?;
        Ok(rdesk::status())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One-time share links (PC to PC → Share link).
#[tauri::command]
pub fn share_state() -> Value {
    share::state()
}

#[tauri::command]
pub async fn share_create(path: String, hours: u64, max: u32, internet: bool) -> R<Value> {
    tauri::async_runtime::spawn_blocking(move || share::create(&path, hours, max, internet)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn share_stop(token: String) -> R<Value> {
    tauri::async_runtime::spawn_blocking(move || share::stop(&token)).await.map_err(|e| e.to_string())
}

/// Transfer page: everything it shows.
#[tauri::command]
pub fn transfer_state() -> Value {
    beam::state()
}

/// Transfer page actions (send, answer, browse, request, apps, setup, received items, settings).
#[tauri::command]
pub async fn transfer_do(app: A<'_>, what: String, args: Option<Value>) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || beam::act(&app, &what, &args.unwrap_or(Value::Null))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn transfer_apps() -> Value {
    beam::apps_state()
}

#[tauri::command]
pub async fn transfer_setup_preview() -> Value {
    tauri::async_runtime::spawn_blocking(beam::setup_preview).await.unwrap_or(Value::Null)
}

/// Claude Sync page: everything it shows.
#[tauri::command]
pub async fn claude_sync_state(app: A<'_>) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || ccsync::state(&app)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn claude_sync_do(app: A<'_>, what: String, args: Option<Value>) -> R<Value> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || ccsync::act(&app, &what, &args.unwrap_or(Value::Null))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn remote_reset_token(app: A, remote: State<Arc<Remote>>) -> Value {
    app.settings.lock().remote_token = util::random_token(22);
    app.save_settings();
    remote::info(&app, &remote)
}

#[tauri::command]
pub fn shared_remove(app: A, id: u64) {
    app.shared.lock().retain(|x| x.id != id);
    app.changed("shared");
}
