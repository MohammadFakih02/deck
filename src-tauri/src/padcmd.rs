//! Tauri commands for the Controller page (the controller overlay and controller batteries).
//! The overlay and battery logic live in `pad`; these only translate JSON. Settings go through
//! `set_setting("pad", …)` like every other setting.

use crate::app::App;
use crate::pad;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

/// The overlay's state, every controller with a battery, the monitors and the key list.
/// `test`: the Test tab is open, so live input is included.
#[tauri::command]
pub async fn pad_state(test: Option<bool>) -> R<Value> {
    Ok(pad::state(test.unwrap_or(false)))
}

/// "edit" (Move / resize), "demo", "visible", "controller", "kbm".
#[tauri::command]
pub async fn pad_action(app: A<'_>, what: String) -> R<()> {
    pad::action(&app, &what)
}
