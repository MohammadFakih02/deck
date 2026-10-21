//! The MCP servers part of Claude Code's `.claude.json`: the user's own (`mcpServers`) and each
//! project's (`projects.<path>.mcpServers`). The rest of that file (sign-in, caches, per-PC state)
//! never leaves this PC.
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// `.claude.json`: in the home folder, or inside the Claude folder when CLAUDE_CONFIG_DIR moved it.
pub fn claude_json(claude: &Path, home: &str) -> PathBuf {
    let default = Path::new(home).join(".claude");
    if claude.eq(&default) {
        Path::new(home).join(".claude.json")
    } else {
        claude.join(".claude.json")
    }
}

fn servers(v: Option<&Value>) -> Option<Map<String, Value>> {
    v.and_then(Value::as_object).filter(|m| !m.is_empty()).cloned()
}

/// `{"user": {...}, "projects": {"<path>": {...}}}`, leaving out empty lists.
pub fn extract(doc: &Value) -> Value {
    let mut out = Map::new();
    if let Some(u) = servers(doc.get("mcpServers")) {
        out.insert("user".into(), Value::Object(u));
    }
    let mut projects = Map::new();
    if let Some(ps) = doc.get("projects").and_then(Value::as_object) {
        for (path, p) in ps {
            if let Some(s) = servers(p.get("mcpServers")) {
                projects.insert(path.clone(), Value::Object(s));
            }
        }
    }
    if !projects.is_empty() {
        out.insert("projects".into(), Value::Object(projects));
    }
    Value::Object(out)
}

pub fn is_empty(v: &Value) -> bool {
    v.as_object().map(|m| m.is_empty()).unwrap_or(true)
}

/// The same bytes for the same servers on every PC.
pub fn to_bytes(v: &Value) -> Vec<u8> {
    fn sorted(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                Value::Object(keys.into_iter().map(|k| (k.clone(), sorted(&m[k]))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
            x => x.clone(),
        }
    }
    let mut b = serde_json::to_vec_pretty(&sorted(v)).unwrap_or_default();
    b.push(b'\n');
    b
}

/// Both lists together; `b` wins where both have a server of the same name.
pub fn union(a: &Value, b: &Value) -> Value {
    let mut out = a.clone();
    let o = out.as_object_mut().expect("extract gives an object");
    if let Some(u) = b.get("user").and_then(Value::as_object) {
        let e = o.entry("user").or_insert_with(|| json!({}));
        for (k, v) in u {
            e[k] = v.clone();
        }
    }
    if let Some(ps) = b.get("projects").and_then(Value::as_object) {
        let e = o.entry("projects").or_insert_with(|| json!({}));
        for (path, s) in ps {
            let pe = e.as_object_mut().unwrap().entry(path.clone()).or_insert_with(|| json!({}));
            for (k, v) in s.as_object().into_iter().flatten() {
                pe[k] = v.clone();
            }
        }
    }
    out
}

/// Puts the servers of `x` (an `extract`) into `.claude.json`'s `doc`, replacing the ones there.
pub fn apply(doc: &mut Value, x: &Value) {
    let Some(d) = doc.as_object_mut() else { return };
    d.insert("mcpServers".into(), x.get("user").cloned().unwrap_or_else(|| json!({})));
    let want = x.get("projects").and_then(Value::as_object).cloned().unwrap_or_default();
    let ps = d.entry("projects").or_insert_with(|| json!({}));
    let Some(ps) = ps.as_object_mut() else { return };
    for (path, p) in ps.iter_mut() {
        if !want.contains_key(path) && servers(p.get("mcpServers")).is_some() {
            p["mcpServers"] = json!({});
        }
    }
    for (path, s) in want {
        let p = ps.entry(path).or_insert_with(|| json!({}));
        if p.is_object() {
            p["mcpServers"] = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_servers_travel_and_come_back() {
        let doc = json!({ "oauthAccount": { "email": "x" }, "mcpServers": { "fs": { "command": "npx" } },
            "projects": { "C:/dev/deck": { "allowedTools": [], "mcpServers": { "db": { "command": "db.exe" } } }, "C:/other": { "mcpServers": {} } } });
        let x = extract(&doc);
        assert_eq!(x, json!({ "user": { "fs": { "command": "npx" } }, "projects": { "C:/dev/deck": { "db": { "command": "db.exe" } } } }));
        assert!(!String::from_utf8(to_bytes(&x)).unwrap().contains("email"));
        let mut there = json!({ "oauthAccount": { "email": "y" }, "projects": { "C:/old": { "mcpServers": { "gone": {} } } } });
        apply(&mut there, &x);
        assert_eq!(there["oauthAccount"]["email"], "y", "the rest is untouched");
        assert_eq!(extract(&there), x);
        assert_eq!(there["projects"]["C:/old"]["mcpServers"], json!({}), "removed there too");
    }

    #[test]
    fn first_time_keeps_both() {
        let a = json!({ "user": { "a": 1, "same": 1 } });
        let b = json!({ "user": { "b": 2, "same": 2 }, "projects": { "C:/p": { "c": 3 } } });
        assert_eq!(union(&a, &b), json!({ "user": { "a": 1, "b": 2, "same": 2 }, "projects": { "C:/p": { "c": 3 } } }));
        assert!(is_empty(&extract(&json!({}))));
    }
}
