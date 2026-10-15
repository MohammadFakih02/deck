//! Finds the synced files under the Claude folder and computes their canonical hashes (cached by
//! size + mtime, so a cycle with no changes only stats files).
use crate::ccsync::canon::{encode_dir, norm_path, Canon};
use crate::ccsync::config::{Config, ProjectMap, HOME_ALIAS};
use crate::ccsync::crypto::hash;
use crate::ccsync::model::{to_ms, Entry, CHUNK};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Session,
    Memory,
    Config,
}

impl Class {
    pub fn idx(self) -> usize {
        self as usize
    }
}

const TOP_DIRS: &[&str] = &["projects", "file-history", "todos", "plans", "agents", "commands", "output-styles", "hooks", "rules", "skills", "plugins"];

/// Which category a path (local or canonical, '/'-separated, relative to the Claude folder) belongs to;
/// None = never synced (credentials, caches, Claude's own account-synced folders, ...).
pub fn classify(rel: &str) -> Option<Class> {
    let low = rel.to_ascii_lowercase();
    let name = low.rsplit('/').next().unwrap_or("");
    if name.ends_with(".ccsync-tmp") || name == "desktop.ini" || name == "thumbs.db" || name.ends_with(".lock") {
        return None;
    }
    let (top, rest) = match low.split_once('/') {
        Some((t, r)) => (t, Some(r)),
        None => (low.as_str(), None),
    };
    match (top, rest) {
        ("projects", Some(r)) => {
            let mut p = r.splitn(3, '/');
            let _enc = p.next();
            let second = p.next()?;
            if second == "memory" && p.next().is_some() {
                Some(Class::Memory)
            } else {
                Some(Class::Session)
            }
        }
        ("file-history" | "todos" | "plans", Some(_)) => Some(Class::Session),
        ("settings.json" | "claude.md" | "keybindings.json", None) => Some(Class::Config),
        ("agents" | "commands" | "output-styles" | "hooks" | "rules", Some(_)) => Some(Class::Config),
        ("skills" | "plugins", Some(r)) => {
            if r.starts_with("synced/") || r.starts_with(".bucket") || r.starts_with("plugin-directory-cache") {
                None
            } else {
                Some(Class::Config)
            }
        }
        _ => None,
    }
}

/// the project alias of a canonical path (`projects/@alias/...`)
pub fn alias_of(canon_rel: &str) -> Option<&str> {
    canon_rel.strip_prefix("projects/@")?.split_once('/').map(|(a, _)| a)
}

/// false for files of a project the user turned syncing off for
pub fn proj_synced(cfg: &Config, canon_rel: &str) -> bool {
    alias_of(canon_rel).map(|a| cfg.projects.get(a).map(|p| p.sync).unwrap_or(true)).unwrap_or(true)
}

pub fn enabled(cfg: &Config, c: Class) -> bool {
    match c {
        Class::Session => cfg.sync_sessions,
        Class::Memory => cfg.sync_memory,
        Class::Config => cfg.sync_config,
    }
}

/// a relative path that can't escape the Claude folder or hit Windows-reserved names
pub fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() < 400
        && rel.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && !c.ends_with('.')
                && !c.ends_with(' ')
                && !c.chars().any(|ch| matches!(ch, ':' | '\\' | '<' | '>' | '"' | '|' | '?' | '*') || (ch as u32) < 32)
        })
}

/// does a Claude project folder name belong to this path? (Claude shortens names over 200 chars + hash)
pub fn enc_matches(path: &str, dir: &str) -> bool {
    let enc = encode_dir(&norm_path(path));
    if enc.eq_ignore_ascii_case(dir) {
        return true;
    }
    enc.len() > 200 && dir.len() > 201 && dir.is_char_boundary(200) && enc.is_char_boundary(200) && enc[..200].eq_ignore_ascii_case(&dir[..200]) && dir.as_bytes()[200] == b'-'
}

#[derive(Default, Clone)]
pub struct ProjDirs {
    /// lowercase folder name -> alias
    pub enc_alias: HashMap<String, String>,
    /// lowercase folder name -> folder name as on disk
    pub actual: HashMap<String, String>,
}

impl ProjDirs {
    pub fn dir_for_path(&self, path: &str) -> String {
        for (_, actual) in &self.actual {
            if enc_matches(path, actual) {
                return actual.clone();
            }
        }
        encode_dir(&norm_path(path))
    }
}

fn find_cwd(dir: &Path, name: &str) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false)).collect();
    files.sort();
    for f in files.iter().take(30) {
        let mut buf = Vec::new();
        if std::fs::File::open(f).and_then(|h| h.take(512 * 1024).read_to_end(&mut buf)).is_err() {
            continue;
        }
        for line in buf.split(|&b| b == b'\n') {
            if memchr::memmem::find(line, b"\"cwd\"").is_none() {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else { continue };
            if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                if enc_matches(cwd, name) {
                    return Some(norm_path(cwd));
                }
            }
        }
    }
    None
}

fn sanitize_alias(s: &str) -> String {
    let a: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    let a = a.trim_matches(|c| c == '_' || c == '.').to_string();
    if a.is_empty() {
        "project".into()
    } else {
        a.chars().take(48).collect()
    }
}

/// Maps every local project folder to an alias, creating aliases for new projects.
/// Returns true if the config changed.
pub fn resolve_projects(claude: &Path, cfg: &mut Config, home: &str) -> (ProjDirs, bool) {
    let mut d = ProjDirs::default();
    let mut changed = false;
    let Ok(rd) = std::fs::read_dir(claude.join("projects")) else { return (d, false) };
    let mut names: Vec<String> = rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in &names {
        d.actual.insert(name.to_lowercase(), name.clone());
    }
    for name in names {
        let low = name.to_lowercase();
        let found = cfg.projects.iter().find(|(_, p)| enc_matches(&p.path, &name)).map(|(a, _)| a.clone());
        if let Some(a) = found {
            d.enc_alias.insert(low, a);
            continue;
        }
        let Some(path) = find_cwd(&claude.join("projects").join(&name), &name) else { continue };
        let base = if path.eq_ignore_ascii_case(home) { HOME_ALIAS.to_string() } else { sanitize_alias(path.rsplit('\\').next().unwrap_or("")) };
        let mut alias = base.clone();
        let mut n = 2;
        loop {
            match cfg.projects.get(&alias) {
                None => break,
                // a guess from another machine whose folder doesn't exist here: this local project takes it over
                Some(p) if p.auto && !d.actual.keys().any(|k| enc_matches(&p.path, k)) => break,
                Some(_) => {
                    alias = format!("{base}-{n}");
                    n += 1;
                }
            }
        }
        cfg.projects.insert(alias.clone(), ProjectMap { path, auto: false, origin: "local".into(), sync: true });
        d.enc_alias.insert(low, alias);
        changed = true;
    }
    (d, changed)
}

pub fn to_canon_rel(local_rel: &str, dirs: &ProjDirs) -> String {
    if let Some(r) = local_rel.strip_prefix("projects/") {
        if let Some((enc, rest)) = r.split_once('/') {
            return match dirs.enc_alias.get(&enc.to_lowercase()) {
                Some(a) => format!("projects/@{a}/{rest}"),
                None => format!("projects/={enc}/{rest}"),
            };
        }
    }
    local_rel.to_string()
}

pub fn to_local_rel(canon_rel: &str, cfg: &Config, dirs: &ProjDirs) -> Option<String> {
    let Some(r) = canon_rel.strip_prefix("projects/") else { return Some(canon_rel.to_string()) };
    let (head, rest) = r.split_once('/')?;
    let dir = if let Some(a) = head.strip_prefix('@') {
        dirs.dir_for_path(&cfg.projects.get(a)?.path)
    } else if let Some(e) = head.strip_prefix('=') {
        dirs.actual.get(&e.to_lowercase()).cloned().unwrap_or_else(|| e.to_string())
    } else {
        return None;
    };
    Some(format!("projects/{dir}/{rest}"))
}

/// What gets hashed and sent: session logs are cut at the last newline so a half-written line never leaves.
pub fn canonical_bytes(canon: &Canon, rel: &str, mut raw: Vec<u8>) -> Vec<u8> {
    if rel.ends_with(".jsonl") {
        match memchr::memrchr(b'\n', &raw) {
            Some(p) => raw.truncate(p + 1),
            None => raw.clear(),
        }
    }
    match canon.encode(&raw) {
        std::borrow::Cow::Borrowed(_) => raw,
        std::borrow::Cow::Owned(o) => o,
    }
}

pub fn chunk_hashes(d: &[u8]) -> Vec<String> {
    d.chunks(CHUNK).map(hash).collect()
}

#[derive(Clone, Debug)]
pub struct LocalFile {
    pub abs: PathBuf,
    pub local_rel: String,
    pub class: Class,
    pub e: Entry,
}

#[derive(Serialize, Deserialize, Default)]
pub struct HashCache {
    fp: String,
    ents: HashMap<String, CacheEnt>,
}

#[derive(Serialize, Deserialize, Clone)]
struct CacheEnt {
    size: u64,
    mtime: i64,
    hash: String,
    chunks: Vec<String>,
}

impl HashCache {
    pub fn load(p: &Path) -> HashCache {
        std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    /// records the canonical hash of a file we just wrote from another machine's data, so the next scan
    /// doesn't recompute it (re-encoding a text that mentions the other machine's literal paths would
    /// give a different hash and look like a local edit)
    pub fn put(&mut self, local_rel: &str, size: u64, mtime: i64, hash: String, chunks: Vec<String>) {
        self.ents.insert(local_rel.to_string(), CacheEnt { size, mtime, hash, chunks });
    }

    pub fn save(&self, p: &Path) {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

pub struct Scan {
    /// canonical rel -> file
    pub files: BTreeMap<String, LocalFile>,
    /// canonical rels that exist but couldn't be read this time (not deletions)
    pub busy: HashSet<String>,
    /// lowercase local rel -> canonical rel
    pub by_local: HashMap<String, String>,
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(PathBuf, String, std::fs::Metadata)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            if rel.is_empty() && !TOP_DIRS.contains(&name.to_ascii_lowercase().as_str()) {
                continue;
            }
            let rl = r.to_ascii_lowercase();
            if rl == "skills/synced" || rl == "plugins/synced" {
                continue;
            }
            walk(&e.path(), &r, out);
        } else if let Ok(m) = e.metadata() {
            out.push((e.path(), r, m));
        }
    }
}

pub fn scan(claude: &Path, cfg: &Config, canon: &Canon, dirs: &ProjDirs, cache: &mut HashCache) -> Scan {
    if cache.fp != canon.fp {
        cache.fp = canon.fp.clone();
        cache.ents.clear();
    }
    let mut raw = Vec::new();
    walk(claude, "", &mut raw);
    let mut s = Scan { files: BTreeMap::new(), busy: HashSet::new(), by_local: HashMap::new() };
    let mut live: HashSet<String> = HashSet::new();
    for (abs, local_rel, meta) in raw {
        let Some(class) = classify(&local_rel) else { continue };
        if !enabled(cfg, class) {
            continue;
        }
        let canon_rel = to_canon_rel(&local_rel, dirs);
        if !safe_rel(&canon_rel) || !proj_synced(cfg, &canon_rel) {
            continue;
        }
        let size = meta.len();
        let mtime = meta.modified().map(to_ms).unwrap_or(0);
        live.insert(local_rel.clone());
        let ce = match cache.ents.get(&local_rel) {
            Some(c) if c.size == size && c.mtime == mtime => c.clone(),
            _ => match std::fs::read(&abs) {
                Ok(bytes) => {
                    let data = canonical_bytes(canon, &local_rel, bytes);
                    let c = CacheEnt { size, mtime, hash: hash(&data), chunks: chunk_hashes(&data) };
                    cache.ents.insert(local_rel.clone(), c.clone());
                    c
                }
                Err(_) => {
                    s.busy.insert(canon_rel);
                    continue;
                }
            },
        };
        s.by_local.insert(local_rel.to_lowercase(), canon_rel.clone());
        s.files.insert(canon_rel, LocalFile { abs, local_rel, class, e: Entry { hash: ce.hash, size, mtime, chunks: ce.chunks, deleted: false } });
    }
    cache.ents.retain(|k, _| live.contains(k));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(classify("projects/c--X/abc.jsonl"), Some(Class::Session));
        assert_eq!(classify("projects/c--X/abc/subagents/a.jsonl"), Some(Class::Session));
        assert_eq!(classify("projects/c--X/memory/MEMORY.md"), Some(Class::Memory));
        assert_eq!(classify("projects/@GAME_APP/memory/x.md"), Some(Class::Memory));
        assert_eq!(classify(".credentials.json"), None);
        assert_eq!(classify("settings.json"), Some(Class::Config));
        assert_eq!(classify("settings.local.json"), None);
        assert_eq!(classify("skills/synced/x/SKILL.md"), None);
        assert_eq!(classify("skills/gepeto/SKILL.md"), Some(Class::Config));
        assert_eq!(classify("plugins/plugin-directory-cache-v2.json"), None);
        assert_eq!(classify("sessions/123.json"), None);
        assert_eq!(classify("projects/stray.txt"), None);
    }

    #[test]
    fn safe() {
        assert!(safe_rel("projects/@~/a.jsonl"));
        assert!(!safe_rel("projects/../../x"));
        assert!(!safe_rel("C:/x"));
        assert!(!safe_rel("/x"));
        assert!(!safe_rel("a//b"));
    }
}
