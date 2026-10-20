//! Synced Claude Code settings can make Claude Code run programs (hooks, the status line, helper
//! commands, plugins) and widen what it may do without asking (permissions, environment). Whoever
//! has the sync key could use that to run code on every synced PC, so an incoming change that
//! touches any of it waits until the user allows it on this PC (see `Shared::held_decision`).

/// Whether writing `incoming` over `current` (None: no file here yet) at `rel` (relative to the
/// Claude folder, '/'-separated) could change what Claude Code runs or may do on this PC.
pub fn needs_approval(rel: &str, current: Option<&[u8]>, incoming: &[u8]) -> bool {
    let mut low = rel.to_ascii_lowercase();
    // a project's own Claude Code settings, hooks and MCP servers (`.claude/` and `.mcp.json` in the
    // project folder) run things just like the user's
    if let Some((_, rest)) = crate::ccsync::scan::code_parts(&low) {
        low = match rest {
            ".claude/settings.json" => "settings.json".into(),
            ".mcp.json" => crate::ccsync::scan::MCP_FILE.into(),
            r if r.starts_with(".claude/hooks/") => format!("hooks/{}", &r[".claude/hooks/".len()..]),
            _ => return false,
        };
    }
    // hook scripts, plugins and MCP servers are programs Claude Code runs
    if low.starts_with("hooks/") || low.starts_with("plugins/") || low == crate::ccsync::scan::MCP_FILE {
        return current != Some(incoming);
    }
    if low != "settings.json" {
        return false;
    }
    // one Claude Code can't read: nothing to compare, so it waits too
    let Ok(new) = serde_json::from_slice::<Value>(incoming) else { return true };
    let old = current.and_then(|b| serde_json::from_slice::<Value>(b).ok()).unwrap_or(Value::Null);
    RUNS.iter().any(|k| part(&old, k) != part(&new, k))
}

use serde_json::Value;

/// settings.json keys that run something or change what Claude Code may do without asking.
const RUNS: &[&str] = &[
    "hooks",
    "disableAllHooks",
    "statusLine",
    "apiKeyHelper",
    "awsAuthRefresh",
    "awsCredentialExport",
    "otelHeadersHelper",
    "env",
    "permissions",
    "sandbox",
    "enabledPlugins",
    "extraKnownMarketplaces",
    "enableAllProjectMcpServers",
    "enabledMcpjsonServers",
    "disabledMcpjsonServers",
];

/// `key` of a settings object, without Deck's own Claude Sync hooks (the same on every PC, added
/// and removed by Deck itself).
fn part(settings: &Value, key: &str) -> Value {
    let mut v = settings.get(key).cloned().unwrap_or(Value::Null);
    if key == "hooks" {
        if let Some(events) = v.as_object_mut() {
            for list in events.values_mut() {
                if let Some(groups) = list.as_array_mut() {
                    for g in groups.iter_mut() {
                        if let Some(inner) = g.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                            inner.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(|c| c.contains("--claude-sync-hook")));
                        }
                    }
                    groups.retain(|g| g.get("hooks").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(true));
                }
            }
            events.retain(|_, list| list.as_array().map(|a| !a.is_empty()).unwrap_or(true));
            if events.is_empty() {
                v = Value::Null;
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::needs_approval;

    const BASE: &str = r#"{"model":"opus","hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"%USERPROFILE%\\AppData\\Local\\Deck\\claude-sync-hook.exe --claude-sync-hook"}]}]},"permissions":{"allow":["Bash(git status)"]}}"#;

    #[test]
    fn ordinary_settings_changes_sync_freely() {
        let theme = BASE.replace(r#""model":"opus""#, r#""model":"sonnet","theme":"dark""#);
        assert!(!needs_approval("settings.json", Some(BASE.as_bytes()), theme.as_bytes()));
        assert!(!needs_approval("CLAUDE.md", Some(b"old"), b"new"));
        assert!(!needs_approval("commands/review.md", None, b"Review the diff"));
        assert!(!needs_approval("projects/@GAME/memory/notes.md", Some(b"a"), b"b"));
    }

    #[test]
    fn anything_that_runs_or_widens_permissions_waits() {
        let hook = BASE.replace(r#""permissions""#, r#""statusLine":{"type":"command","command":"evil.exe"},"permissions""#);
        assert!(needs_approval("settings.json", Some(BASE.as_bytes()), hook.as_bytes()));
        let new_hook = BASE.replace(r#"[{"hooks":[{"type":"command","command":"%USERPROFILE%"#, r#"[{"hooks":[{"type":"command","command":"calc.exe"}]},{"hooks":[{"type":"command","command":"%USERPROFILE%"#);
        assert!(needs_approval("settings.json", Some(BASE.as_bytes()), new_hook.as_bytes()));
        let allow = BASE.replace("Bash(git status)", "Bash(*)");
        assert!(needs_approval("settings.json", Some(BASE.as_bytes()), allow.as_bytes()));
        let env = BASE.replace(r#""model":"opus""#, r#""model":"opus","env":{"NODE_OPTIONS":"--require x"}"#);
        assert!(needs_approval("settings.json", Some(BASE.as_bytes()), env.as_bytes()));
        // a first settings.json with any of it, or one Claude Code couldn't read, waits too
        assert!(needs_approval("settings.json", None, BASE.as_bytes()));
        assert!(needs_approval("settings.json", Some(BASE.as_bytes()), b"{not json"));
        // hook scripts and plugins are programs
        assert!(needs_approval("hooks/pre.ps1", Some(b"echo hi"), b"rm -r ~"));
        assert!(needs_approval("plugins/marketplaces/x/plugin.json", None, b"{}"));
        assert!(!needs_approval("hooks/pre.ps1", Some(b"same"), b"same"));
        assert!(needs_approval("deck-mcp.json", None, br#"{"user":{"x":{"command":"x.exe"}}}"#));
        // the same inside a project's folder
        assert!(needs_approval("code/@deck/.mcp.json", None, b"{}"));
        assert!(needs_approval("code/@deck/.claude/hooks/a.ps1", Some(b"a"), b"b"));
        assert!(needs_approval("code/@deck/.claude/settings.json", None, BASE.as_bytes()));
        assert!(!needs_approval("code/@deck/.claude/settings.json", Some(BASE.as_bytes()), BASE.replacen("{", r#"{"theme":"dark","#, 1).as_bytes()));
        assert!(!needs_approval("code/@deck/src/main.rs", Some(b"a"), b"b"));
    }

    /// A held change waits for the user; "apply" lets exactly that version through, "keep this
    /// PC's" keeps it out without asking again, and a newer version asks again. Kept across restarts.
    #[test]
    fn held_changes_wait_for_a_decision() {
        use crate::ccsync::config::{Paths, Shared};
        let tmp = std::env::temp_dir().join(format!("deck-held-{}", crate::util::now_ms()));
        let paths = || Paths::new(Some(tmp.join("claude").display().to_string()), Some(tmp.join("data").display().to_string()), None);
        std::fs::create_dir_all(tmp.join("claude")).unwrap();
        let sh = Shared::new(paths());
        assert_eq!(sh.held_decision("settings.json", "h1", "LAPTOP"), None);
        assert_eq!(sh.undecided(), vec![("settings.json".to_string(), "LAPTOP".to_string())]);
        assert_eq!(sh.held_decision("settings.json", "h1", "LAPTOP"), None, "still waiting");
        sh.decide_held(true);
        assert_eq!(sh.held_decision("settings.json", "h1", "LAPTOP"), Some(true));
        sh.held_done("settings.json");
        assert_eq!(sh.held_decision("hooks/a.ps1", "h2", "LAPTOP"), None);
        sh.decide_held(false);
        let again = Shared::new(paths());
        assert_eq!(again.held_decision("hooks/a.ps1", "h2", "LAPTOP"), Some(false), "kept this PC's, after a restart too");
        assert_eq!(again.held_decision("hooks/a.ps1", "h3", "LAPTOP"), None, "a newer version asks again");
        assert!(again.undecided().len() == 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Deck's own Claude Sync hooks (the same on every PC) coming or going don't need asking.
    #[test]
    fn decks_own_sync_hooks_are_not_asked_about() {
        let without = r#"{"model":"opus","permissions":{"allow":["Bash(git status)"]}}"#;
        assert!(!needs_approval("settings.json", Some(without.as_bytes()), BASE.as_bytes()));
        assert!(!needs_approval("settings.json", Some(BASE.as_bytes()), without.as_bytes()));
    }
}
