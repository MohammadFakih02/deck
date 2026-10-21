//! What one machine publishes: every synced file under its canonical path, split into 1 MiB chunks.
//! Session logs only ever grow, so an update usually means just the last chunk or two are new.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CHUNK: usize = 1 << 20;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    /// blake3 of the canonical content ("" for a deletion)
    pub hash: String,
    pub size: u64,
    /// unix millis
    pub mtime: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chunks: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Manifest {
    pub machine_id: String,
    pub machine_name: String,
    pub generated_at: i64,
    /// alias -> project path on that machine
    pub aliases: BTreeMap<String, String>,
    pub entries: BTreeMap<String, Entry>,
    /// sessions open in Claude Code on that machine
    #[serde(default)]
    pub running: BTreeMap<String, crate::ccsync::locks::RunLock>,
    /// projects the user deleted on that machine: alias -> when
    #[serde(default)]
    pub dead_projects: BTreeMap<String, i64>,
    /// sessions the user deleted "on all machines" there: session id -> when
    #[serde(default)]
    pub dead_sessions: BTreeMap<String, i64>,
    /// what the Sessions page shows for the sessions on that machine: session id -> details
    #[serde(default)]
    pub sessions: BTreeMap<String, crate::ccsync::sessions::SessionMeta>,
    /// ClaudeSync version and build on that machine
    #[serde(default)]
    pub app: String,
    /// the programs that machine has (Rust, Node, Python...), when it shares them
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<crate::ccsync::tools::Tool>>,
    /// project alias -> its git remote and branch on that machine (to set up git history elsewhere)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub git: BTreeMap<String, crate::ccsync::git::GitInfo>,
    /// that machine's processor family ("AMD64", "ARM64"; "" from older versions)
    #[serde(default)]
    pub arch: String,
}

impl Manifest {
    /// identifies the content (not the timestamp), to skip republishing an unchanged manifest
    pub fn content_id(&self) -> String {
        let mut h = blake3::Hasher::new();
        for (a, p) in &self.aliases {
            h.update(a.as_bytes());
            h.update(p.as_bytes());
        }
        for (k, e) in &self.entries {
            h.update(k.as_bytes());
            h.update(e.hash.as_bytes());
            h.update(&e.mtime.to_le_bytes());
        }
        for (s, r) in &self.running {
            h.update(s.as_bytes());
            h.update(&r.started.to_le_bytes());
        }
        for (a, t) in &self.dead_projects {
            h.update(a.as_bytes());
            h.update(&t.to_le_bytes());
        }
        for (s, t) in &self.dead_sessions {
            h.update(s.as_bytes());
            h.update(&t.to_le_bytes());
        }
        h.update(self.machine_name.as_bytes());
        h.update(self.app.as_bytes());
        h.update(self.arch.as_bytes());
        for (a, g) in &self.git {
            h.update(a.as_bytes());
            h.update(g.url.as_bytes());
            h.update(g.branch.as_bytes());
        }
        for t in self.tools.iter().flatten() {
            h.update(t.key().as_bytes());
            h.update(t.version.as_bytes());
        }
        h.finalize().to_hex()[..24].to_string()
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn to_ms(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn from_ms(ms: i64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms.max(0) as u64)
}
