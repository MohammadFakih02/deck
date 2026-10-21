//! Settings (data dir `config.json`), the shared state between the sync thread and the UI, and logging.
use crate::util::LockAnyway;
use crate::ccsync::model::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, Mutex};

pub const HOME_ALIAS: &str = "~";

/// "1.1.0+1760000000": version and build time, published so machines can compare
pub fn app_id() -> String {
    format!("{}+{}", env!("CARGO_PKG_VERSION"), env!("DECK_BUILD"))
}

/// how the other machine's build compares to this one: Less = theirs is older (an empty id is a version that didn't publish one)
pub fn compare_app(theirs: &str) -> std::cmp::Ordering {
    let ts = |s: &str| s.split_once('+').and_then(|(_, t)| t.parse::<u64>().ok()).unwrap_or(0);
    ts(theirs).cmp(&ts(&app_id()))
}

/// "1.1.0" from "1.1.0+1760000000"
pub fn app_version(id: &str) -> &str {
    if id.is_empty() {
        "older than 1.1"
    } else {
        id.split('+').next().unwrap_or(id)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProjectMap {
    /// project folder on this machine
    pub path: String,
    /// true while the path is just a guess copied from another machine
    #[serde(default)]
    pub auto: bool,
    /// "local" or the machine the alias was learned from
    #[serde(default)]
    pub origin: String,
    /// false = this project's files are neither sent nor received on this PC
    #[serde(default = "yes")]
    pub sync: bool,
    /// false = this project's sessions aren't synced (its memory and code still are): start a new
    /// session on the other PC instead of continuing one
    #[serde(default = "yes")]
    pub sessions: bool,
    /// false = this project's own folder (its code) isn't copied, even with project code on
    #[serde(default = "yes")]
    pub code: bool,
}

impl ProjectMap {
    pub fn new(path: String, auto: bool, origin: &str) -> ProjectMap {
        ProjectMap { path, auto, origin: origin.into(), sync: true, sessions: true, code: true }
    }
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub machine_id: String,
    pub machine_name: String,
    /// sealed on disk (crate::secret)
    #[serde(with = "crate::secret::field")]
    pub sync_key: String,
    pub lan_enabled: bool,
    pub lan_port: u16,
    /// also look for the other PCs over Tailscale (away from home)
    pub tailscale: bool,
    /// extra peers as host:port, for networks that block broadcasts
    pub manual_peers: Vec<String>,
    pub hub_enabled: bool,
    /// a folder synced by OneDrive / Google Drive / Dropbox
    pub hub_dir: String,
    pub sync_sessions: bool,
    pub sync_memory: bool,
    pub sync_config: bool,
    /// each project's own folder (its code), without build output and .git
    pub sync_code: bool,
    /// with `sync_code`: the build output too (target, node_modules, .venv...)
    pub sync_build: bool,
    /// tell this PC which programs (Rust, Node, Python...) the other PCs have, to install missing ones
    pub sync_tools: bool,
    pub interval_secs: u64,
    pub paused: bool,
    /// block prompts in sessions that are open on another machine (Claude Code hook)
    pub session_lock: bool,
    /// tell Claude at every session start which machine it is running on (Claude Code hook)
    pub machine_note: bool,
    pub projects: BTreeMap<String, ProjectMap>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            machine_id: crate::ccsync::crypto::hex(&crate::ccsync::crypto::random::<8>()),
            machine_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
            sync_key: crate::ccsync::crypto::gen_sync_key(),
            lan_enabled: true,
            lan_port: 47836,
            tailscale: true,
            manual_peers: vec![],
            hub_enabled: false,
            hub_dir: String::new(),
            sync_sessions: true,
            sync_memory: true,
            sync_config: true,
            sync_code: false,
            sync_build: false,
            sync_tools: true,
            interval_secs: 20,
            paused: false,
            session_lock: true,
            machine_note: true,
            projects: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn aliases(&self) -> Vec<(String, String)> {
        self.projects.iter().map(|(a, p)| (a.clone(), p.path.clone())).collect()
    }
}

pub struct Paths {
    pub claude: PathBuf,
    pub data: PathBuf,
    pub home: String,
}

impl Paths {
    pub fn new(claude: Option<String>, data: Option<String>, home: Option<String>) -> Paths {
        let home = home.unwrap_or_else(|| std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\Default".into()));
        let claude = claude
            .or_else(|| std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|s| !s.is_empty()))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".claude"));
        let data = data.map(PathBuf::from).unwrap_or_else(|| crate::settings::Settings::dir().join("claude-sync"));
        let _ = std::fs::create_dir_all(&data);
        Paths { claude, data, home: crate::ccsync::canon::norm_path(&home) }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MachineStatus {
    pub id: String,
    pub name: String,
    pub via: String,
    pub generated_at: i64,
    pub files: usize,
    pub aliases: BTreeMap<String, String>,
    /// their Deck version and build ("" = a version too old to say)
    pub app: String,
    /// the programs they have (None: not shared)
    pub tools: Option<Vec<crate::ccsync::tools::Tool>>,
    /// their projects' git remotes
    pub git: BTreeMap<String, crate::ccsync::git::GitInfo>,
    /// their processor family ("" = not said)
    pub arch: String,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub phase: String,
    pub last_cycle: i64,
    pub last_result: String,
    pub last_error: String,
    pub lan_port: u16,
    pub local_files: [usize; 4],
    /// project alias -> why its code isn't copied from this PC (too big...)
    pub code_skipped: BTreeMap<String, String>,
    pub local_bytes: u64,
    pub sessions_per_alias: BTreeMap<String, usize>,
    pub machines: Vec<MachineStatus>,
    pub received_total: usize,
    pub conflicts_total: usize,
    /// session id -> (lock, project alias)
    pub open_here: Vec<(String, crate::ccsync::locks::RunLock, String)>,
    pub open_elsewhere: Vec<(String, crate::ccsync::locks::RemoteLock, String)>,
    /// session id -> machine with newer messages
    pub pending: BTreeMap<String, String>,
    pub hook_note: String,
    /// every session known on this machine and the others, newest first
    pub sessions: Vec<SessionRow>,
}

#[derive(Clone, Debug, Default)]
pub struct SessionRow {
    pub sid: String,
    pub meta: crate::ccsync::sessions::SessionMeta,
    /// the session log is on this machine
    pub here: bool,
    /// names of the other machines that have it
    pub elsewhere: Vec<String>,
}

/// An incoming file that would change what Claude Code runs or may do here (see `guard.rs`),
/// waiting for the user's decision.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Held {
    /// the version waiting (its hash)
    pub hash: String,
    /// the PC it came from
    pub who: String,
    pub since: i64,
    /// None: not decided yet; Some(true): apply it; Some(false): keep this PC's
    pub decision: Option<bool>,
}

pub enum Cmd {
    /// sync even if paused
    SyncNow,
    /// move a project's synced files to a new local path
    Remap { alias: String, path: String },
    /// delete a project's sessions and memory here and on every other machine
    DeleteProject { alias: String },
    /// move a session to the trash here, and on the other machines too when `everywhere`
    DeleteSession { sid: String, everywhere: bool },
}

pub struct Shared {
    pub paths: Paths,
    pub cfg: Mutex<Config>,
    pub status: Mutex<Status>,
    pub log: Mutex<VecDeque<String>>,
    pub cmds: Mutex<Vec<Cmd>>,
    /// "take over": session id -> allowed here until (ms)
    pub overrides: Mutex<BTreeMap<String, i64>>,
    /// extra hook arguments when testing with non-default folders
    pub hook_extra: Mutex<String>,
    /// Claude Sync switched off in Deck: no syncing, no hooks, nothing announced or answered
    pub off: AtomicBool,
    /// canonical path -> a change held for the user's OK (kept in `held.json`)
    pub held: Mutex<BTreeMap<String, Held>>,
    wake: (Mutex<bool>, Condvar),
}

impl Shared {
    pub fn new(paths: Paths) -> Shared {
        let file = paths.data.join("config.json");
        let mut cfg: Config = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // a sealed key this Windows account can't open (a copied profile): enter the key again
        if cfg.sync_key.is_empty() {
            cfg.sync_key = crate::ccsync::crypto::gen_sync_key();
        }
        cfg.projects.insert(HOME_ALIAS.into(), ProjectMap { path: paths.home.clone(), sync: cfg.projects.get(HOME_ALIAS).map(|p| p.sync).unwrap_or(true), sessions: cfg.projects.get(HOME_ALIAS).map(|p| p.sessions).unwrap_or(true), ..ProjectMap::new(String::new(), false, "local") });
        let sh = Shared {
            paths,
            cfg: Mutex::new(cfg),
            status: Mutex::new(Status { phase: "Starting".into(), ..Default::default() }),
            log: Mutex::new(VecDeque::new()),
            cmds: Mutex::new(vec![]),
            overrides: Mutex::new(BTreeMap::new()),
            hook_extra: Mutex::new(String::new()),
            off: AtomicBool::new(false),
            held: Mutex::new(std::fs::read(paths_held(&file)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()),
            wake: (Mutex::new(false), Condvar::new()),
        };
        sh.save_cfg();
        sh
    }

    pub fn cfg(&self) -> Config {
        self.cfg.lock_anyway().clone()
    }

    pub fn save_cfg(&self) {
        let c = self.cfg();
        let _ = crate::util::write_atomic(&self.paths.data.join("config.json"), &serde_json::to_vec_pretty(&c).unwrap_or_default());
    }

    pub fn set_cfg(&self, f: impl FnOnce(&mut Config)) {
        f(&mut self.cfg.lock_anyway());
        self.save_cfg();
    }

    /// run a sync cycle now
    pub fn wake(&self) {
        *self.wake.0.lock_anyway() = true;
        self.wake.1.notify_all();
    }

    /// sleeps until woken or the timeout passes; returns true if woken
    pub fn wait(&self, secs: u64) -> bool {
        let g = self.wake.0.lock_anyway();
        let (mut g, _) = self.wake.1.wait_timeout_while(g, std::time::Duration::from_secs(secs), |w| !*w).unwrap();
        std::mem::replace(&mut *g, false)
    }

    pub fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        crate::log::debug_with(|| format!("claude sync: {msg}"));
        let t = now_ms() / 1000;
        let line = format!("{} {}", clock(t), msg);
        {
            let mut l = self.log.lock_anyway();
            l.push_back(line.clone());
            while l.len() > 800 {
                l.pop_front();
            }
        }
        let file = self.paths.data.join("claudesync.log");
        if std::fs::metadata(&file).map(|m| m.len() > 4 << 20).unwrap_or(false) {
            let _ = std::fs::rename(&file, self.paths.data.join("claudesync.old.log"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
            let _ = writeln!(f, "{} {}", now_ms(), msg);
        }
    }

    pub fn status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock_anyway());
    }

    /// What to do with version `hash` of `p` from `who`, a change the user has to allow (see
    /// `guard.rs`): Some(true) write it, Some(false) keep this PC's, None ask (it's recorded, and
    /// a newer version than the one decided on asks again).
    pub fn held_decision(&self, p: &str, hash: &str, who: &str) -> Option<bool> {
        let mut h = self.held.lock_anyway();
        if let Some(x) = h.get(p).filter(|x| x.hash == hash) {
            return x.decision;
        }
        h.insert(p.to_string(), Held { hash: hash.to_string(), who: who.to_string(), since: now_ms(), decision: None });
        self.save_held(&h);
        None
    }

    /// The user's answer for every change still waiting: `apply` them, or keep this PC's.
    pub fn decide_held(&self, apply: bool) {
        let mut h = self.held.lock_anyway();
        for x in h.values_mut().filter(|x| x.decision.is_none()) {
            x.decision = Some(apply);
        }
        self.save_held(&h);
    }

    /// `p` was written: nothing is held for it any more.
    pub fn held_done(&self, p: &str) {
        let mut h = self.held.lock_anyway();
        if h.remove(p).is_some() {
            self.save_held(&h);
        }
    }

    /// Changes waiting for a decision: (path, from which PC).
    pub fn undecided(&self) -> Vec<(String, String)> {
        self.held.lock_anyway().iter().filter(|(_, x)| x.decision.is_none()).map(|(p, x)| (p.clone(), x.who.clone())).collect()
    }

    fn save_held(&self, h: &BTreeMap<String, Held>) {
        let _ = crate::util::write_atomic(&paths_held(&self.paths.data.join("config.json")), &serde_json::to_vec_pretty(h).unwrap_or_default());
    }
}

/// `held.json` next to `config.json`.
fn paths_held(config: &std::path::Path) -> PathBuf {
    config.with_file_name("held.json")
}

/// local wall-clock HH:MM:SS for a unix time (seconds)
pub fn clock(unix: i64) -> String {
    let off = local_offset_secs();
    let s = (unix + off).rem_euclid(86400);
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn local_offset_secs() -> i64 {
    use std::sync::OnceLock;
    static OFF: OnceLock<i64> = OnceLock::new();
    *OFF.get_or_init(|| unsafe {
        let mut tz: windows_sys::Win32::System::Time::TIME_ZONE_INFORMATION = std::mem::zeroed();
        let r = windows_sys::Win32::System::Time::GetTimeZoneInformation(&mut tz);
        let bias = tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias };
        -(bias as i64) * 60
    })
}

