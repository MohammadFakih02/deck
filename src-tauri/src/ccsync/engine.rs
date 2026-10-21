//! The sync loop. Each cycle:
//!  1. map local project folders to aliases, scan + hash files (canonical form)
//!  2. note deletions, build our manifest, serve it on the LAN and publish it to the cloud folder
//!  3. fetch the other machines' manifests (LAN and/or cloud folder)
//!  4. three-way merge per file against the last version both sides agreed on ("base"):
//!     only they changed -> take theirs; only we changed -> keep (they pull ours);
//!     both changed -> session log: if one extends the other take the longer one, otherwise keep both
//!     (the losing branch becomes a new session with its own id, chosen the same way on both machines);
//!     other files: newest wins, the loser is saved under the data dir's conflicts folder.
//!
//! Session logs are never deleted by sync (Claude's own cleanup on one machine stays local); memory and
//! config deletions are passed on.
use crate::util::LockAnyway;
use crate::ccsync::canon::{norm_path, Canon};
use crate::ccsync::config::{Cmd, Config, MachineStatus, ProjectMap, Shared, HOME_ALIAS};
use crate::ccsync::crypto::{hash, Key};
use crate::ccsync::hub::Hub;
use crate::ccsync::lan::{self, Client, Lan, ServeSnap};
use crate::ccsync::locks::{self, LocksFile, RemoteLock, RunLock};
use crate::ccsync::model::{from_ms, now_ms, to_ms, Entry, Manifest, CHUNK};
use crate::ccsync::sessions::{is_sid, SessionMeta};
use crate::ccsync::scan::{self, canonical_bytes, classify, code_parts, code_wanted, enabled, resolve_projects, safe_rel, to_local_rel, Class, HashCache, LocalFile, ProjDirs, Scan};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::ccsync::mcp;

const DAY: i64 = 86_400_000;

#[derive(Serialize, Deserialize, Default)]
struct State {
    /// machine id -> canonical path -> last hash both sides had ("-" = both deleted)
    base: BTreeMap<String, BTreeMap<String, String>>,
    /// memory/config files present at the last scan, to notice deletions
    known: BTreeSet<String>,
    /// deletions we publish: path -> when
    tombs: BTreeMap<String, i64>,
    /// whether our hook is in settings.json as far as we last set it (only changed on a toggle, so two
    /// machines with different choices don't keep flipping the synced settings.json)
    #[serde(default)]
    hook_applied: Option<bool>,
    /// same for the SessionStart "which machine am I on" hook
    #[serde(default)]
    ident_applied: Option<bool>,
    /// session id -> other machines whose messages are part of that session's history here
    #[serde(default)]
    visited: BTreeMap<String, BTreeSet<String>>,
    /// projects deleted here by the user: alias -> when (published so other machines delete them too)
    #[serde(default)]
    my_dead: BTreeMap<String, i64>,
    /// projects deleted here because another machine's deletion arrived: alias -> that deletion's time
    #[serde(default)]
    applied_dead: BTreeMap<String, i64>,
    /// sessions deleted on this PC (by the user here, or because another machine deleted them): session
    /// id -> when. They are never received again.
    #[serde(default)]
    hidden: BTreeMap<String, i64>,
    /// sessions the user deleted "on all machines" here: published so the other machines delete them too
    #[serde(default)]
    my_dead_sess: BTreeMap<String, i64>,
    /// hash of the MCP server list as last exported to / imported from `deck-mcp.json`
    #[serde(default)]
    mcp_hash: String,
}

enum Via {
    Lan(Client),
    Hub,
}

struct Remote {
    m: Manifest,
    via: Via,
}

pub struct Engine {
    sh: Arc<Shared>,
    lan: Arc<Lan>,
    state: State,
    cache: HashCache,
    last_hub_pub: String,
    last_local: String,
    last_gc: i64,
    last_hub_time: i64,
    /// sessions open in Claude Code here (this cycle)
    running: BTreeMap<String, RunLock>,
    /// sessions open here with newer messages waiting elsewhere (this cycle)
    pending: BTreeMap<String, String>,
    /// (session id, machine) for session files written from another machine during this merge
    seen: Mutex<Vec<(String, String)>>,
    /// (local rel path, size, mtime, hash, chunk hashes) of files written during this merge
    cache_puts: Mutex<Vec<(String, u64, i64, String, Vec<String>)>>,
    /// canonical path -> (size, mtime, details) so unchanged session logs aren't read again
    meta_cache: HashMap<String, (u64, i64, SessionMeta)>,
    /// machine id -> (name, its sessions) as last seen, so an offline machine's sessions stay listed
    peer_sessions: HashMap<String, (String, BTreeMap<String, SessionMeta>)>,
    /// hash of state.json as last written
    saved_state: Mutex<String>,
}

#[derive(Default)]
struct Tally {
    received: usize,
    conflicts: usize,
    deleted: usize,
    wrote: bool,
    errors: Vec<String>,
}

/// Writes a file in the Claude folder. Deck has admin rights and the folder is the user's, so this
/// goes only where the signed-in user could write themselves (their own links, like skills kept
/// elsewhere, still work; one into Windows' folders doesn't) - see crate::fsguard.
fn write_file(abs: &Path, data: &[u8], mtime: i64) -> std::io::Result<()> {
    use std::io::Write;
    let (Some(dir), Some(name)) = (abs.parent(), abs.file_name()) else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, abs.display().to_string()));
    };
    let name = name.to_string_lossy().to_string();
    let d = crate::fsguard::open_for_user(dir)?;
    d.check_replace(&name)?;
    let tmp = format!("{name}.ccsync-tmp");
    let mut f = d.create(&tmp, true)?;
    if let Err(e) = f.write_all(data) {
        drop(f);
        let _ = d.remove(&tmp);
        return Err(e);
    }
    if mtime > 0 {
        let _ = f.set_modified(from_ms(mtime));
    }
    drop(f);
    d.rename(&tmp, &name, true).inspect_err(|_| {
        let _ = d.remove(&tmp);
    })
}

/// Windows' "not the same drive" error: a move becomes a copy.
const NOT_SAME_DEVICE: i32 = 17;

/// Moves `from` to `to` for the user (see crate::fsguard::move_for_user); across drives, a copy.
fn move_item(from: &Path, to: &Path) -> std::io::Result<()> {
    match crate::fsguard::move_for_user(from, to) {
        Err(e) if e.raw_os_error() == Some(NOT_SAME_DEVICE) => {
            if from.is_dir() {
                copy_dir(from, to)?;
                std::fs::remove_dir_all(from)
            } else {
                std::fs::copy(from, to)?;
                std::fs::remove_file(from)
            }
        }
        r => r,
    }
}

/// moves a file into `dir`, keeping its relative path
fn stash(abs: &Path, dir: &Path, rel: &str) -> std::io::Result<()> {
    move_item(abs, &dir.join(rel.replace('/', "\\")))
}

fn stamp() -> String {
    let s = now_ms() / 1000;
    let days = s / 86400;
    // civil date from days since epoch (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}_{}", crate::ccsync::config::clock(s).replace(':', ""))
}

/// session uuid of `projects/<x>/<uuid>.jsonl`
fn session_id(p: &str) -> Option<&str> {
    let name = p.strip_prefix("projects/")?.split_once('/')?.1;
    let id = name.strip_suffix(".jsonl")?;
    (!id.contains('/') && is_sid(id)).then_some(id)
}

/// the same new id on every machine for the same losing branch
fn fork_id(p: &str, loser_hash: &str) -> String {
    let h = blake3::hash(format!("{p}\n{loser_hash}").as_bytes());
    let b = h.as_bytes();
    let x = crate::ccsync::crypto::hex(&b[..16]);
    format!("{}-{}-4{}-{}{}-{}", &x[..8], &x[8..12], &x[13..16], ["8", "9", "a", "b"][(b[8] & 3) as usize], &x[17..20], &x[20..32])
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// A project path from another PC: one under that PC's home folder goes under this PC's
/// (`C:\Users\Me\dev\x` there is `C:\Users\Laptop\dev\x` here); anything else stays as it is.
fn rebase_home(path: &str, their_home: Option<&str>, home: &str) -> String {
    let Some(h) = their_home.map(norm_path).filter(|h| h.len() > 3) else { return path.to_string() };
    match path.get(..h.len()) {
        Some(head) if head.eq_ignore_ascii_case(&h) && (path.len() == h.len() || path[h.len()..].starts_with('\\')) => format!("{}{}", norm_path(home), &path[h.len()..]),
        _ => path.to_string(),
    }
}

fn replace_all(d: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(d.len());
    let mut last = 0;
    for i in memchr::memmem::find_iter(d, from) {
        o.extend_from_slice(&d[last..i]);
        o.extend_from_slice(to);
        last = i + from.len();
    }
    o.extend_from_slice(&d[last..]);
    o
}

impl Engine {
    pub fn new(sh: Arc<Shared>, lan: Arc<Lan>) -> Engine {
        let state = std::fs::read(sh.paths.data.join("state.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let cache = HashCache::load(&sh.paths.data.join("hashcache.json"));
        Engine {
            sh,
            lan,
            state,
            cache,
            last_hub_pub: String::new(),
            last_local: String::new(),
            last_gc: 0,
            last_hub_time: 0,
            running: BTreeMap::new(),
            pending: BTreeMap::new(),
            seen: Mutex::new(Vec::new()),
            cache_puts: Mutex::new(Vec::new()),
            meta_cache: HashMap::new(),
            peer_sessions: HashMap::new(),
            saved_state: Mutex::new(String::new()),
        }
    }

    /// Only when something changed: this runs every cycle, and the file grows with every synced
    /// file (rewriting it unchanged every 20 s would be gigabytes of disk writes a day for nothing).
    fn save_state(&self) {
        let body = serde_json::to_vec(&self.state).unwrap_or_default();
        let h = hash(&body);
        if *self.saved_state.lock_anyway() == h {
            return;
        }
        if crate::util::write_atomic(&self.sh.paths.data.join("state.json"), &body).is_ok() {
            *self.saved_state.lock_anyway() = h;
        }
    }

    fn phase(&self, s: &str) {
        self.sh.status(|st| st.phase = s.to_string());
    }

    pub fn run(mut self) {
        let mut force = true;
        loop {
            // switched off in Deck: Claude Code's hooks go (they'd only ever let prompts through
            // anyway), then nothing happens until it is switched on again
            if self.sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                self.switch_off();
                while self.sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                    self.sh.wait(3600);
                }
                force = true;
                continue;
            }
            let cmds: Vec<Cmd> = std::mem::take(&mut *self.sh.cmds.lock_anyway());
            for c in cmds {
                match c {
                    Cmd::SyncNow => force = true,
                    Cmd::DeleteProject { alias } => match self.delete_project(&alias, None) {
                        Ok(m) => self.sh.log(m),
                        Err(e) => self.sh.log(format!("Deleting {alias} failed: {e}")),
                    },
                    Cmd::DeleteSession { sid, everywhere } => match self.delete_session(&sid, None) {
                        Ok(m) => {
                            if everywhere {
                                self.state.my_dead_sess.insert(sid.to_ascii_lowercase(), now_ms());
                                self.save_state();
                            }
                            self.sh.log(if everywhere { format!("{m}; the other machines will delete it too") } else { m });
                        }
                        Err(e) => self.sh.log(format!("Deleting session failed: {e}")),
                    },
                    Cmd::Remap { alias, path } => match self.remap(&alias, &path) {
                        Ok(m) => self.sh.log(m),
                        Err(e) => self.sh.log(format!("Moving {alias} failed: {e}")),
                    },
                }
            }
            let cfg = self.sh.cfg();
            self.write_identity(&cfg);
            if cfg.paused && !force {
                self.phase("Paused");
            } else {
                self.cycle();
            }
            force = false;
            // wait for the interval, a wake-up, or a Claude session opening/closing here (so the other
            // machine learns about locks within seconds)
            let start = std::time::Instant::now();
            let sig = |r: &BTreeMap<String, RunLock>| r.keys().cloned().collect::<Vec<_>>().join(",");
            let before = sig(&self.running);
            while start.elapsed().as_secs() < cfg.interval_secs.clamp(5, 3600) {
                if self.sh.wait(3) {
                    break;
                }
                if !cfg.paused && sig(&locks::running(&self.sh.paths.claude)) != before {
                    break;
                }
            }
        }
    }

    /// Claude Sync off: our hooks out of Claude Code's settings.json, the lock file says "off"
    fn switch_off(&mut self) {
        for event in ["UserPromptSubmit", "SessionStart"] {
            match locks::set_hook(&self.sh.paths.claude, event, None) {
                Ok(true) => self.sh.log(format!("Removed the {event} hook from settings.json")),
                Ok(false) => {}
                Err(e) => self.sh.log(format!("Couldn't remove the {event} hook: {e}")),
            }
        }
        self.state.hook_applied = Some(false);
        self.state.ident_applied = Some(false);
        self.save_state();
        locks::write_file(&self.sh.paths.data, &LocksFile { updated: now_ms(), ..Default::default() });
        locks::write_identity(&self.sh.paths.data, &locks::Identity::default());
        self.phase("Off");
    }

    /// keeps the Claude Code hook and its exe in place
    fn maintain_hook(&mut self, cfg: &Config) {
        let home = self.sh.paths.home.clone();
        let mut note = String::new();
        if let Err(e) = locks::install_exe(&home) {
            note = format!("Couldn't copy the hook program: {e}");
        }
        // never point Claude at a hook program that isn't there (it would show an error on every prompt)
        let exe_ok = locks::hook_exe(&home).is_file();
        let cmd = locks::hook_command(&home, &self.sh.hook_extra.lock_anyway());
        // (event, wanted, already applied, what to call it)
        let hooks = [("UserPromptSubmit", cfg.session_lock, self.state.hook_applied, "session-lock"), ("SessionStart", cfg.machine_note, self.state.ident_applied, "machine-name")];
        for (i, (event, want, applied, label)) in hooks.into_iter().enumerate() {
            // a wanted hook is re-checked every time (an uninstall, or anything else, may have removed it)
            if (!want && applied == Some(false)) || (want && !exe_ok) {
                continue;
            }
            match locks::set_hook(&self.sh.paths.claude, event, want.then_some(cmd.as_str())) {
                Ok(changed) => {
                    if i == 0 {
                        self.state.hook_applied = Some(want);
                    } else {
                        self.state.ident_applied = Some(want);
                    }
                    if changed {
                        self.sh.log(if want { format!("Added the {label} hook to Claude Code's settings.json") } else { format!("Removed the {label} hook from settings.json") });
                    }
                }
                Err(e) => note = format!("The {label} hook isn't installed: {e}"),
            }
        }
        self.sh.status(|s| s.hook_note = note);
    }

    /// what the SessionStart hook tells Claude: which machine this is and which sessions came from elsewhere
    fn write_identity(&self, cfg: &Config) {
        let others: Vec<String> = self.sh.status.lock_anyway().machines.iter().filter(|m| m.id != cfg.machine_id).map(|m| m.name.clone()).collect();
        let visited = self.state.visited.iter().map(|(s, m)| (s.clone(), m.iter().cloned().collect())).collect();
        let id = locks::Identity { enabled: cfg.machine_note, machine: cfg.machine_name.clone(), home: self.sh.paths.home.clone(), others, visited };
        locks::write_identity(&self.sh.paths.data, &id);
    }

    /// what the hook reads; also feeds the Status page
    fn publish_locks(&self, cfg: &Config, remote: &BTreeMap<String, RemoteLock>, scan: &Scan) {
        let now = now_ms();
        let overrides: BTreeMap<String, i64> = {
            let mut o = self.sh.overrides.lock_anyway();
            o.retain(|_, t| *t > now);
            o.clone()
        };
        let f = LocksFile { updated: now, enabled: cfg.session_lock, remote: remote.clone(), pending: self.pending.clone(), overrides };
        locks::write_file(&self.sh.paths.data, &f);
        self.write_identity(cfg);
        let alias_of = |sid: &str| -> String {
            let tail = format!("/{sid}.jsonl");
            scan.files.keys().find(|k| k.ends_with(&tail)).and_then(|k| k.strip_prefix("projects/")).and_then(|k| k.split_once('/')).map(|(a, _)| a.trim_start_matches(['@', '=']).to_string()).unwrap_or_default()
        };
        let here: Vec<_> = self.running.iter().map(|(s, r)| (s.clone(), r.clone(), alias_of(s))).collect();
        let there: Vec<_> = remote.iter().map(|(s, r)| (s.clone(), r.clone(), alias_of(s))).collect();
        let pending = self.pending.clone();
        self.sh.status(|s| {
            s.open_here = here;
            s.open_elsewhere = there;
            s.pending = pending;
        });
    }

    /// one sync pass; also usable headless (`--once`)
    pub fn cycle(&mut self) {
        let claude = self.sh.paths.claude.clone();
        if !claude.join("projects").is_dir() && !claude.join("settings.json").exists() {
            // a fresh machine that never ran Claude Code: create the folders so it can receive
            if let Err(e) = std::fs::create_dir_all(claude.join("projects")) {
                self.phase("Waiting for Claude Code");
                self.sh.status(|s| s.last_error = format!("Can't create {}: {e}", claude.display()));
                return;
            }
            self.sh.log(format!("{} was empty; created it so this PC can receive sessions", claude.display()));
        }
        let mut tally = Tally::default();
        self.maintain_hook(&self.sh.cfg());
        for round in 0..3 {
            self.phase("Scanning");
            self.running = locks::running(&claude);
            self.pending.clear();
            let mut cfg = self.sh.cfg();
            let key = Key::new(&cfg.sync_key);
            let (dirs, changed) = resolve_projects(&claude, &mut cfg, &self.sh.paths.home);
            if changed {
                let projects = cfg.projects.clone();
                for (a, p) in &projects {
                    if !self.sh.cfg().projects.contains_key(a) {
                        self.sh.log(format!("New project {a} = {}", p.path));
                    }
                }
                self.sh.set_cfg(|c| c.projects = projects);
            }
            // a project we deleted but that exists here again (new sessions) is no longer "deleted"
            self.state.my_dead.retain(|a, _| !cfg.projects.contains_key(a));
            let canon = Arc::new(Canon::new(&cfg.aliases()));
            if cfg.sync_config {
                self.sync_mcp();
            }
            let scan = scan::scan_with_home(&claude, &cfg, &canon, &dirs, &mut self.cache, &self.sh.paths.home);
            self.cache.save(&self.sh.paths.data.join("hashcache.json"));
            self.update_tombs(&cfg, &scan);
            let mut man = self.build_manifest(&cfg, &scan);
            man.sessions = self.session_metas(&scan);
            self.publish_lan(&man, &scan, &canon);
            self.local_status(&scan);

            let hub = if cfg.hub_enabled {
                match Hub::new(&cfg.hub_dir, key.clone()) {
                    Ok(h) => Some(h),
                    Err(e) => {
                        tally.errors.push(format!("Cloud folder: {e}"));
                        None
                    }
                }
            } else {
                None
            };
            if let Some(h) = &hub {
                self.phase("Uploading to cloud folder");
                if let Err(e) = self.publish_hub(h, &man, &scan, &canon) {
                    tally.errors.push(format!("Cloud folder: {e}"));
                }
            }

            self.phase("Checking other machines");
            let mut remotes = self.gather(&cfg, &key, hub.as_ref(), &mut tally);
            let local_changed = man.content_id() != self.last_local;
            if local_changed {
                for r in remotes.iter_mut() {
                    if let Via::Lan(c) = &mut r.via {
                        let _ = c.poke();
                    }
                }
                self.last_local = man.content_id();
            }

            // projects another machine's user deleted
            let mut deleted_any = false;
            for r in &remotes {
                for (a, &t) in &r.m.dead_projects.clone() {
                    if a == HOME_ALIAS || a.is_empty() || a.contains([':', '/', '\\']) {
                        continue;
                    }
                    let known = self.state.applied_dead.get(a).copied().unwrap_or(0).max(self.state.my_dead.get(a).copied().unwrap_or(0));
                    if t <= known {
                        continue;
                    }
                    match self.delete_project(a, Some(t)) {
                        Ok(m) => {
                            self.sh.log(format!("{}: {m}", r.m.machine_name));
                            deleted_any = true;
                        }
                        Err(e) => tally.errors.push(format!("Can't delete project {a} as {} did: {e}", r.m.machine_name)),
                    }
                }
            }
            // sessions another machine's user deleted on all machines
            for r in &remotes {
                for (sid, &t) in &r.m.dead_sessions.clone() {
                    if !is_sid(sid) || self.state.hidden.get(sid).copied().unwrap_or(0) >= t {
                        continue;
                    }
                    match self.delete_session(sid, Some(t)) {
                        Ok(m) => {
                            self.sh.log(format!("{}: {m}", r.m.machine_name));
                            deleted_any = true;
                        }
                        Err(e) => tally.errors.push(format!("{} deleted a session that can't be deleted here yet: {e}", r.m.machine_name)),
                    }
                }
            }
            if deleted_any && round < 2 {
                continue;
            }

            // projects we haven't seen: guess the same path until the user maps them
            let mut learned = Vec::new();
            for r in &remotes {
                for (a, p) in &r.m.aliases {
                    if !cfg.projects.contains_key(a) && self.dead_allows(a, &r.m.entries) && !learned.iter().any(|(x, _, _): &(String, String, String)| x == a) && !a.contains([':', '/', '\\']) && !a.is_empty() {
                        learned.push((a.clone(), rebase_home(&norm_path(p), r.m.aliases.get(HOME_ALIAS).map(String::as_str), &self.sh.paths.home), r.m.machine_name.clone()));
                    }
                }
            }
            if !learned.is_empty() && round < 2 {
                self.sh.set_cfg(|c| {
                    for (a, p, from) in &learned {
                        c.projects.entry(a.clone()).or_insert(ProjectMap::new(p.clone(), true, from));
                    }
                });
                for (a, p, from) in &learned {
                    self.sh.log(format!("Project {a} from {from}: using {p} here (change it under Projects if it lives elsewhere)"));
                }
                continue;
            }

            // sessions open on other machines (cloud-folder manifests only count while they're fresh)
            let mut remote_locks: BTreeMap<String, RemoteLock> = BTreeMap::new();
            for r in &remotes {
                let fresh = matches!(r.via, Via::Lan(_)) || now_ms() - r.m.generated_at < locks::CLOUD_LOCK_TTL;
                if !fresh {
                    continue;
                }
                for (sid, rl) in &r.m.running {
                    let l = RemoteLock { machine: r.m.machine_name.clone(), started: rl.started, name: rl.name.clone() };
                    match remote_locks.get(sid) {
                        Some(o) if o.started <= l.started => {}
                        _ => {
                            remote_locks.insert(sid.clone(), l);
                        }
                    }
                }
            }

            self.phase("Syncing");
            for r in remotes.iter_mut() {
                self.merge(&cfg, &dirs, &canon, &scan, r, hub.as_ref(), &mut tally);
            }
            self.publish_locks(&cfg, &remote_locks, &scan);
            self.update_session_rows(&man, &remotes);
            self.save_state();
            self.cache.save(&self.sh.paths.data.join("hashcache.json"));
            // a file that grew while we were copying it: look again right away instead of next cycle
            let transient = |e: &String| e.contains("no longer has that chunk") || e.contains("changed on the other machine while syncing");
            if round < 2 && tally.errors.iter().any(transient) {
                tally.errors.retain(|e| !transient(e));
                std::thread::sleep(std::time::Duration::from_millis(800));
                continue;
            }
            if tally.wrote && round < 2 {
                tally.wrote = false;
                continue; // rescan so the manifest we serve matches what we just wrote
            }
            break;
        }
        if self.last_gc + 6 * 3600 * 1000 < now_ms() {
            if let Some(h) = self.sh.cfg().hub_enabled.then(|| Hub::new(&self.sh.cfg().hub_dir, Key::new(&self.sh.cfg().sync_key)).ok()).flatten() {
                let cfg = self.sh.cfg();
                if let Some(man) = self.lan_manifest(&cfg) {
                    let n = h.gc(&man);
                    if n > 0 {
                        self.sh.log(format!("Cloud folder: removed {n} unused chunks"));
                    }
                }
            }
            self.last_gc = now_ms();
        }
        let mut summary = Vec::new();
        if tally.received > 0 {
            summary.push(format!("received {} file(s)", tally.received));
        }
        if tally.deleted > 0 {
            summary.push(format!("removed {} file(s) deleted elsewhere", tally.deleted));
        }
        if tally.conflicts > 0 {
            summary.push(format!("{} conflict(s) resolved", tally.conflicts));
        }
        if !summary.is_empty() {
            self.sh.log(summary.join(", "));
        }
        for e in &tally.errors {
            self.sh.log(e.clone());
        }
        let res = if summary.is_empty() { "Up to date".to_string() } else { summary.join(", ") };
        self.sh.status(|s| {
            s.phase = "Idle".into();
            s.last_cycle = now_ms();
            s.last_result = res;
            s.last_error = tally.errors.first().cloned().unwrap_or_default();
            s.received_total += tally.received;
            s.conflicts_total += tally.conflicts;
        });
    }

    fn lan_manifest(&self, _cfg: &Config) -> Option<Manifest> {
        let snap = self.lan.serve.read().unwrap().clone()?;
        let j = crate::ccsync::crypto::inflate(&snap.manifest, 512 << 20)?;
        serde_json::from_slice(&j).ok()
    }

    fn local_status(&self, scan: &Scan) {
        let mut counts = [0usize; 4];
        let mut bytes = 0u64;
        let mut per: BTreeMap<String, usize> = BTreeMap::new();
        for (p, f) in &scan.files {
            counts[f.class.idx()] += 1;
            bytes += f.e.size;
            if let Some(rest) = p.strip_prefix("projects/@") {
                if let Some((a, file)) = rest.split_once('/') {
                    if !file.contains('/') && file.ends_with(".jsonl") {
                        *per.entry(a.to_string()).or_default() += 1;
                    }
                }
            }
        }
        let skipped = scan.code_skipped.clone();
        self.sh.status(|s| {
            s.code_skipped = skipped;
            s.local_files = counts;
            s.local_bytes = bytes;
            s.sessions_per_alias = per;
        });
    }

    fn update_tombs(&mut self, cfg: &Config, scan: &Scan) {
        let now = now_ms();
        let present = |p: &String| scan.files.contains_key(p) || scan.busy.contains(p);
        let gone: Vec<String> = self
            .state
            .known
            .iter()
            .filter(|p| !present(p))
            .filter(|p| classify(p).map(|c| c != Class::Session && enabled(cfg, c)).unwrap_or(false))
            .filter(|p| scan::proj_synced(cfg, p))
            // project files: only while still copied (switching build files or a project's code off,
            // or its folder missing, deletes nothing on the other PCs)
            .filter(|p| code_wanted(cfg, p) && !code_parts(p).is_some_and(|(a, _)| scan.code_no_tombs.contains(a)))
            .cloned()
            .collect();
        // a whole category vanishing is far more likely a broken/moved folder than a real clean-up
        if gone.len() > 20 && gone.len() * 2 > self.state.known.len() {
            self.sh.log(format!("{} memory/config files disappeared at once; not passing that on as deletions", gone.len()));
        } else {
            for p in gone {
                self.state.tombs.entry(p).or_insert(now);
            }
        }
        for p in scan.files.keys() {
            self.state.tombs.remove(p);
        }
        self.state.tombs.retain(|_, t| now - *t < 30 * DAY);
        self.state.known = scan.files.iter().filter(|(_, f)| f.class != Class::Session).map(|(p, _)| p.clone()).chain(scan.busy.iter().filter(|p| classify(p) != Some(Class::Session)).cloned()).collect();
    }

    fn build_manifest(&self, cfg: &Config, scan: &Scan) -> Manifest {
        let mut entries: BTreeMap<String, Entry> = scan.files.iter().map(|(p, f)| (p.clone(), f.e.clone())).collect();
        for (p, t) in &self.state.tombs {
            if classify(p).map(|c| enabled(cfg, c)).unwrap_or(false) && scan::proj_synced(cfg, p) && code_wanted(cfg, p) {
                entries.entry(p.clone()).or_insert(Entry { hash: String::new(), size: 0, mtime: *t, chunks: vec![], deleted: true });
            }
        }
        Manifest {
            machine_id: cfg.machine_id.clone(),
            machine_name: cfg.machine_name.clone(),
            generated_at: now_ms(),
            aliases: cfg.projects.iter().map(|(a, p)| (a.clone(), p.path.clone())).collect(),
            entries,
            running: self.running.clone(),
            dead_projects: self.state.my_dead.clone(),
            dead_sessions: self.state.my_dead_sess.clone(),
            sessions: BTreeMap::new(),
            app: crate::ccsync::config::app_id(),
            tools: if cfg.sync_tools { crate::ccsync::tools::current() } else { None },
            arch: scan::native_arch().to_string(),
            git: if cfg.sync_code {
                cfg.projects.iter().filter(|(a, p)| a.as_str() != HOME_ALIAS && p.sync && p.code && !p.auto).filter_map(|(a, p)| Some((a.clone(), crate::ccsync::git::read(Path::new(&p.path))?))).collect()
            } else {
                BTreeMap::new()
            },
        }
    }

    fn publish_lan(&self, man: &Manifest, scan: &Scan, canon: &Arc<Canon>) {
        let mut chunks = HashMap::new();
        for f in scan.files.values() {
            for (i, h) in f.e.chunks.iter().enumerate() {
                chunks.entry(h.clone()).or_insert_with(|| (f.abs.clone(), f.local_rel.clone(), i, f.e.hash.clone()));
            }
        }
        let snap = ServeSnap { manifest: crate::ccsync::crypto::deflate(&serde_json::to_vec(man).unwrap()), chunks, canon: canon.clone() };
        *self.lan.serve.write().unwrap() = Some(Arc::new(snap));
    }

    fn publish_hub(&mut self, hub: &Hub, man: &Manifest, scan: &Scan, canon: &Canon) -> Result<(), String> {
        let id = format!("{}|{}", man.content_id(), hub.dir.display());
        // unchanged: skip, except for a heartbeat while sessions are open here (keeps their locks alive)
        if id == self.last_hub_pub && (self.running.is_empty() || now_ms() - self.last_hub_time < 60_000) {
            return Ok(());
        }
        let mut have = hub.existing();
        let mut uploaded = 0usize;
        for f in scan.files.values() {
            let missing: Vec<usize> = f.e.chunks.iter().enumerate().filter(|(_, h)| !have.contains(&hub.name(h))).map(|(i, _)| i).collect();
            if missing.is_empty() {
                continue;
            }
            let Ok(raw) = std::fs::read(&f.abs) else { continue };
            let d = canonical_bytes(canon, &f.local_rel, raw);
            if hash(&d) != f.e.hash {
                continue; // changed since the scan; next cycle
            }
            for i in missing {
                let h = &f.e.chunks[i];
                let part = &d[i * CHUNK..((i + 1) * CHUNK).min(d.len())];
                hub.put_chunk(h, part).map_err(|e| e.to_string())?;
                have.insert(hub.name(h));
                uploaded += part.len();
            }
        }
        hub.put_manifest(man).map_err(|e| e.to_string())?;
        if uploaded > 4 << 20 {
            self.sh.log(format!("Cloud folder: uploaded {:.1} MB", uploaded as f64 / 1048576.0));
        }
        self.last_hub_pub = id;
        self.last_hub_time = now_ms();
        Ok(())
    }

    fn gather(&mut self, cfg: &Config, key: &Key, hub: Option<&Hub>, tally: &mut Tally) -> Vec<Remote> {
        let mut by_id: HashMap<String, Remote> = HashMap::new();
        let mut via_names: HashMap<String, Vec<&'static str>> = HashMap::new();
        if cfg.lan_enabled {
            let mut addrs: Vec<(std::net::SocketAddr, String)> = self.lan.live_peers().into_iter().map(|p| (p.addr, p.name)).collect();
            // the user's other PCs on Tailscale (away from home): tried quietly, after the ones found here
            let ts: Vec<std::net::SocketAddr> = if !cfg.tailscale { vec![] } else { crate::rdesk::tailscale_pcs() }.into_iter().map(|ip| std::net::SocketAddr::new(ip, cfg.lan_port)).collect();
            for m in &cfg.manual_peers {
                match lan::resolve(m, cfg.lan_port) {
                    Some(a) if !addrs.iter().any(|(x, _)| *x == a) => addrs.push((a, m.clone())),
                    Some(_) => {}
                    None => tally.errors.push(format!("Can't resolve peer {m}")),
                }
            }
            for a in &ts {
                if !addrs.iter().any(|(x, _)| x == a) {
                    addrs.push((*a, format!("tailscale:{}", a.ip())));
                }
            }
            for (addr, name) in addrs {
                let quiet = name.starts_with("tailscale:");
                match Client::connect(addr, key, &cfg.machine_id, &cfg.machine_name) {
                    Ok(mut c) => {
                        // this PC, or one already reached on the home network
                        if c.peer_id == cfg.machine_id || by_id.contains_key(&c.peer_id) {
                            continue;
                        }
                        match c.manifest() {
                            Ok(Some(m)) if m.machine_id == c.peer_id => {
                                let v = via_names.entry(m.machine_id.clone()).or_default();
                                let how = if crate::rdesk::is_tailscale(addr.ip()) { "Tailscale" } else { "LAN" };
                                if !v.contains(&how) {
                                    v.push(how);
                                }
                                by_id.insert(m.machine_id.clone(), Remote { m, via: Via::Lan(c) });
                            }
                            Ok(_) => {}
                            Err(e) if !quiet => tally.errors.push(format!("{name}: {e}")),
                            Err(_) => {}
                        }
                    }
                    // a Tailscale device without Deck (or switched off) isn't a problem
                    Err(_) if quiet => {}
                    Err(e) => tally.errors.push(format!("Can't reach {name} ({addr}): {e}")),
                }
            }
        }
        if let Some(h) = hub {
            let (ms, unreadable) = h.manifests(&cfg.machine_id);
            if unreadable > 0 {
                tally.errors.push(format!("Cloud folder has {unreadable} machine(s) using a different sync key"));
            }
            for m in ms {
                via_names.entry(m.machine_id.clone()).or_default().push("cloud");
                let newer = by_id.get(&m.machine_id).map(|r| m.generated_at > r.m.generated_at + 5_000).unwrap_or(true);
                if newer {
                    by_id.insert(m.machine_id.clone(), Remote { m, via: Via::Hub });
                }
            }
        }
        let machines: Vec<MachineStatus> = by_id
            .values()
            .map(|r| MachineStatus {
                id: r.m.machine_id.clone(),
                name: r.m.machine_name.clone(),
                via: via_names.get(&r.m.machine_id).map(|v| v.join(" + ")).unwrap_or_default(),
                generated_at: r.m.generated_at,
                files: r.m.entries.values().filter(|e| !e.deleted).count(),
                aliases: r.m.aliases.clone(),
                app: r.m.app.clone(),
                tools: r.m.tools.clone(),
                git: r.m.git.clone(),
                arch: r.m.arch.clone(),
            })
            .collect();
        self.sh.status(|s| {
            // keep machines that are offline right now, so the list doesn't flicker
            for m in machines {
                s.machines.retain(|x| x.id != m.id);
                s.machines.push(m);
            }
            s.machines.sort_by(|a, b| a.name.cmp(&b.name));
        });
        let mut v: Vec<Remote> = by_id.into_values().collect();
        v.sort_by(|a, b| a.m.machine_name.cmp(&b.m.machine_name));
        v
    }

    /// rebuilds a remote file from chunks we already have locally plus ones fetched from the peer
    fn fetch(&self, re: &Entry, local: Option<&LocalFile>, canon: &Canon, via: &mut Via, hub: Option<&Hub>) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(re.size as usize);
        let mut local_data: Option<Vec<u8>> = None;
        for (i, h) in re.chunks.iter().enumerate() {
            let mut got = None;
            if let Some(l) = local {
                if l.e.chunks.get(i) == Some(h) {
                    if local_data.is_none() {
                        local_data = std::fs::read(&l.abs).ok().map(|raw| canonical_bytes(canon, &l.local_rel, raw));
                    }
                    if let Some(d) = &local_data {
                        if let Some(part) = d.get(i * CHUNK..((i + 1) * CHUNK).min(d.len())) {
                            if hash(part) == *h {
                                got = Some(part.to_vec());
                            }
                        }
                    }
                }
            }
            let c = match got {
                Some(c) => c,
                None => {
                    let r = match via {
                        Via::Lan(c) => c.chunk(h).map_err(|e| e.to_string()),
                        Via::Hub => Err(String::new()),
                    };
                    match r {
                        Ok(c) => c,
                        Err(e) => match hub {
                            Some(hb) => hb.get_chunk(h).map_err(|e2| if e.is_empty() { e2.to_string() } else { format!("{e}; cloud folder: {e2}") })?,
                            None => return Err(e),
                        },
                    }
                }
            };
            if hash(&c) != *h {
                return Err("chunk failed verification".into());
            }
            out.extend_from_slice(&c);
        }
        if hash(&out) != re.hash {
            return Err("file changed on the other machine while syncing; will retry".into());
        }
        Ok(out)
    }

    /// Whether canonical `data` for `p` may replace the local file `abs`: a change that would alter
    /// what Claude Code runs or may do here waits for the user (guard.rs); a new one is announced.
    fn allowed_here(&self, p: &str, local_rel: &str, abs: &Path, data: &[u8], bytes: &[u8], who: Option<&str>) -> bool {
        if !crate::ccsync::guard::needs_approval(local_rel, std::fs::read(abs).ok().as_deref(), bytes) {
            return true;
        }
        let before = self.sh.undecided().len();
        let d = self.sh.held_decision(p, &hash(data), who.unwrap_or("another PC"));
        if self.sh.undecided().len() != before {
            self.sh.log(format!("{local_rel} from {} would change what Claude Code runs or may do here; waiting for your OK in Deck", who.unwrap_or("another PC")));
            crate::ccsync::held_changed(&self.sh);
        }
        d == Some(true)
    }

    /// writes canonical content to the local file for canonical path `p`; Ok(false) = try again later
    fn write_remote(&self, cfg: &Config, dirs: &ProjDirs, scan: &Scan, canon: &Canon, p: &str, data: &[u8], mtime: i64, from: Option<&str>) -> Result<bool, String> {
        let local_rel = to_local_rel(p, cfg, dirs).ok_or("no local folder for this project")?;
        if !safe_rel(&local_rel) || classify(&local_rel).is_none() {
            return Err(format!("refusing unsafe path {local_rel}"));
        }
        if let Some(owner) = scan.by_local.get(&local_rel.to_lowercase()) {
            if owner != p {
                return Err(format!("{p} and {owner} are the same file on this PC; two projects share one folder, fix it under Projects"));
            }
        }
        let abs = if code_parts(p).is_some() {
            if !code_wanted(cfg, p) || !cfg.projects.get(scan::alias_of(p).unwrap_or("")).is_some_and(|m| scan::code_root_ok(&m.path, &self.sh.paths.home, &self.sh.paths.claude)) {
                return Err("not copying this project's files here".into());
            }
            scan::code_abs(cfg, p).ok_or("no local folder for this project")?
        } else {
            self.sh.paths.claude.join(local_rel.replace('/', "\\"))
        };
        // Claude (or an editor) is writing this file right now: leave it for the next cycle
        if let Ok(m) = std::fs::metadata(&abs) {
            if let Ok(t) = m.modified() {
                if t.elapsed().map(|d| d.as_secs() < 15).unwrap_or(true) {
                    return Ok(false);
                }
            }
        }
        let bytes = scan::decode(canon, p, data);
        if !self.allowed_here(p, &local_rel, &abs, data, &bytes, from) {
            return Ok(false);
        }
        write_file(&abs, &bytes, mtime).map_err(|e| format!("{local_rel}: {e}"))?;
        self.sh.held_done(p);
        if let Ok(m) = std::fs::metadata(&abs) {
            let mt = m.modified().map(to_ms).unwrap_or(0);
            self.cache_puts.lock_anyway().push((local_rel.clone(), m.len(), mt, hash(data), scan::chunk_hashes(data)));
        }
        if let (Some(m), Some(sid)) = (from, session_id(p)) {
            self.seen.lock_anyway().push((sid.to_string(), m.to_string()));
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn merge(&mut self, cfg: &Config, dirs: &ProjDirs, canon: &Canon, scan: &Scan, r: &mut Remote, hub: Option<&Hub>, t: &mut Tally) {
        let mid = r.m.machine_id.clone();
        let who = r.m.machine_name.clone();
        let mut bases = self.state.base.remove(&mid).unwrap_or_default();
        let entries = std::mem::take(&mut r.m.entries);
        let mut dead_ok: HashMap<String, bool> = HashMap::new();
        let hidden: Vec<String> = self.state.hidden.keys().cloned().collect();
        let mut low_space: HashMap<String, bool> = HashMap::new();
        for (p, re) in &entries {
            if !safe_rel(p) || !scan::proj_synced(cfg, p) || !code_wanted(cfg, p) {
                continue;
            }
            // a project too big to copy (or one that can't be) neither sends nor receives
            if code_parts(p).is_some_and(|(a, _)| scan.code_skipped.contains_key(a)) {
                continue;
            }
            // a session deleted on this PC stays deleted (its log, subagent logs, file history, todos)
            if !hidden.is_empty() && p.len() >= 36 && classify(p) == Some(Class::Session) {
                let low = p.to_ascii_lowercase();
                if hidden.iter().any(|h| low.contains(h.as_str())) {
                    continue;
                }
            }
            if let Some(a) = scan::alias_of(p) {
                if !cfg.projects.contains_key(a) {
                    let ok = *dead_ok.entry(a.to_string()).or_insert_with(|| self.dead_allows(a, &entries));
                    if !ok {
                        continue;
                    }
                }
            }
            let Some(class) = classify(p) else { continue };
            // build output made for another kind of processor (an ARM laptop and an Intel/AMD PC)
            // wouldn't run here: this PC builds its own
            if !r.m.arch.is_empty() && r.m.arch != scan::native_arch() && scan::is_build_path(p) {
                continue;
            }
            if !enabled(cfg, class) || scan.busy.contains(p) {
                continue;
            }
            let local = scan.files.get(p);
            let b = bases.get(p).map(|s| s.as_str());
            if re.deleted {
                if class == Class::Session {
                    continue;
                }
                match local {
                    None => {
                        bases.insert(p.clone(), "-".into());
                    }
                    Some(l) if Some(l.e.hash.as_str()) == b => {
                        let trash = self.sh.paths.data.join("trash").join(stamp());
                        match stash(&l.abs, &trash, &l.local_rel) {
                            Ok(()) => {
                                bases.insert(p.clone(), "-".into());
                                t.deleted += 1;
                                t.wrote = true;
                                self.sh.log(format!("{who} deleted {}", l.local_rel));
                            }
                            Err(e) => t.errors.push(format!("Can't remove {}: {e}", l.local_rel)),
                        }
                    }
                    _ => {} // changed here since: keep ours
                }
                continue;
            }
            if let Some(l) = local {
                if l.e.hash == re.hash {
                    bases.insert(p.clone(), re.hash.clone());
                    continue;
                }
            }
            enum A {
                Skip,
                Take,
                Conflict,
            }
            let a = match local {
                None if b == Some(re.hash.as_str()) => A::Skip, // we deleted it after syncing
                None => A::Take,
                Some(l) if b == Some(l.e.hash.as_str()) => A::Take,
                Some(_) if b == Some(re.hash.as_str()) => A::Skip, // ours is newer; they'll pull it
                Some(_) => A::Conflict,
            };
            // a session open in Claude here: Claude wouldn't see a new file anyway (it holds the
            // conversation in memory), so wait until it's closed; the hook tells the user meanwhile
            if !matches!(a, A::Skip) {
                if let Some(sid) = session_id(p) {
                    if self.running.contains_key(sid) {
                        self.pending.insert(sid.to_string(), who.clone());
                        continue;
                    }
                }
            }
            match a {
                A::Skip => {}
                A::Take => {
                    // a project file onto a drive that is nearly full: stop that project for now
                    // (asked once per project per sync, not for every file)
                    if let Some((alias, _)) = code_parts(p) {
                        let full = *low_space.entry(alias.to_string()).or_insert_with(|| {
                            let root = cfg.projects.get(alias).map(|m| m.path.clone()).unwrap_or_default();
                            let probe = Path::new(&root).ancestors().find(|a| a.exists()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(&root));
                            crate::util::free_bytes(&probe).is_some_and(|f| f < 2 << 30)
                        });
                        if full {
                            if !t.errors.iter().any(|e| e.contains(&format!("project {alias}"))) {
                                t.errors.push(format!("Not copying more files of project {alias}: less than 2 GB free on its drive"));
                            }
                            continue;
                        }
                    }
                    let res = self.fetch(re, local, canon, &mut r.via, hub).and_then(|d| self.write_remote(cfg, dirs, scan, canon, p, &d, re.mtime, Some(who.as_str())));
                    match res {
                        Ok(true) => {
                            bases.insert(p.clone(), re.hash.clone());
                            t.received += 1;
                            t.wrote = true;
                        }
                        Ok(false) => {}
                        Err(e) => t.errors.push(format!("{p}: {e}")),
                    }
                }
                A::Conflict => match self.conflict(cfg, dirs, canon, scan, p, re, local.unwrap(), &mut r.via, hub, &who) {
                    Ok(Some(w)) => {
                        bases.insert(p.clone(), re.hash.clone());
                        t.conflicts += 1;
                        t.wrote |= w;
                    }
                    Ok(None) => {}
                    Err(e) => t.errors.push(format!("{p}: {e}")),
                },
            }
        }
        // forget bases for files neither side has any more
        bases.retain(|p, _| entries.contains_key(p) || scan.files.contains_key(p));
        r.m.entries = entries;
        self.state.base.insert(mid, bases);
        let puts = std::mem::take(&mut *self.cache_puts.lock_anyway());
        for (rel, size, mtime, h, chunks) in puts {
            self.cache.put(&rel, size, mtime, h, chunks);
        }
        let seen = std::mem::take(&mut *self.seen.lock_anyway());
        for (sid, machine) in seen {
            self.state.visited.entry(sid).or_default().insert(machine);
        }
    }

    /// both sides changed `p`. Ok(None) = retry next cycle, Ok(Some(wrote_something))
    #[allow(clippy::too_many_arguments)]
    fn conflict(&self, cfg: &Config, dirs: &ProjDirs, canon: &Canon, scan: &Scan, p: &str, re: &Entry, l: &LocalFile, via: &mut Via, hub: Option<&Hub>, who: &str) -> Result<Option<bool>, String> {
        let rdata = self.fetch(re, Some(l), canon, via, hub)?;
        let ldata = canonical_bytes(canon, &l.local_rel, std::fs::read(&l.abs).map_err(|e| e.to_string())?);
        if hash(&ldata) != l.e.hash {
            return Ok(None);
        }
        if p.ends_with(".jsonl") && code_parts(p).is_none() {
            if rdata.starts_with(&ldata) {
                // they continued the conversation we have: take the longer log
                return Ok(self.write_remote(cfg, dirs, scan, canon, p, &rdata, re.mtime, Some(who))?.then_some(true));
            }
            if ldata.starts_with(&rdata) {
                return Ok(Some(false)); // we're ahead; they'll take ours
            }
            // the logs share a start and differ only after it: if one side's extra lines are bookkeeping
            // (no user/assistant messages), that side has nothing to keep and the other one wins
            let common = ldata.iter().zip(rdata.iter()).take_while(|(a, b)| a == b).count();
            let cut = memchr::memrchr(b'\n', &ldata[..common]).map(|i| i + 1).unwrap_or(0);
            let has_msgs = |t: &[u8]| t.split(|&b| b == b'\n').any(|ln| memchr::memmem::find(ln, b"\"type\":\"user\"").is_some() || memchr::memmem::find(ln, b"\"type\":\"assistant\"").is_some());
            if cut > 0 {
                let (lm, rm) = (has_msgs(&ldata[cut..]), has_msgs(&rdata[cut..]));
                if !(lm && rm) {
                    let remote_wins = if lm != rm { rm } else { (rdata.len(), &re.hash) > (ldata.len(), &l.e.hash) };
                    if remote_wins {
                        return Ok(self.write_remote(cfg, dirs, scan, canon, p, &rdata, re.mtime, Some(who))?.then_some(true));
                    }
                    return Ok(Some(false));
                }
            }
            if let Some(sid) = session_id(p) {
                // the same session was continued on both machines: keep both
                let local_wins = (ldata.len(), &l.e.hash) > (rdata.len(), &re.hash);
                let (loser, loser_hash, loser_mtime) = if local_wins { (&rdata, &re.hash, re.mtime) } else { (&ldata, &l.e.hash, l.e.mtime) };
                let nid = fork_id(p, loser_hash);
                let fork_p = format!("{}{}.jsonl", &p[..p.len() - 42], nid);
                if !scan.files.contains_key(&fork_p) {
                    let fork = replace_all(loser, sid.as_bytes(), nid.as_bytes());
                    // the fork holds the other machine's messages only when that side lost
                    if !self.write_remote(cfg, dirs, scan, canon, &fork_p, &fork, loser_mtime, local_wins.then_some(who))? {
                        return Ok(None);
                    }
                }
                if !local_wins && !self.write_remote(cfg, dirs, scan, canon, p, &rdata, re.mtime, Some(who))? {
                    return Ok(None);
                }
                self.sh.log(format!("Session {sid} was continued here and on {who}; kept both (the other branch is now {nid})"));
                return Ok(Some(true));
            }
        }
        // MCP servers changed on both PCs: keep both lists. Both PCs merge the same way (the newer
        // file's server wins a name both have), so they end up with the same file.
        if p == scan::MCP_FILE {
            if let (Ok(lv), Ok(rv)) = (serde_json::from_slice::<serde_json::Value>(&ldata), serde_json::from_slice::<serde_json::Value>(&rdata)) {
                let remote_newer = (re.mtime, &re.hash) > (l.e.mtime, &l.e.hash);
                let merged = mcp::to_bytes(&if remote_newer { mcp::union(&lv, &rv) } else { mcp::union(&rv, &lv) });
                if merged == ldata {
                    return Ok(Some(false));
                }
                return Ok(self.write_remote(cfg, dirs, scan, canon, p, &merged, re.mtime.max(l.e.mtime), Some(who))?.then_some(true));
            }
        }
        // anything else: newest wins, the other version goes to the conflicts folder
        let remote_wins = (re.mtime, &re.hash) > (l.e.mtime, &l.e.hash);
        let dir = self.sh.paths.data.join("conflicts").join(stamp());
        // held for the user's OK: no copy to the conflicts folder every cycle while it waits
        if remote_wins && !self.allowed_here(p, &l.local_rel, &l.abs, &rdata, &scan::decode(canon, p, &rdata), Some(who)) {
            return Ok(None);
        }
        if remote_wins {
            let dst = dir.join(l.local_rel.replace('/', "\\"));
            let ours = std::fs::read(&l.abs).map_err(|e| e.to_string())?;
            crate::fsguard::write_for_user(&dst, &ours).map_err(|e| e.to_string())?;
            if !self.write_remote(cfg, dirs, scan, canon, p, &rdata, re.mtime, Some(who))? {
                return Ok(None);
            }
            self.sh.log(format!("{} changed here and on {who}; kept {who}'s newer copy, ours is in {}", l.local_rel, dir.display()));
        } else {
            let dst = dir.join(format!("{}.from-{}", l.local_rel.replace('/', "\\"), who));
            let _ = crate::fsguard::write_for_user(&dst, &scan::decode(canon, p, &rdata));
            self.sh.log(format!("{} changed here and on {who}; kept ours (newer), theirs is in {}", l.local_rel, dir.display()));
        }
        Ok(Some(remote_wins))
    }

    /// Claude Code keeps MCP servers in `.claude.json`, beside sign-in and per-PC state that must not
    /// sync. Their part (`mcpServers`, and each project's) is mirrored into `deck-mcp.json` in the
    /// Claude folder, which syncs like the settings (an incoming change waits for the user's OK, see
    /// guard.rs). A newer mirror (from another PC) goes into `.claude.json`; otherwise a change in
    /// `.claude.json` goes out to the mirror.
    fn sync_mcp(&mut self) {
        let claude = self.sh.paths.claude.clone();
        let cj = mcp::claude_json(&claude, &self.sh.paths.home);
        let mirror = claude.join(scan::MCP_FILE);
        let Ok(raw) = std::fs::read(&cj) else { return };
        let Ok(mut doc) = serde_json::from_slice::<serde_json::Value>(&raw) else { return };
        let here = mcp::extract(&doc);
        let here_b = mcp::to_bytes(&here);
        match std::fs::read(&mirror).ok() {
            // arrived from another PC since last time: into .claude.json
            Some(m) if hash(&m) != self.state.mcp_hash && m != here_b => {
                let Ok(theirs) = serde_json::from_slice::<serde_json::Value>(&m) else { return };
                // the very first time, servers only this PC has are kept (and go out next cycle)
                let first = self.state.mcp_hash.is_empty();
                let merged = if first { mcp::union(&here, &theirs) } else { theirs };
                mcp::apply(&mut doc, &merged);
                match crate::fsguard::write_for_user(&cj, &serde_json::to_vec_pretty(&doc).unwrap_or_default()) {
                    Ok(()) => {
                        self.sh.log("MCP servers from another PC added to Claude Code (restart open Claude Code sessions to use them)");
                        let b = mcp::to_bytes(&merged);
                        if first && b != m {
                            let _ = write_file(&mirror, &b, 0);
                        }
                        self.state.mcp_hash = hash(&b);
                    }
                    Err(e) => self.sh.log(format!("Couldn't add MCP servers to {}: {e}", cj.display())),
                }
            }
            Some(m) if m == here_b => self.state.mcp_hash = hash(&m),
            // changed here (or no mirror yet): out to the mirror
            _ => {
                if mcp::is_empty(&here) && !mirror.exists() {
                    return;
                }
                if write_file(&mirror, &here_b, 0).is_ok() {
                    self.state.mcp_hash = hash(&here_b);
                }
            }
        }
    }

    /// moves a project's synced files to the folder Claude uses for `new_path`, and rewrites paths
    /// inside every synced file for the new location
    /// false while `alias` is a deleted project and the other machine has nothing newer than the deletion
    fn dead_allows(&self, alias: &str, entries: &BTreeMap<String, Entry>) -> bool {
        let t = self.state.my_dead.get(alias).copied().unwrap_or(0).max(self.state.applied_dead.get(alias).copied().unwrap_or(0));
        if t == 0 {
            return true;
        }
        let prefix = format!("projects/@{alias}/");
        entries.iter().any(|(p, e)| !e.deleted && e.mtime > t && p.starts_with(&prefix))
    }

    /// details of each session log in the scan, reading only the logs that changed
    fn session_metas(&mut self, scan: &Scan) -> BTreeMap<String, SessionMeta> {
        let mut out = BTreeMap::new();
        let mut live: HashSet<&String> = HashSet::new();
        for (p, f) in &scan.files {
            let Some(sid) = session_id(p) else { continue };
            live.insert(p);
            let hit = self.meta_cache.get(p).filter(|(size, mtime, _)| *size == f.e.size && *mtime == f.e.mtime).map(|(_, _, m)| m.clone());
            let meta = match hit {
                Some(m) => m,
                None => {
                    let mut m = crate::ccsync::sessions::read_meta(&f.abs).unwrap_or_default();
                    m.size = f.e.size;
                    m.mtime = f.e.mtime;
                    m.alias = scan::alias_of(p).unwrap_or("").to_string();
                    self.meta_cache.insert(p.clone(), (f.e.size, f.e.mtime, m.clone()));
                    m
                }
            };
            out.insert(sid.to_ascii_lowercase(), meta);
        }
        self.meta_cache.retain(|k, _| live.contains(k));
        out
    }

    /// the list on the Sessions page: ours plus what the other machines last told us they have
    fn update_session_rows(&mut self, man: &Manifest, remotes: &[Remote]) {
        for r in remotes {
            self.peer_sessions.insert(r.m.machine_id.clone(), (r.m.machine_name.clone(), r.m.sessions.clone()));
        }
        let mut rows: BTreeMap<String, crate::ccsync::config::SessionRow> = BTreeMap::new();
        for (sid, m) in &man.sessions {
            rows.insert(sid.clone(), crate::ccsync::config::SessionRow { sid: sid.clone(), meta: m.clone(), here: true, elsewhere: vec![] });
        }
        for (name, sessions) in self.peer_sessions.values() {
            for (sid, m) in sessions {
                match rows.get_mut(sid) {
                    Some(row) => row.elsewhere.push(name.clone()),
                    None if self.state.hidden.contains_key(sid) => {} // deleted here; the other machine hasn't caught up yet
                    None => {
                        rows.insert(sid.clone(), crate::ccsync::config::SessionRow { sid: sid.clone(), meta: m.clone(), here: false, elsewhere: vec![name.clone()] });
                    }
                }
            }
        }
        let mut v: Vec<_> = rows.into_values().collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.meta.mtime));
        // for testing without a window
        if std::env::var_os("CCSYNC_DUMP_SESSIONS").is_some() {
            let txt: String = v.iter().map(|r| format!("{}|here={}|on={}|prompts={}|alias={}|title={}|first={}|last={}
", r.sid, r.here, r.elsewhere.join(","), r.meta.prompts, r.meta.alias, r.meta.title, r.meta.first, r.meta.last)).collect();
            let _ = std::fs::write(self.sh.paths.data.join("sessions-dump.txt"), txt);
        }
        self.sh.status(|s| s.sessions = v);
    }

    /// Moves a session's files on this PC to the trash folder and makes sure it is never received again.
    /// `from` = None: the user asked for it here; Some(t): another machine's deletion made at `t`.
    pub fn delete_session(&mut self, sid: &str, from: Option<i64>) -> Result<String, String> {
        let sid = sid.to_ascii_lowercase();
        if !is_sid(&sid) {
            return Err("not a session id".into());
        }
        let claude = self.sh.paths.claude.clone();
        if locks::running(&claude).contains_key(&sid) {
            return Err(format!("session {} is open in Claude Code on this PC; close it first", &sid[..8]));
        }
        let mut targets: Vec<PathBuf> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(claude.join("projects")) {
            for d in rd.flatten() {
                if d.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    targets.push(d.path().join(format!("{sid}.jsonl")));
                    targets.push(d.path().join(&sid));
                }
            }
        }
        targets.push(claude.join("file-history").join(&sid));
        if let Ok(rd) = std::fs::read_dir(claude.join("todos")) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().to_ascii_lowercase().starts_with(&sid) {
                    targets.push(e.path());
                }
            }
        }
        let trash = self.sh.paths.data.join("trash").join(format!("{}_session-{}", stamp(), &sid[..8]));
        let mut moved = 0;
        for t in targets {
            if !t.exists() {
                continue;
            }
            let Ok(rel) = t.strip_prefix(&claude) else { continue };
            let dst = trash.join(rel);
            if dst.exists() {
                return Err(format!("{} is already in the trash folder", dst.display()));
            }
            move_item(&t, &dst).map_err(|e| format!("can't move {}: {e}", t.display()))?;
            moved += 1;
        }
        self.state.hidden.insert(sid.clone(), now_ms().max(from.unwrap_or(0)));
        for b in self.state.base.values_mut() {
            b.retain(|p, _| !p.to_ascii_lowercase().contains(&sid));
        }
        self.state.visited.remove(&sid);
        self.state.my_dead_sess.retain(|_, t| now_ms() - *t < 90 * DAY);
        self.save_state();
        self.sh.wake();
        Ok(format!("Session {} deleted here ({moved} item(s) moved to Claude Sync's trash folder)", &sid[..8]))
    }

    /// Moves a project's sessions and memory on this PC to the trash folder and forgets the project.
    /// `from` = None: the user asked for it here (published so the other machines do the same);
    /// Some(t): another machine's deletion made at `t`.
    pub fn delete_project(&mut self, alias: &str, from: Option<i64>) -> Result<String, String> {
        if alias == HOME_ALIAS || alias.is_empty() {
            return Err("this project can't be deleted".into());
        }
        let claude = self.sh.paths.claude.clone();
        let mut cfg = self.sh.cfg();
        let (dirs, _) = resolve_projects(&claude, &mut cfg, &self.sh.paths.home);
        let mut moved = 0;
        if let Some(pm) = cfg.projects.get(alias) {
            let name = dirs.dir_for_path(&pm.path);
            let abs = claude.join("projects").join(&name);
            if abs.is_dir() {
                let open = locks::running(&claude);
                if open.keys().any(|sid| abs.join(format!("{sid}.jsonl")).exists()) {
                    return Err("a Claude Code session in this project is open on this PC; close it first".into());
                }
                let dst = self.sh.paths.data.join("trash").join(stamp()).join("projects").join(&name);
                moved = std::fs::read_dir(&abs).map(|r| r.flatten().count()).unwrap_or(0);
                move_item(&abs, &dst).map_err(|e| format!("can't move {}: {e}", abs.display()))?;
            }
        }
        self.sh.set_cfg(|c| {
            c.projects.remove(alias);
        });
        // its project files stay where they are; this PC just stops keeping track of them
        let prefixes = [format!("projects/@{alias}/"), format!("code/@{alias}/")];
        let ours = |p: &String| prefixes.iter().any(|x| p.starts_with(x.as_str()));
        self.state.known.retain(|p| !ours(p));
        self.state.tombs.retain(|p, _| !ours(p));
        for b in self.state.base.values_mut() {
            b.retain(|p, _| !ours(p));
        }
        match from {
            None => {
                self.state.my_dead.insert(alias.to_string(), now_ms());
            }
            Some(t) => {
                self.state.applied_dead.insert(alias.to_string(), t);
            }
        }
        self.state.my_dead.retain(|_, t| now_ms() - *t < 90 * DAY);
        self.cache = HashCache::default();
        self.save_state();
        self.sh.wake();
        Ok(format!("Project {alias} deleted here ({moved} item(s) moved to Claude Sync's trash folder)"))
    }

    pub fn remap(&mut self, alias: &str, new_path: &str) -> Result<String, String> {
        let new_path = norm_path(new_path);
        if new_path.len() < 3 {
            return Err("path too short".into());
        }
        let claude = self.sh.paths.claude.clone();
        let mut old_cfg = self.sh.cfg();
        old_cfg.sync_sessions = true;
        old_cfg.sync_memory = true;
        old_cfg.sync_config = true;
        old_cfg.sync_code = false;
        let (dirs, _) = resolve_projects(&claude, &mut old_cfg, &self.sh.paths.home);
        let Some(old) = old_cfg.projects.get(alias).cloned() else { return Err("unknown project".into()) };
        let mut new_cfg = old_cfg.clone();
        new_cfg.projects.insert(alias.into(), ProjectMap { path: new_path.clone(), auto: false, ..old.clone() });
        let canon_old = Canon::new(&old_cfg.aliases());
        let canon_new = Canon::new(&new_cfg.aliases());
        let mut tmp_cache = HashCache::default();
        let scan = scan::scan(&claude, &old_cfg, &canon_old, &dirs, &mut tmp_cache);
        let (mut moved, mut rewritten) = (0, 0);
        let mut old_dirs: HashSet<PathBuf> = HashSet::new();
        for (p, f) in &scan.files {
            let Some(new_rel) = to_local_rel(p, &new_cfg, &dirs) else { continue };
            let raw = std::fs::read(&f.abs).map_err(|e| format!("{}: {e}", f.local_rel))?;
            let new_raw = canon_new.decode(&canon_old.encode(&raw)).into_owned();
            let same_path = new_rel.eq_ignore_ascii_case(&f.local_rel);
            if same_path && new_raw == raw {
                continue;
            }
            let abs = claude.join(new_rel.replace('/', "\\"));
            write_file(&abs, &new_raw, f.e.mtime).map_err(|e| format!("{new_rel}: {e}"))?;
            if same_path {
                rewritten += 1;
            } else {
                std::fs::remove_file(&f.abs).map_err(|e| format!("{}: {e}", f.local_rel))?;
                if let Some(d) = f.abs.parent() {
                    old_dirs.insert(d.to_path_buf());
                }
                moved += 1;
            }
        }
        // drop now-empty folders left behind
        let mut ds: Vec<PathBuf> = old_dirs.into_iter().collect();
        ds.sort_by_key(|d| std::cmp::Reverse(d.as_os_str().len()));
        for d in ds {
            let mut cur = Some(d.as_path());
            while let Some(c) = cur {
                if !c.starts_with(claude.join("projects")) || c == claude.join("projects") || std::fs::remove_dir(c).is_err() {
                    break;
                }
                cur = c.parent();
            }
        }
        self.sh.set_cfg(|c| {
            c.projects.insert(alias.into(), ProjectMap { path: new_path.clone(), auto: false, ..old.clone() });
        });
        // the project's files at the new folder are compared afresh (missing ones are fetched, not
        // deleted on the other PCs)
        let prefix = format!("code/@{alias}/");
        self.state.known.retain(|p| !p.starts_with(&prefix));
        self.state.tombs.retain(|p, _| !p.starts_with(&prefix));
        for b in self.state.base.values_mut() {
            b.retain(|p, _| !p.starts_with(&prefix));
        }
        self.save_state();
        self.cache = HashCache::default();
        self.sh.wake();
        Ok(format!("{alias} now maps to {new_path}: moved {moved} file(s), updated paths in {rewritten} more"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_paths_follow_the_home_folder() {
        assert_eq!(rebase_home(r"C:\Users\Me\dev\x", Some(r"C:\Users\Me"), r"C:\Users\Laptop"), r"C:\Users\Laptop\dev\x");
        assert_eq!(rebase_home(r"c:\users\me\dev", Some(r"C:\Users\Me"), r"C:\Users\Laptop"), r"C:\Users\Laptop\dev");
        assert_eq!(rebase_home(r"C:\Users\Mel\x", Some(r"C:\Users\Me"), r"C:\Users\Laptop"), r"C:\Users\Mel\x", "not a prefix of a name");
        assert_eq!(rebase_home(r"D:\work\x", Some(r"C:\Users\Me"), r"C:\Users\Laptop"), r"D:\work\x");
        assert_eq!(rebase_home(r"D:\work\x", None, r"C:\Users\Laptop"), r"D:\work\x");
    }

    #[test]
    fn fork_ids_look_like_uuids_and_are_stable() {
        let a = fork_id("projects/@x/1.jsonl", "abc");
        assert_eq!(a.len(), 36);
        assert_eq!(a, fork_id("projects/@x/1.jsonl", "abc"));
        assert_ne!(a, fork_id("projects/@x/1.jsonl", "abd"));
        assert_eq!(session_id(&format!("projects/@x/{a}.jsonl")), Some(a.as_str()));
        assert_eq!(&a[14..15], "4");
    }
}
