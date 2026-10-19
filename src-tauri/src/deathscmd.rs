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

/// The counter and the card as they'd look with the saved settings: { counter, card } (data URLs).
#[tauri::command]
pub async fn deaths_preview(app: A<'_>) -> R<Value> {
    Ok(deaths::preview(&app))
}

/// A picture of a game's (or a boss's) numbers: copied to the clipboard, or (`save`) saved where
/// the user picks. Returns the saved file's path.
#[tauri::command]
pub async fn deaths_share(app: A<'_>, handle: tauri::AppHandle, game: String, boss: Option<String>, save: bool) -> R<Option<String>> {
    let img = deaths::share_picture(&app, &game, boss.as_deref())?;
    let png = crate::apps::encode_png(&img.rgba_straight(), img.w() as u32, img.h() as u32).ok_or("Couldn't draw the picture.")?;
    if !save {
        crate::clipboard::set_png(&png)?;
        return Ok(None);
    }
    let name = format!("{}.png", boss.as_deref().unwrap_or("deaths").chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>().trim());
    save_as(&handle, "Save the picture", &name, "PNG picture", "png", &png)
}

/// A game's history (or every game's) as a spreadsheet, saved where the user picks.
#[tauri::command]
pub async fn deaths_export(handle: tauri::AppHandle, game: Option<String>) -> R<Option<String>> {
    let csv = deaths::export_csv(game.as_deref())?;
    save_as(&handle, "Export the death history", "deaths.csv", "Spreadsheet (CSV)", "csv", csv.as_bytes())
}

fn save_as(handle: &tauri::AppHandle, title: &str, name: &str, kind: &str, ext: &str, bytes: &[u8]) -> R<Option<String>> {
    crate::DIALOG.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = crate::file_dialog(handle, title.into()).set_file_name(name).add_filter(kind, &[ext]).blocking_save_file();
    crate::DIALOG.store(false, std::sync::atomic::Ordering::Relaxed);
    let Some(path) = r.and_then(|f| f.into_path().ok()) else { return Ok(None) };
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't save it: {e}"))?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// A note for a boss (empty removes it).
#[tauri::command]
pub async fn deaths_note(app: A<'_>, game: String, boss: String, text: String) -> R<()> {
    deaths::set_note(&app, &game, &boss, &text)
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
