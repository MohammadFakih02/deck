//! Tauri commands for the audio/media controls (output device, volumes, media keys).
//! The mixer and session logic live in `audio` and `app.media`; these only translate JSON.

use crate::app::App;
use crate::audio;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub async fn audio_state() -> R<Value> {
    Ok(json!({ "outputs": audio::outputs(), "master": audio::master(), "apps": audio::apps() }))
}

#[tauri::command]
pub async fn set_output(app: A<'_>, id: String) -> R<()> {
    audio::set_default_output(&id)?;
    app.changed("audio");
    Ok(())
}

#[tauri::command]
pub async fn set_master(vol: Option<f32>, muted: Option<bool>) -> R<()> {
    audio::set_master(vol, muted)
}

#[tauri::command]
pub async fn set_app_volume(key: String, vol: Option<f32>, muted: Option<bool>) -> R<()> {
    audio::set_app(&key, vol, muted)
}

#[tauri::command]
pub async fn media_control(app: A<'_>, id: String, action: String, pos: Option<f64>) -> R<()> {
    app.media.control(&id, &action, pos)
}
