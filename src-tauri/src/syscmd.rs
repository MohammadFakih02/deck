//! Tauri commands for the System pages (Processes, Startup, open-file locks) and the
//! This PC support report. The logic lives in `app.sys`, `winmgr`, `startup`, `locksmith`
//! and `support`; these only translate JSON.

use crate::app::App;
use crate::{hw, locksmith, startup, support, util, winmgr};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub async fn processes(app: A<'_>) -> R<Vec<Value>> {
    let windowed: std::collections::HashMap<u32, (String, bool)> = winmgr::list().into_iter().map(|w| (w.pid, (w.title, w.hung))).collect();
    Ok(app.sys.processes(&windowed))
}

#[tauri::command]
pub async fn kill(app: A<'_>, name: String) -> R<usize> {
    app.sys.kill_name(&name)
}

#[tauri::command]
pub fn kill_pid(app: A, pid: u32) -> bool {
    app.sys.kill_pid(pid)
}

/// What's known about these programs' safety (no checking), the running check, and whether a
/// VirusTotal key is saved.
#[tauri::command]
pub fn filecheck_state(paths: Vec<String>) -> Value {
    crate::filecheck::state(&paths)
}

/// Starts a check for every one of these programs: "sig" | "engines" | "defender" | "vt".
#[tauri::command]
pub fn filecheck_start(app: A, what: String, paths: Vec<String>) -> R<()> {
    crate::filecheck::start(app.inner().clone(), &what, paths)
}

#[tauri::command]
pub fn filecheck_stop() {
    crate::filecheck::stop()
}

/// Checks one file once, privately: the checks in `what` ("sig" | "engines" | "defender" | "vt").
/// Nothing about the file is kept on disk.
#[tauri::command]
pub fn filecheck_once_start(app: A, path: String, what: Vec<String>) -> R<()> {
    crate::filecheck::once_start(app.inner().clone(), path, what)
}

/// The one-time check's progress and results (once: null when there is none).
#[tauri::command]
pub fn filecheck_once_state() -> Value {
    crate::filecheck::once_state()
}

#[tauri::command]
pub fn filecheck_once_stop() {
    crate::filecheck::once_stop()
}

/// Forgets the one-time check's file and results.
#[tauri::command]
pub fn filecheck_once_forget() {
    crate::filecheck::once_forget()
}

/// Saves the VirusTotal API key ("" forgets it).
#[tauri::command]
pub fn filecheck_key(key: String) -> R<()> {
    crate::filecheck::set_vt_key(&key)
}

#[tauri::command]
pub async fn startup_list() -> R<Vec<startup::Entry>> {
    Ok(startup::list())
}

#[tauri::command]
pub fn startup_set(id: String, on: bool) -> R<()> {
    startup::set_enabled(&id, on)
}

#[tauri::command]
pub async fn who_uses(path: String) -> R<Vec<locksmith::Holder>> {
    locksmith::who_uses(&path)
}

/// The support report (This PC): works with or without admin. `save` writes it to a file and
/// shows it in Explorer instead of returning the text; `lights` adds the lights test (every zone
/// red, green and blue for about 15 s, then what the controllers answered).
#[tauri::command]
pub async fn support_report(app: A<'_>, save: bool, lights: Option<bool>) -> R<String> {
    let a = app.inner().clone();
    let lights = lights.unwrap_or(false);
    let text = tauri::async_runtime::spawn_blocking(move || {
        let mut text = support::report(&a);
        if lights {
            text.push_str("\r\n== Lights test ==\r\n");
            text.push_str(&match a.hw.lock().clone() {
                Some(sh) => hw::lights_test(&sh).unwrap_or_else(|| "skipped: a lights test was already running\r\n".into()),
                None => "skipped: the hardware engine isn't running\r\n".into(),
            });
        }
        text
    })
    .await
    .map_err(|e| e.to_string())?;
    if !save {
        return Ok(text);
    }
    let dir = util::appdata().join("Deck");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("deck-support-report.txt");
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    util::reveal(&path.to_string_lossy());
    Ok(path.to_string_lossy().to_string())
}
