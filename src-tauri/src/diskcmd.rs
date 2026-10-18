//! Tauri commands for the Disk (storage treemap) and Clean up pages.
//! The scanning and deletion logic lives in `app.disk`, `app.cleanup`, `disk` and `cleanup`;
//! these only translate JSON and guard protected paths.

use crate::app::App;
use crate::{cleanup, disk};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

#[tauri::command]
pub fn disk_scan(app: A, root: String) {
    let a = app.inner().clone();
    app.disk.start(root, move || a.changed("disk"));
    app.changed("disk");
}

#[tauri::command]
pub fn disk_status(app: A) -> Value {
    app.disk.status()
}

#[tauri::command]
pub fn disk_cancel(app: A) {
    app.disk.cancel()
}

/// The nested treemap of folder `id`, laid out for a w × h canvas.
#[tauri::command]
pub fn disk_treemap(app: A, id: u32, w: f64, h: f64) -> Value {
    app.disk.treemap(id, w, h)
}

/// The file types under folder `id`, biggest first, with their treemap colours.
#[tauri::command]
pub fn disk_types(app: A, id: u32) -> Value {
    app.disk.types(id)
}

/// A treemap cell's path: a file (`file`) or a folder (`id`).
#[tauri::command]
pub fn disk_path(app: A, id: Option<u32>, file: Option<u32>) -> Option<String> {
    match (file, id) {
        (Some(f), _) => app.disk.file_path(f),
        (None, Some(d)) => app.disk.dir_path(d),
        _ => None,
    }
}

#[tauri::command]
pub fn disk_children(app: A, id: u32) -> Value {
    app.disk.children(id)
}

#[tauri::command]
pub fn disk_big(app: A) -> Vec<Value> {
    app.disk.big_files()
}

#[tauri::command]
pub async fn recycle(app: A<'_>, paths: Vec<String>, sizes: Vec<u64>) -> R<()> {
    if let Some(why) = paths.iter().find_map(|p| disk::protected(p)) {
        return Err(why.into());
    }
    disk::recycle(&paths)?;
    for (p, s) in paths.iter().zip(sizes.iter()) {
        app.disk.forget(p, *s);
    }
    app.changed("disk");
    Ok(())
}

#[tauri::command]
pub fn cleanup_scan(app: A) {
    app.cleanup.scan(app.inner().clone());
}

#[tauri::command]
pub fn cleanup_state(app: A) -> Value {
    let (rb, rbn) = cleanup::recycle_bin();
    json!({
        "targets": *app.cleanup.targets.lock(),
        "busy": app.cleanup.busy.load(Ordering::Relaxed),
        "last": *app.cleanup.last.lock(),
        "recycle": { "size": rb, "items": rbn },
    })
}

#[tauri::command]
pub fn cleanup_run(app: A, ids: Vec<String>) {
    app.cleanup.run(ids, app.inner().clone());
}
