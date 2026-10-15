//! Session locks: a session that is open in Claude Code on one machine can't be continued on another.
//!
//! - Claude lists its running sessions in `<claude>/sessions/<pid>.json`; we keep the ones whose process
//!   is still alive and publish them in our manifest.
//! - The engine writes `<data>/locks.json`: sessions open on other machines, sessions open here that have
//!   newer messages waiting on another machine, and "take over" overrides.
//! - A Claude Code UserPromptSubmit hook runs `deck.exe --claude-sync-hook` (a copy of Deck), which reads that file and blocks
//!   the prompt (exit code 2, message on stderr) when the session belongs to another machine.
use crate::ccsync::model::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// how long a lock read from the cloud folder stays valid without a fresh manifest
pub const CLOUD_LOCK_TTL: i64 = 3 * 60_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RunLock {
    pub started: i64,
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RemoteLock {
    pub machine: String,
    pub started: i64,
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LocksFile {
    pub updated: i64,
    pub enabled: bool,
    /// session id -> the machine that has it open
    pub remote: BTreeMap<String, RemoteLock>,
    /// session id (open here) -> machine with newer messages for it
    pub pending: BTreeMap<String, String>,
    /// session id -> allowed here anyway until (ms)
    pub overrides: BTreeMap<String, i64>,
}

/// `<data>/identity.json`: what the SessionStart hook tells Claude
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Identity {
    pub enabled: bool,
    pub machine: String,
    pub home: String,
    /// the other machines this one syncs with
    pub others: Vec<String>,
    /// session id -> other machines whose messages are in that session's history
    pub visited: BTreeMap<String, Vec<String>>,
}

pub fn write_identity(data: &Path, id: &Identity) {
    let p = data.join("identity.json");
    let body = serde_json::to_vec_pretty(id).unwrap_or_default();
    // only touch the disk when something changed (this runs every cycle)
    if std::fs::read(&p).map(|b| b == body).unwrap_or(false) {
        return;
    }
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// machine names come from synced files: keep them to plain characters before they reach Claude's context
fn clean_name(s: &str) -> String {
    let n: String = s.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')).take(40).collect();
    n.trim().to_string()
}

/// the text added to Claude's context at session start; None = say nothing
pub fn identity_text(id: &Identity, session_id: &str) -> Option<String> {
    let me = clean_name(&id.machine);
    if !id.enabled || me.is_empty() {
        return None;
    }
    let mut t = format!("Deck: this Claude Code session is running on the machine \"{me}\" (Windows, user folder {}).", id.home);
    let others: Vec<String> = id.others.iter().map(|o| clean_name(o)).filter(|o| !o.is_empty() && *o != me).collect();
    if !others.is_empty() {
        t.push_str(&format!(
            " The user also works on: {}. Sessions and memory notes are synced between these machines, so a note about hardware, installed software or files may describe a different machine than this one; check before assuming.",
            others.join(", ")
        ));
    }
    let from: Vec<String> = id.visited.get(session_id).into_iter().flatten().map(|o| clean_name(o)).filter(|o| !o.is_empty() && *o != me).collect();
    if !from.is_empty() {
        t.push_str(&format!(
            " This session also ran on {} earlier, so earlier tool output, file states and system details in this conversation may come from that machine, not this one.",
            from.join(", ")
        ));
    }
    Some(t)
}

fn session_start(data: &Path, v: &serde_json::Value) -> i32 {
    use std::io::Write;
    let Some(id) = std::fs::read(data.join("identity.json")).ok().and_then(|b| serde_json::from_slice::<Identity>(&b).ok()) else { return 0 };
    let sid = v.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
    let Some(text) = identity_text(&id, sid) else { return 0 };
    let out = serde_json::json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": text } });
    let mut so = std::io::stdout();
    let _ = so.write_all(out.to_string().as_bytes());
    let _ = so.flush();
    0
}

#[derive(Deserialize)]
struct SessFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "startedAt", default)]
    started_at: i64,
    #[serde(rename = "procStart", default)]
    proc_start: String,
    #[serde(default)]
    name: String,
}

fn alive(pid: u32, proc_start: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, FILETIME};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        let mut ok = GetExitCodeProcess(h, &mut code) != 0 && code == 259; // STILL_ACTIVE
        if ok {
            // procStart is the process creation FILETIME: rules out a reused pid
            if let Ok(want) = proc_start.parse::<u64>() {
                let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let (mut c, mut e, mut k, mut u) = (z, z, z, z);
                if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                    let got = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
                    ok = got.abs_diff(want) < 50_000_000; // 5 s
                }
            }
        }
        CloseHandle(h);
        ok
    }
}

/// sessions open in Claude Code on this machine right now: session id -> lock
pub fn running(claude: &Path) -> BTreeMap<String, RunLock> {
    let mut out: BTreeMap<String, RunLock> = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(claude.join("sessions")) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "json").unwrap_or(true) {
            continue;
        }
        let Some(s) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<SessFile>(&b).ok()) else { continue };
        if s.session_id.is_empty() || !alive(s.pid, &s.proc_start) {
            continue;
        }
        let started = if s.started_at > 0 { s.started_at } else { now_ms() };
        let e = out.entry(s.session_id).or_insert(RunLock { started, name: s.name.clone() });
        e.started = e.started.min(started);
    }
    out
}

pub fn read_file(data: &Path) -> Option<LocksFile> {
    std::fs::read(data.join("locks.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn write_file(data: &Path, f: &LocksFile) {
    let p = data.join("locks.json");
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(f).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

pub fn sync_event_name(data: &Path) -> String {
    format!("DeckClaudeSync_sync_{}", &crate::ccsync::crypto::hash(data.to_string_lossy().to_lowercase().as_bytes())[..12])
}

fn signal_sync(data: &Path) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenEventW, SetEvent};
    unsafe {
        let h = OpenEventW(0x0002, 0, crate::ccsync::winutil::wide(&sync_event_name(data)).as_ptr()); // EVENT_MODIFY_STATE
        if !h.is_null() {
            SetEvent(h);
            CloseHandle(h);
        }
    }
}

/// `deck.exe --claude-sync-hook`: Claude Code UserPromptSubmit hook. Returns the process exit code
/// (0 = let the prompt through, 2 = block it and show stderr). Anything unexpected lets it through.
pub fn hook(data: &Path, claude: &Path) -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().take(4 << 20).read_to_string(&mut input);
    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    if v.get("hook_event_name").and_then(|e| e.as_str()) == Some("SessionStart") {
        return session_start(data, &v);
    }
    let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) else { return 0 };
    let Some(lf) = read_file(data) else { return 0 };
    let now = now_ms();
    // Deck's Claude Sync not running (or paused for a while): never get in the way
    if !lf.enabled || now - lf.updated > 5 * 60_000 {
        return 0;
    }
    if lf.overrides.get(sid).map(|&t| t > now).unwrap_or(false) {
        return 0;
    }
    let here_since = running(claude).get(sid).map(|r| r.started).unwrap_or(now);
    if let Some(r) = lf.remote.get(sid) {
        // if both opened it, the one that opened it first keeps it
        if r.started <= here_since {
            eprintln!(
                "Deck (Claude Code sync): this session is open on {} (since {}), so it can't be continued here too.\n\
                 Close it on {} first (exit Claude there or close that tab), wait a few seconds, then /resume it here again so you get its latest messages.\n\
                 To use it here anyway, click Take over on Deck's Claude Code sync page.",
                r.machine,
                crate::ccsync::config::clock(r.started / 1000),
                r.machine
            );
            return 2;
        }
    }
    if let Some(m) = lf.pending.get(sid) {
        signal_sync(data);
        eprintln!(
            "Deck (Claude Code sync): {m} added messages to this session after it was opened here, so this copy is out of date.\n\
             Leave this session (/clear, /resume another one, or exit Claude), wait a few seconds for Deck to update it, then /resume it again.\n\
             To continue this older copy anyway, click Take over on Deck's Claude Code sync page."
        );
        return 2;
    }
    0
}

/// path of the hook exe for a home folder (kept home-relative so the synced settings.json is the same on every PC)
pub fn hook_exe(home: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(home).join(r"AppData\Local\Deck\claude-sync-hook.exe")
}

pub fn hook_command(home: &str, extra: &str) -> String {
    let p = hook_exe(home).display().to_string().replace('\\', "/");
    format!("\"{p}\" --claude-sync-hook{extra}")
}

/// keeps a copy of this exe where the hook expects it
pub fn install_exe(home: &str) -> Result<(), String> {
    let dst = hook_exe(home);
    let src = std::env::current_exe().map_err(|e| e.to_string())?;
    if src.canonicalize().ok() == dst.canonicalize().ok() {
        return Ok(());
    }
    let (sm, dm) = (std::fs::metadata(&src).map_err(|e| e.to_string())?, std::fs::metadata(&dst).ok());
    let same = dm.map(|d| d.len() == sm.len() && d.modified().ok() == sm.modified().ok()).unwrap_or(false);
    if same {
        return Ok(());
    }
    std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
    // the old copy may be running as a hook this very moment: move it aside instead of overwriting
    let old = dst.with_extension("old.exe");
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::rename(&dst, &old);
    std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
    if let (Ok(f), Ok(t)) = (std::fs::File::options().write(true).open(&dst), sm.modified()) {
        let _ = f.set_modified(t);
    }
    Ok(())
}

fn is_ours(h: &serde_json::Value) -> bool {
    h.get("command").and_then(|c| c.as_str()).map(|c| c.contains("--claude-sync-hook")).unwrap_or(false)
}

/// adds (Some(command)) or removes (None) our hook for `event` in settings.json; true if it changed
pub fn set_hook(claude: &Path, event: &str, command: Option<&str>) -> Result<bool, String> {
    let file = claude.join("settings.json");
    let orig: serde_json::Value = match std::fs::read(&file) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("settings.json isn't valid JSON ({e}); not touching it"))?,
        Err(_) if command.is_none() => return Ok(false),
        Err(_) => serde_json::json!({}),
    };
    // removing something that was never added: leave the file exactly as it is
    if command.is_none() && !orig.get("hooks").and_then(|h| h.get(event)).map(|l| l.is_array()).unwrap_or(false) {
        return Ok(false);
    }
    let mut v = orig.clone();
    let Some(obj) = v.as_object_mut() else { return Err("settings.json isn't a JSON object".into()) };
    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let Some(hooks) = hooks.as_object_mut() else { return Err("settings.json \"hooks\" isn't an object".into()) };
    let list = hooks.entry(event).or_insert_with(|| serde_json::json!([]));
    let Some(list) = list.as_array_mut() else { return Err(format!("{event} hooks aren't a list")) };
    // drop our old entries, keep everyone else's
    for g in list.iter_mut() {
        if let Some(inner) = g.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            inner.retain(|h| !is_ours(h));
        }
    }
    list.retain(|g| g.get("hooks").and_then(|h| h.as_array()).map(|a| !a.is_empty()).unwrap_or(true));
    if let Some(cmd) = command {
        list.push(serde_json::json!({ "hooks": [ { "type": "command", "command": cmd, "timeout": 10 } ] }));
    }
    if list.is_empty() {
        hooks.remove(event);
    }
    if hooks.is_empty() {
        obj.remove("hooks");
    }
    if v == orig {
        return Ok(false);
    }
    let tmp = file.with_extension("json.ccsync-tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_add_remove_keeps_other_hooks() {
        let d = std::env::temp_dir().join(format!("ccsync-test-{}", now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), r#"{"model":"opus","hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"other.exe"}]}]}}"#).unwrap();
        let cmd = "\"C:/x/claude-sync-hook.exe\" --claude-sync-hook";
        assert!(set_hook(&d, "UserPromptSubmit", Some(cmd)).unwrap());
        assert!(!set_hook(&d, "UserPromptSubmit", Some(cmd)).unwrap());
        assert!(set_hook(&d, "SessionStart", Some(cmd)).unwrap());
        assert!(!set_hook(&d, "SessionStart", Some(cmd)).unwrap());
        let s = std::fs::read_to_string(d.join("settings.json")).unwrap();
        assert!(s.contains("other.exe") && s.contains("claude-sync-hook.exe") && s.contains("SessionStart"));
        // key order is kept
        assert!(s.find("model").unwrap() < s.find("hooks").unwrap());
        // removing one event leaves the other and the user's hook alone
        assert!(set_hook(&d, "SessionStart", None).unwrap());
        let s = std::fs::read_to_string(d.join("settings.json")).unwrap();
        assert!(!s.contains("SessionStart") && s.contains("UserPromptSubmit") && s.contains("other.exe"));
        assert!(set_hook(&d, "UserPromptSubmit", None).unwrap());
        let s = std::fs::read_to_string(d.join("settings.json")).unwrap();
        assert!(s.contains("other.exe") && !s.contains("claude-sync-hook"));
        // removing what isn't there doesn't rewrite the file
        let before = std::fs::read(d.join("settings.json")).unwrap();
        assert!(!set_hook(&d, "SessionStart", None).unwrap());
        assert_eq!(before, std::fs::read(d.join("settings.json")).unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn identity_text_rules() {
        let mut id = Identity { enabled: true, machine: "LAPTOP".into(), home: r"C:\Users\sam".into(), ..Default::default() };
        let t = identity_text(&id, "s1").unwrap();
        assert!(t.contains("\"LAPTOP\"") && t.contains(r"C:\Users\sam") && !t.contains("also works on") && !t.contains("also ran"));
        id.others = vec!["PC".into(), "LAPTOP".into(), "bad\"name\n".into()];
        id.visited.insert("s1".into(), vec!["PC".into()]);
        let t = identity_text(&id, "s1").unwrap();
        assert!(t.contains("also works on: PC, badname.") && t.contains("also ran on PC earlier"));
        // another session has no history note
        assert!(!identity_text(&id, "s2").unwrap().contains("also ran"));
        id.enabled = false;
        assert!(identity_text(&id, "s1").is_none());
    }
}
