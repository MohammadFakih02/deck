//! The layouts of the phone's on-screen keyboard (controller page → Keyboard), kept on the PC so every
//! phone and Deck's own window see the same ones: %APPDATA%\Deck\phone-keys.json.
//!
//! The phone owns what a layout looks like (its buttons: key, combo, mouse, wheel, text, macro, trackpad;
//! where they sit; their size and colour). The PC only keeps the file: it checks the shape and the size,
//! counts the revision, and writes it whole, so a damaged or oversized upload can't take the others with it.

use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const MAX_BYTES: usize = 512 * 1024;
const MAX_LAYOUTS: usize = 60;
const MAX_ITEMS: usize = 300;
const MAX_GAMES: usize = 20;

fn file() -> PathBuf {
    crate::util::appdata().join("Deck").join("phone-keys.json")
}

pub fn empty() -> Value {
    json!({ "v": 1, "rev": 0, "auto": true, "active": "", "layouts": [] })
}

/// One write at a time: two phones saving together can't interleave their revisions.
static WRITE: Mutex<()> = parking_lot::const_mutex(());

pub fn load() -> Value {
    if cfg!(test) {
        return empty();
    }
    let _g = WRITE.lock();
    read()
}

fn read() -> Value {
    std::fs::read(file()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| clean(&v)).unwrap_or_else(empty)
}

fn text(v: &Value, max: usize) -> String {
    v.as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(max).collect()
}

/// The document with only well-formed layouts: each has an id, a name, the games it is for (lower case) and
/// its buttons (objects). Anything else is dropped.
pub fn clean(v: &Value) -> Value {
    let mut layouts = Vec::new();
    for l in v["layouts"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().take(MAX_LAYOUTS) {
        let id = text(&l["id"], 40);
        if id.is_empty() || layouts.iter().any(|x: &Value| x["id"] == id) {
            continue;
        }
        let games: Vec<String> = l["games"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().map(|g| text(g, 80).trim().to_lowercase()).filter(|g| !g.is_empty()).take(MAX_GAMES).collect();
        let items: Vec<Value> = l["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().filter(|i| i.is_object()).take(MAX_ITEMS).cloned().collect();
        let mut o = Map::new();
        // (settings of the layout itself, such as its size and opacity, are kept as the phone wrote them)
        if let Some(src) = l.as_object() {
            for (k, val) in src {
                if !["id", "name", "games", "items"].contains(&k.as_str()) && (val.is_number() || val.is_boolean() || val.as_str().is_some_and(|s| s.len() <= 80)) {
                    o.insert(k.clone(), val.clone());
                }
            }
        }
        o.insert("id".into(), json!(id));
        o.insert("name".into(), json!(text(&l["name"], 60)));
        o.insert("games".into(), json!(games));
        o.insert("items".into(), json!(items));
        layouts.push(Value::Object(o));
    }
    let active = text(&v["active"], 40);
    let active = if layouts.iter().any(|l| l["id"] == active) { active } else { String::new() };
    json!({ "v": 1, "rev": v["rev"].as_u64().unwrap_or(0), "auto": v["auto"].as_bool().unwrap_or(true), "active": active, "layouts": layouts })
}

/// Stores `doc` (cleaned) with the next revision, and returns what was stored.
pub fn save(doc: &Value) -> Result<Value, String> {
    let mut c = clean(doc);
    if serde_json::to_vec(&c).map(|b| b.len()).unwrap_or(usize::MAX) > MAX_BYTES {
        return Err("The layouts are too big to save (more than 512 KB): remove some buttons or layouts".into());
    }
    if cfg!(test) {
        return Ok(c);
    }
    let _g = WRITE.lock();
    let rev = read()["rev"].as_u64().unwrap_or(0) + 1;
    c["rev"] = json!(rev);
    let path = file();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| format!("The layouts couldn't be saved: {e}"))?;
    crate::util::write_atomic(&path, &serde_json::to_vec(&c).unwrap_or_default()).map_err(|e| format!("The layouts couldn't be saved: {e}"))?;
    Ok(c)
}

/// What Deck's own window does to the layouts: rename, delete, tie to games, switch the automatic choice,
/// import (pasted layout JSON, as `export` gives it). Returns the new document, or for `export` the layout.
pub fn act(what: &str, a: &Value) -> Result<Value, String> {
    if what == "state" {
        return Ok(load());
    }
    let doc = if cfg!(test) { empty() } else { load() };
    let (out, changed) = act_on(doc, what, a)?;
    if changed {
        save(&out)
    } else {
        Ok(out)
    }
}

/// `act` on a document: the result and whether it changed (then it is saved).
pub fn act_on(mut doc: Value, what: &str, a: &Value) -> Result<(Value, bool), String> {
    let id = text(&a["id"], 40);
    let pos = doc["layouts"].as_array().and_then(|l| l.iter().position(|x| x["id"] == id));
    let layouts = |d: &mut Value| d["layouts"].as_array_mut().map(std::mem::take).unwrap_or_default();
    match what {
        "rename" => {
            let name = text(&a["name"], 40);
            let (Some(i), false) = (pos, name.trim().is_empty()) else { return Err("There is no such layout, or the name is empty".into()) };
            doc["layouts"][i]["name"] = json!(name.trim());
        }
        "delete" => {
            let Some(i) = pos else { return Err("There is no such layout".into()) };
            doc["layouts"].as_array_mut().map(|l| l.remove(i));
        }
        "games" => {
            let Some(i) = pos else { return Err("There is no such layout".into()) };
            let games: Vec<String> = a["games"].as_array().map(|g| g.as_slice()).unwrap_or(&[]).iter().map(|g| text(g, 80).trim().to_lowercase()).filter(|g| !g.is_empty()).collect();
            // a game has one layout
            let mut all = layouts(&mut doc);
            for (j, l) in all.iter_mut().enumerate() {
                if j != i {
                    let keep: Vec<Value> = l["games"].as_array().map(|g| g.as_slice()).unwrap_or(&[]).iter().filter(|g| !games.iter().any(|x| g.as_str() == Some(x))).cloned().collect();
                    l["games"] = json!(keep);
                }
            }
            all[i]["games"] = json!(games);
            doc["layouts"] = Value::Array(all);
        }
        "auto" => doc["auto"] = json!(a["on"].as_bool().unwrap_or(true)),
        "export" => {
            let Some(i) = pos else { return Err("There is no such layout".into()) };
            let mut l = doc["layouts"][i].clone();
            l.as_object_mut().map(|o| o.remove("id"));
            return Ok((json!({ "deck_keyboard_layout": 1, "layout": l }), false));
        }
        "import" => {
            let raw = a["text"].as_str().unwrap_or("");
            let v: Value = serde_json::from_str(raw).map_err(|_| "That is not a layout: paste the text Export gave".to_string())?;
            let l = if v["deck_keyboard_layout"].is_number() { v["layout"].clone() } else { v };
            if !l["items"].is_array() {
                return Err("That is not a layout: it has no buttons".into());
            }
            let mut all = layouts(&mut doc);
            if all.len() >= MAX_LAYOUTS {
                return Err("There are too many layouts already: delete one first".into());
            }
            let mut l = l;
            let n = all.len();
            l["id"] = json!(format!("i{:x}{n}", crate::util::now_s() % 0xfffff));
            if text(&l["name"], 40).trim().is_empty() {
                l["name"] = json!("Imported layout");
            }
            // the games of an imported layout are not taken from it: the same game may be tied elsewhere
            l["games"] = json!([]);
            all.push(l);
            doc["layouts"] = Value::Array(all);
        }
        _ => return Err("Unknown action".into()),
    }
    Ok((clean(&doc), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Value {
        clean(&json!({ "layouts": [
            { "id": "a", "name": "Keyboard", "games": [], "items": [{ "type": "key", "keys": ["w"] }] },
            { "id": "b", "name": "Skyrim", "games": ["skyrim"], "items": [] }
        ], "active": "a" }))
    }

    #[test]
    fn rename_delete_and_games() {
        let (d, ch) = act_on(two(), "rename", &json!({ "id": "a", "name": "  Typing  " })).unwrap();
        assert!(ch && d["layouts"][0]["name"] == json!("Typing"));
        assert!(act_on(two(), "rename", &json!({ "id": "zz", "name": "x" })).is_err());
        assert!(act_on(two(), "rename", &json!({ "id": "a", "name": "  " })).is_err());
        let (d, _) = act_on(two(), "delete", &json!({ "id": "b" })).unwrap();
        assert_eq!(d["layouts"].as_array().unwrap().len(), 1);
        // a game moves to the layout it is given to
        let (d, _) = act_on(two(), "games", &json!({ "id": "a", "games": ["Skyrim", " Fallout 4 "] })).unwrap();
        assert_eq!(d["layouts"][0]["games"], json!(["skyrim", "fallout 4"]));
        assert_eq!(d["layouts"][1]["games"], json!([]));
        let (d, _) = act_on(two(), "auto", &json!({ "on": false })).unwrap();
        assert_eq!(d["auto"], json!(false));
        assert!(act_on(two(), "nonsense", &json!({})).is_err());
    }

    #[test]
    fn a_layout_goes_out_and_comes_back_in() {
        let (out, ch) = act_on(two(), "export", &json!({ "id": "a" })).unwrap();
        assert!(!ch && out["layout"].get("id").is_none());
        let text = out.to_string();
        let (d, ch) = act_on(two(), "import", &json!({ "text": text })).unwrap();
        let l = d["layouts"].as_array().unwrap();
        assert!(ch && l.len() == 3 && l[2]["name"] == json!("Keyboard") && l[2]["items"].as_array().unwrap().len() == 1);
        assert_ne!(l[2]["id"], l[0]["id"]);
        assert_eq!(l[2]["games"], json!([]));
        // a bare layout works too; rubbish does not
        assert!(act_on(two(), "import", &json!({ "text": r#"{"name":"Mine","items":[]}"# })).is_ok());
        assert!(act_on(two(), "import", &json!({ "text": "hello" })).is_err());
        assert!(act_on(two(), "import", &json!({ "text": r#"{"name":"x"}"# })).is_err());
    }

    #[test]
    fn only_well_formed_layouts_are_kept() {
        let doc = json!({
            "layouts": [
                { "id": "a", "name": "Skyrim", "games": [" Skyrim ", "", "SkyrimSE"], "items": [{ "type": "key", "keys": ["w"] }, 5, "x"], "scale": 1.2, "junk": { "deep": 1 } },
                { "id": "a", "name": "twice" },
                { "name": "no id" },
                { "id": "b", "name": "B\u{7}ell", "items": "nope" }
            ],
            "active": "zzz"
        });
        let c = clean(&doc);
        let l = c["layouts"].as_array().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0]["games"], json!(["skyrim", "skyrimse"]));
        assert_eq!(l[0]["items"].as_array().unwrap().len(), 1);
        assert_eq!(l[0]["scale"], json!(1.2));
        assert!(l[0].get("junk").is_none());
        assert_eq!(l[1]["name"], json!("Bell"));
        assert_eq!(l[1]["items"], json!([]));
        // an active layout that is gone is none
        assert_eq!(c["active"], json!(""));
    }

    #[test]
    fn too_big_a_document_is_refused() {
        let items: Vec<Value> = (0..300).map(|i| json!({ "type": "text", "text": "x".repeat(80), "i": i })).collect();
        let layouts: Vec<Value> = (0..60).map(|i| json!({ "id": format!("l{i}"), "name": "n", "items": items })).collect();
        assert!(save(&json!({ "layouts": layouts })).is_err());
        assert!(save(&json!({ "layouts": [{ "id": "a", "name": "n" }] })).is_ok());
    }
}
