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
    /// a project's own folder: `code/@alias/<path in the project>` (never path-rewritten)
    Code,
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
    // a project's files (Cargo.lock and the like included)
    if let Some((_, rest)) = code_parts(rel) {
        return (!rest.is_empty() && !name.ends_with(".ccsync-tmp")).then_some(Class::Code);
    }
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
        ("settings.json" | "claude.md" | "keybindings.json" | MCP_FILE, None) => Some(Class::Config),
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

/// Claude Code's MCP servers (kept in `.claude.json`, next to much that is per-PC), as a file in the
/// Claude folder that syncs like the settings (see `Engine::sync_mcp`)
pub const MCP_FILE: &str = "deck-mcp.json";

/// the project alias of a canonical path (`projects/@alias/...` or `code/@alias/...`)
pub fn alias_of(canon_rel: &str) -> Option<&str> {
    canon_rel.strip_prefix("projects/@").or_else(|| canon_rel.strip_prefix("code/@"))?.split_once('/').map(|(a, _)| a)
}

/// (alias, path inside the project) of a project file's canonical path `code/@alias/...`
pub fn code_parts(canon_rel: &str) -> Option<(&str, &str)> {
    canon_rel.strip_prefix("code/@")?.split_once('/')
}

/// false for files of a project the user turned syncing off for (or only its sessions)
pub fn proj_synced(cfg: &Config, canon_rel: &str) -> bool {
    let Some(p) = alias_of(canon_rel).and_then(|a| cfg.projects.get(a)) else { return true };
    p.sync && (p.sessions || classify(canon_rel) != Some(Class::Session))
}

/// Folders never copied with a project's code: version control and editor state belong to each PC.
const NEVER_DIRS: &[&str] = &[".git", ".svn", ".hg", ".vs"];
/// Build output and installed packages: copied only with "build files" on.
pub const BUILD_DIRS: &[&str] = &["target", "node_modules", ".venv", "venv", "__pycache__", "dist", "build", ".next", ".nuxt", ".gradle", "obj", ".pytest_cache", ".mypy_cache", ".turbo", ".parcel-cache"];
/// Larger files aren't copied.
pub const CODE_FILE_MAX: u64 = 512 << 20;

/// A project bigger than this (files, bytes) isn't copied at all, so a big folder that happens to
/// be a project (a backup, a whole drive) never floods the other PCs. Build files raise it.
pub fn code_limits(build: bool) -> (usize, u64) {
    if build {
        (300_000, 40 << 30)
    } else {
        (40_000, 3 << 30)
    }
}

/// Whether a project folder can have its code copied: not a drive, the home folder or a folder
/// above it, nor anything in or above the Claude folder.
pub fn code_root_ok(path: &str, home: &str, claude: &Path) -> bool {
    let p = norm_path(path).to_ascii_lowercase();
    let h = norm_path(home).to_ascii_lowercase();
    let c = norm_path(&claude.display().to_string()).to_ascii_lowercase();
    let under = |a: &str, b: &str| a.starts_with(&format!("{b}\\"));
    p.len() > 3 && p != h && !under(&h, &p) && p != c && !under(&p, &c) && !under(&c, &p)
}

/// Whether the project file `canon_rel` is synced with these settings (true for anything that isn't
/// a project file): project code on, the project and its code switched on, not .git, and build
/// output only with build files on.
pub fn code_wanted(cfg: &Config, canon_rel: &str) -> bool {
    let Some((a, rest)) = code_parts(canon_rel) else { return true };
    let Some(p) = cfg.projects.get(a) else { return false };
    // a folder only guessed from another PC gets files once the user keeps or moves it (Projects)
    if !cfg.sync_code || a == HOME_ALIAS || !p.sync || !p.code || p.auto {
        return false;
    }
    let low = rest.to_ascii_lowercase();
    let name = low.rsplit('/').next().unwrap_or("");
    // per-PC files: Claude Code's local project settings, Explorer's
    if low == ".claude/settings.local.json" || name == "desktop.ini" || name == "thumbs.db" {
        return false;
    }
    let dirs: Vec<String> = rest.split('/').rev().skip(1).map(|c| c.to_ascii_lowercase()).collect();
    !dirs.iter().any(|d| NEVER_DIRS.contains(&d.as_str())) && (cfg.sync_build || !dirs.iter().any(|d| BUILD_DIRS.contains(&d.as_str())))
}

/// Whether a project file is build output or installed packages (made for one kind of processor).
pub fn is_build_path(canon_rel: &str) -> bool {
    code_parts(canon_rel).is_some_and(|(_, rest)| rest.split('/').rev().skip(1).any(|c| BUILD_DIRS.contains(&c.to_ascii_lowercase().as_str())))
}

/// This PC's processor family as Windows reports it ("AMD64", "ARM64"), also when Deck itself
/// runs emulated: build output made on one doesn't run on the other.
pub fn native_arch() -> &'static str {
    static A: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    A.get_or_init(|| {
        crate::util::run_for(10, "reg.exe", &["query", r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment", "/v", "PROCESSOR_ARCHITECTURE"])
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find_map(|l| l.split_once("REG_SZ").map(|(_, v)| v.trim().to_ascii_uppercase())))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default().to_ascii_uppercase())
    })
}

/// The file on this PC for a canonical project-file path (None: unknown project).
pub fn code_abs(cfg: &Config, canon_rel: &str) -> Option<PathBuf> {
    let (a, rest) = code_parts(canon_rel)?;
    Some(Path::new(&cfg.projects.get(a)?.path).join(rest.replace('/', "\\")))
}

/// What is written to disk for canonical `data`: paths put back for this PC (project files are
/// copied as they are).
pub fn decode<'a>(canon: &Canon, canon_rel: &str, data: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
    if code_parts(canon_rel).is_some() {
        std::borrow::Cow::Borrowed(data)
    } else {
        canon.decode(data)
    }
}

pub fn enabled(cfg: &Config, c: Class) -> bool {
    match c {
        Class::Session => cfg.sync_sessions,
        Class::Memory => cfg.sync_memory,
        Class::Config => cfg.sync_config,
        Class::Code => cfg.sync_code,
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
        for actual in self.actual.values() {
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
        cfg.projects.insert(alias.clone(), ProjectMap::new(path, false, "local"));
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
    } else {
        let e = head.strip_prefix('=')?;
        dirs.actual.get(&e.to_lowercase()).cloned().unwrap_or_else(|| e.to_string())
    };
    Some(format!("projects/{dir}/{rest}"))
}

/// What gets hashed and sent: session logs are cut at the last newline so a half-written line never leaves.
pub fn canonical_bytes(canon: &Canon, rel: &str, mut raw: Vec<u8>) -> Vec<u8> {
    if code_parts(rel).is_some() {
        return raw;
    }
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
    /// changed since the last save (with project files it can be large: not rewritten every cycle)
    #[serde(skip)]
    dirty: bool,
    /// when it was last written: a cache (losing changes only means hashing those files again),
    /// so it's written at most every 10 minutes, not each time a session log grows
    #[serde(skip)]
    saved_at: Option<std::time::Instant>,
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
        self.dirty = true;
    }

    pub fn save(&mut self, p: &Path) {
        if p.exists() && (!self.dirty || self.saved_at.is_some_and(|t| t.elapsed().as_secs() < 600)) {
            return;
        }
        if crate::util::write_atomic(p, &serde_json::to_vec(self).unwrap_or_default()).is_ok() {
            self.dirty = false;
            self.saved_at = Some(std::time::Instant::now());
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
    /// projects whose code isn't sent or received from this PC: alias -> why (too big...)
    pub code_skipped: BTreeMap<String, String>,
    /// projects whose files missing now aren't deletions: skipped, or the folder isn't here
    pub code_no_tombs: HashSet<String>,
}

/// Projects found too big to copy: (alias, folder, build files) -> (when, why). Looked at again every
/// 10 minutes, so a big folder isn't walked every cycle.
static TOO_BIG: std::sync::Mutex<Option<HashMap<String, (std::time::Instant, String)>>> = std::sync::Mutex::new(None);

fn too_big_key(alias: &str, path: &str, build: bool) -> String {
    format!("{alias}|{}|{build}", path.to_ascii_lowercase())
}

fn too_big_lately(alias: &str, path: &str, build: bool) -> Option<String> {
    let g = TOO_BIG.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()?.get(&too_big_key(alias, path, build)).filter(|(t, _)| t.elapsed().as_secs() < 600).map(|(_, e)| e.clone())
}

fn too_big_now(alias: &str, path: &str, build: bool, why: &str) {
    let mut g = TOO_BIG.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new).insert(too_big_key(alias, path, build), (std::time::Instant::now(), why.to_string()));
}

/// A project's files (file, canonical path, details), leaving out what `code_wanted` would; Err =
/// too big to copy.
fn walk_code(root: &Path, alias: &str, cfg: &Config, out: &mut Vec<(PathBuf, String, std::fs::Metadata)>) -> Result<(), String> {
    let (max_files, max_bytes) = code_limits(cfg.sync_build);
    let (mut n, mut bytes) = (0usize, 0u64);
    let mut stack = vec![(root.to_path_buf(), String::new())];
    let start = out.len();
    while let Some((dir, rel)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let canon = format!("code/@{alias}/{r}");
            if ft.is_dir() {
                // a folder is left out when a file in it would be
                if code_wanted(cfg, &format!("{canon}/x")) {
                    stack.push((e.path(), r));
                }
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.len() > CODE_FILE_MAX || !code_wanted(cfg, &canon) {
                continue;
            }
            n += 1;
            bytes += m.len();
            if n > max_files || bytes > max_bytes {
                out.truncate(start);
                return Err(format!("too big to copy (over {max_files} files or {})", crate::util::fmt_bytes(max_bytes)));
            }
            out.push((e.path(), canon, m));
        }
    }
    Ok(())
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
    scan_with_home(claude, cfg, canon, dirs, cache, "")
}

/// `scan`, plus each project's own files when project code is on (`home` keeps the home folder,
/// and anything above it, out)
pub fn scan_with_home(claude: &Path, cfg: &Config, canon: &Canon, dirs: &ProjDirs, cache: &mut HashCache, home: &str) -> Scan {
    if cache.fp != canon.fp {
        cache.fp = canon.fp.clone();
        cache.ents.clear();
        cache.dirty = true;
    }
    let mut raw = Vec::new();
    walk(claude, "", &mut raw);
    let mut s = Scan { files: BTreeMap::new(), busy: HashSet::new(), by_local: HashMap::new(), code_skipped: BTreeMap::new(), code_no_tombs: HashSet::new() };
    if cfg.sync_code && !home.is_empty() {
        for (alias, p) in &cfg.projects {
            if alias == HOME_ALIAS || !p.sync || !p.code {
                continue;
            }
            let root = Path::new(&p.path);
            if !code_root_ok(&p.path, home, claude) {
                s.code_skipped.insert(alias.clone(), "this folder can't be copied (a drive, your user folder or a folder above it)".into());
                s.code_no_tombs.insert(alias.clone());
            } else if !root.is_dir() {
                s.code_no_tombs.insert(alias.clone());
            } else if let Some(e) = too_big_lately(alias, &p.path, cfg.sync_build) {
                s.code_skipped.insert(alias.clone(), e);
                s.code_no_tombs.insert(alias.clone());
            } else if let Err(e) = walk_code(root, alias, cfg, &mut raw) {
                too_big_now(alias, &p.path, cfg.sync_build, &e);
                s.code_skipped.insert(alias.clone(), e);
                s.code_no_tombs.insert(alias.clone());
            }
        }
    }
    let mut live: HashSet<String> = HashSet::new();
    for (abs, local_rel, meta) in raw {
        let Some(class) = classify(&local_rel) else { continue };
        if !enabled(cfg, class) {
            continue;
        }
        let canon_rel = if class == Class::Code { local_rel.clone() } else { to_canon_rel(&local_rel, dirs) };
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
                    cache.dirty = true;
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
    let before = cache.ents.len();
    cache.ents.retain(|k, _| live.contains(k));
    cache.dirty |= cache.ents.len() != before;
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
        assert_eq!(classify("code/@deck/Cargo.lock"), Some(Class::Code));
        assert_eq!(classify("code/@deck/src/main.rs"), Some(Class::Code));
        assert_eq!(classify("code/deck/x"), None);
        assert_eq!(classify("deck-mcp.json"), Some(Class::Config));
    }

    #[test]
    fn code_choices() {
        let mut cfg = Config::default();
        cfg.projects.insert("deck".into(), ProjectMap::new(r"C:\dev\deck".into(), false, "local"));
        assert!(!code_wanted(&cfg, "code/@deck/src/main.rs"), "off by default");
        cfg.sync_code = true;
        assert!(code_wanted(&cfg, "code/@deck/src/main.rs"));
        assert!(code_wanted(&cfg, "code/@deck/Cargo.lock"));
        assert!(!code_wanted(&cfg, "code/@deck/.git/config"));
        assert!(!code_wanted(&cfg, "code/@deck/target/release/deck.exe"));
        assert!(!code_wanted(&cfg, "code/@deck/ui/node_modules/x/index.js"));
        assert!(code_wanted(&cfg, "code/@deck/target"), "a file named like a build folder");
        assert!(!code_wanted(&cfg, "code/@other/a.rs"), "unknown project");
        assert!(code_wanted(&cfg, "projects/@deck/a.jsonl"), "not a project file");
        cfg.sync_build = true;
        assert!(code_wanted(&cfg, "code/@deck/target/release/deck.exe"));
        assert!(!code_wanted(&cfg, "code/@deck/.git/config"), "never .git");
        assert!(!code_wanted(&cfg, "code/@deck/.claude/settings.local.json"), "per-PC");
        assert!(is_build_path("code/@deck/ui/node_modules/a/b.node"));
        assert!(!is_build_path("code/@deck/src/target.rs"));
        assert!(!is_build_path("projects/@deck/target/x"));
        assert!(code_wanted(&cfg, "code/@deck/.claude/settings.json"));
        cfg.projects.get_mut("deck").unwrap().auto = true;
        assert!(!code_wanted(&cfg, "code/@deck/src/main.rs"), "only guessed: wait for the user");
        cfg.projects.get_mut("deck").unwrap().auto = false;
        cfg.projects.get_mut("deck").unwrap().code = false;
        assert!(!code_wanted(&cfg, "code/@deck/src/main.rs"));
        let claude = Path::new(r"C:\Users\me\.claude");
        assert!(!code_root_ok(r"C:\", r"C:\Users\me", claude));
        assert!(!code_root_ok(r"C:\Users", r"C:\Users\me", claude));
        assert!(!code_root_ok(r"C:\Users\me", r"C:\Users\me", claude));
        assert!(!code_root_ok(r"C:\Users\me\.claude\x", r"C:\Users\me", claude));
        assert!(code_root_ok(r"C:\Users\me\deck", r"C:\Users\me", claude));
    }

    #[test]
    fn sessions_off_for_one_project() {
        let mut cfg = Config::default();
        let mut p = ProjectMap::new(r"C:\dev\deck".into(), false, "local");
        p.sessions = false;
        cfg.projects.insert("deck".into(), p);
        assert!(!proj_synced(&cfg, "projects/@deck/0123abcd-0000-4000-8000-000000000001.jsonl"));
        assert!(proj_synced(&cfg, "projects/@deck/memory/MEMORY.md"));
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
