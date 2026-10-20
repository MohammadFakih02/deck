//! Tauri commands for Maintenance → Tweaks. The engine lives in `tweaks`; these only translate JSON
//! and run its slow calls off the UI thread.

use crate::app::App;
use crate::tweaks;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// Every tweak with its state, the categories, presets, and what waits for a restart.
#[tauri::command]
pub async fn tweaks_state() -> R<Value> {
    blocking(tweaks::state).await
}

/// Sets tweaks: [{id, opt}] in order. Returns the ones that failed, with why.
#[tauri::command]
pub async fn tweaks_set(app: A<'_>, changes: Vec<(String, String)>) -> R<Value> {
    let app = app.inner().clone();
    blocking(move || {
        let mut failed = Vec::new();
        for (id, opt) in changes {
            if let Err(e) = tweaks::set(&app, &id, &opt) {
                failed.push(json!({ "id": id, "name": tweaks::find(&id).map(|t| t.name.clone()), "error": e }));
            }
        }
        app.changed("tweaks");
        json!({ "failed": failed })
    })
    .await
}

/// undo, undo_all, forget, action, restore_point, after (explorer / signout / restart / later).
#[tauri::command]
pub async fn tweaks_do(app: A<'_>, what: String, id: Option<String>) -> R<Value> {
    let app = app.inner().clone();
    blocking(move || -> R<Value> {
        let id = id.unwrap_or_default();
        let out = match what.as_str() {
            "undo" => tweaks::undo(&id).map(|_| json!({})),
            "undo_all" => {
                let (n, failed) = tweaks::undo_all();
                Ok(json!({ "undone": n, "failed": failed }))
            }
            "forget" => {
                tweaks::forget(&id);
                Ok(json!({}))
            }
            "action" => tweaks::run_action(&app, &id).map(|_| json!({})),
            "restore_point" => tweaks::restore_point().map(|made| json!({ "made": made })),
            "after" => tweaks::after(&app, &id).map(|_| json!({})),
            // Windows' own panels a tweak links to (a fixed list)
            "open" => match id.as_str() {
                "visualfx" => crate::util::launch_as_user(&crate::util::system_dir().join("SystemPropertiesPerformance.exe").display().to_string(), "", "").map(|_| json!({})),
                _ => Err("unknown panel".into()),
            },
            _ => Err(format!("unknown {what}")),
        };
        app.changed("tweaks");
        out
    })
    .await?
}

/// Built-in apps: installed ones (and the ones Deck removed).
#[tauri::command]
pub async fn tweaks_apps() -> R<Value> {
    blocking(tweaks::apps::state).await
}

/// remove / reinstall a built-in app by name.
#[tauri::command]
pub async fn tweaks_app(app: A<'_>, what: String, name: String) -> R<()> {
    let app = app.inner().clone();
    blocking(move || match what.as_str() {
        "remove" => tweaks::apps::remove(&app, &name),
        "reinstall" => tweaks::apps::reinstall(&name),
        _ => Err(format!("unknown {what}")),
    })
    .await?
}

/// Store apps without the Store: search, links (rg-adguard style), install / save in Deck with
/// progress, send to FluxDM, winget for desktop apps, and the download list.
#[tauri::command]
pub async fn tweaks_store(what: String, id: Option<String>, query: Option<String>) -> R<Value> {
    blocking(move || -> R<Value> {
        let id = id.unwrap_or_default();
        match what.as_str() {
            "search" => tweaks::apps::store_search(&query.unwrap_or_default()),
            "links" => tweaks::storedl::link_list(&id),
            "install" | "save" => tweaks::storedl::start(&id, &what).map(|_| json!({})),
            "fluxdm" => tweaks::storedl::to_fluxdm(&id).map(|n| json!({ "sent": n })),
            "winget" => tweaks::apps::store_install(&id).map(|_| json!({})),
            "jobs" => Ok(json!({ "jobs": tweaks::storedl::jobs() })),
            "cancel" => {
                tweaks::storedl::cancel(&id);
                Ok(json!({}))
            }
            "clear" => {
                tweaks::storedl::clear_finished();
                Ok(json!({}))
            }
            _ => Err(format!("unknown {what}")),
        }
    })
    .await?
}
