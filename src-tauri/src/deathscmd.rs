//! Tauri commands for the Deaths page (the death counter). The logic lives in `deaths`; these
//! only translate JSON. Settings go through `set_setting("deaths", …)` like every other setting.

use crate::app::App;
use crate::deaths;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

type A<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

/// Every counted game, the one shown (`game`, else the one in front / last used) with its open
/// fight, bosses and history, and the settings.
#[tauri::command]
pub async fn deaths_state(app: A<'_>, game: Option<String>) -> R<Value> {
    Ok(deaths::state(&app, game.as_deref()))
}

/// "death", "undo", "start" (with `boss`), "win", "abandon", "rename" (with `boss`), "off", "on",
/// "delete:<index>".
#[tauri::command]
pub async fn deaths_act(app: A<'_>, what: String, boss: Option<String>, game: Option<String>) -> R<()> {
    deaths::act(&app, &what, boss.as_deref(), game.as_deref(), "manual")
}

/// The frames before the last death counted by hand in `game` (null: none), to teach from.
#[tauri::command]
pub async fn deaths_strip(game: String) -> R<Value> {
    Ok(deaths::strip(&game))
}

/// "teach" (with `pick`, a frame of the strip), "forget" (with `pick`, a taught one), "share"
/// and "submit" (return a code; submit is for the shared library), "import" (with `code`;
/// returns the game's name).
#[tauri::command]
pub async fn deaths_teach(app: A<'_>, what: String, game: Option<String>, pick: Option<usize>, code: Option<String>) -> R<String> {
    let game = game.unwrap_or_default();
    match what.as_str() {
        "teach" => deaths::teach(&app, &game, pick.ok_or("pick a picture")?).map(|_| String::new()),
        "forget" => deaths::forget(&app, &game, pick.ok_or("which one?")?).map(|_| String::new()),
        "share" => deaths::share(&game),
        "submit" => {
            if !deaths::library::good_key(&game) {
                return Err("Deck can only add games it can name the same way on every PC; this game's program name is shared by many games.".into());
            }
            deaths::share(&game)
        }
        "import" => deaths::import(&app, code.as_deref().unwrap_or_default()),
        _ => Err(format!("unknown action {what}")),
    }
}
