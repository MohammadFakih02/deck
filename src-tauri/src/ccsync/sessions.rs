//! What the Sessions page shows about a session: its title, what was asked first and last, how many
//! prompts it holds. Read from the session log (`<uuid>.jsonl`) and published in the manifest so each
//! machine can list the other's sessions too.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SessionMeta {
    /// Claude's own title for the conversation (or the one the user gave it)
    pub title: String,
    pub first: String,
    pub last: String,
    /// prompts the user typed
    pub prompts: u32,
    pub size: u64,
    /// last change, unix millis
    pub mtime: i64,
    /// project alias
    pub alias: String,
}

/// a Claude Code session id (a 36 character uuid)
pub fn is_sid(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// one line of text, cut to `n` characters
fn tidy(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        let mut t: String = one.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// the text of a prompt the user typed; None for tool results, system notes, command echoes and the
/// summary Claude writes when a conversation is continued
fn prompt_text(v: &serde_json::Value) -> Option<String> {
    if v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false) || v.get("isSidechain").and_then(|x| x.as_bool()).unwrap_or(false) || v.get("isCompactSummary").and_then(|x| x.as_bool()).unwrap_or(false) {
        return None;
    }
    let c = v.get("message")?.get("content")?;
    let text = match c {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text")).and_then(|b| b.get("text")).and_then(|t| t.as_str())?.to_string(),
        _ => return None,
    };
    let t = text.trim();
    if t.is_empty() || t.starts_with('<') || t.starts_with("This session is being continued") || t.starts_with("Caveat:") {
        return None;
    }
    Some(t.to_string())
}

/// reads the session log; size/mtime/alias are filled in by the caller
pub fn read_meta(path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read(path).ok()?;
    let mut m = SessionMeta::default();
    let mut ai = String::new();
    let mut custom = String::new();
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if memchr::memmem::find(line, b"\"type\":\"user\"").is_some() {
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else { continue };
            if v.get("type").and_then(|t| t.as_str()) != Some("user") {
                continue;
            }
            if let Some(t) = prompt_text(&v) {
                m.prompts += 1;
                if m.first.is_empty() {
                    m.first = tidy(&t, 160);
                }
                m.last = tidy(&t, 160);
            }
        } else if memchr::memmem::find(line, b"\"type\":\"ai-title\"").is_some() {
            if let Some(t) = serde_json::from_slice::<serde_json::Value>(line).ok().and_then(|v| v.get("aiTitle").and_then(|t| t.as_str()).map(|s| s.to_string())) {
                ai = t;
            }
        } else if memchr::memmem::find(line, b"\"type\":\"custom-title\"").is_some() {
            if let Some(t) = serde_json::from_slice::<serde_json::Value>(line).ok().and_then(|v| v.get("customTitle").and_then(|t| t.as_str()).map(|s| s.to_string())) {
                custom = t;
            }
        }
    }
    m.title = tidy(if !custom.is_empty() { &custom } else { &ai }, 90);
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_title_and_prompts() {
        let d = std::env::temp_dir().join(format!("ccs-sess-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.jsonl");
        let lines = [
            r#"{"type":"queue-operation","operation":"enqueue"}"#,
            r#"{"type":"user","message":{"role":"user","content":"<local-command-caveat>x</local-command-caveat>"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"  fix the   tray icon\nplease "}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"done"}]}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"second ask"}}"#,
            r#"{"type":"ai-title","aiTitle":"Tray icon fix"}"#,
            r#"{"type":"assistant","message":{"content":"quoting \"type\":\"user\" in text"}}"#,
        ];
        std::fs::write(&f, lines.join("\n") + "\n").unwrap();
        let m = read_meta(&f).unwrap();
        assert_eq!(m.title, "Tray icon fix");
        assert_eq!(m.first, "fix the tray icon please");
        assert_eq!(m.last, "second ask");
        assert_eq!(m.prompts, 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sid_check() {
        assert!(is_sid("90437ea6-d374-41f2-ba6e-fa90552ba6f1"));
        assert!(!is_sid("../../x"));
        assert!(!is_sid("90437ea6"));
    }

    #[test]
    fn tidy_cuts_on_chars() {
        assert_eq!(tidy("ééééé", 3), "éé…");
        assert_eq!(tidy("a  b", 10), "a b");
    }
}
