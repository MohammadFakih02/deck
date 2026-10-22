//! Tauri commands for Maintenance → Apps. The work lives in `programs`; these run it off the UI
//! thread (an uninstaller can take minutes).

use crate::app::App;
use crate::programs;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// Every installed program.
#[tauri::command]
pub async fn programs_list(app: A<'_>, force: bool) -> R<Value> {
    let app = app.inner().clone();
    blocking(move || programs::to_json(&programs::list(&app, force))).await
}

/// uninstall (its own uninstaller, waited for) / forget (uninstaller missing: straight to the
/// leftovers) / scan (leftovers of the last one) / remove (leftovers by id) / folder (open it).
#[tauri::command]
pub async fn programs_do(app: A<'_>, what: String, id: Option<String>, ids: Option<Vec<usize>>) -> R<Value> {
    let app = app.inner().clone();
    blocking(move || -> R<Value> {
        let id = id.unwrap_or_default();
        let out = match what.as_str() {
            "uninstall" | "uninstall_advanced" => {
                let r = programs::uninstall(&app, &id, what == "uninstall_advanced").map(|o| json!({ "gone": o.gone, "detail": o.detail }));
                // the uninstaller's window took the focus and Deck hid: come back with the result
                app.show_page("programs");
                r
            }
            "drop_unfinished" => {
                programs::drop_unfinished();
                Ok(json!({}))
            }
            "forget" => programs::forget(&app, &id).map(|_| json!({})),
            "scan" => programs::scan(&app).map(|l| json!({ "leftovers": l })),
            "remove" => programs::remove_leftovers(&ids.unwrap_or_default()).map(|f| json!({ "failed": f })),
            _ => Err(format!("unknown {what}")),
        };
        app.changed("programs");
        out
    })
    .await?
}
