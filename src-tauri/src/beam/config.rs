//! Transfer (AppBeam in Deck): settings (`config.json` in its data folder), inbox history, and the
//! state shared between the network threads and Deck's page.
use crate::util::LockAnyway;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub machine_id: String,
    pub machine_name: String,
    /// sealed on disk (crate::secret)
    #[serde(with = "crate::secret::field")]
    pub code: String,
    pub port: u16,
    /// extra peers as host or host:port, for networks that block broadcasts
    pub manual_peers: Vec<String>,
    /// where received files land
    pub inbox_dir: String,
    /// ask before accepting anything (otherwise plain files are accepted automatically)
    pub ask_before_receive: bool,
    /// no longer used: installers from another PC always wait for a click here (kept so older
    /// settings files still read)
    pub trust_install: bool,
    /// folders other paired PCs may request files from (nothing outside them is ever served)
    pub shared_folders: Vec<String>,
    /// per-device answer to "may this device request files from here": off / ask / always
    pub policies: Vec<DevicePolicy>,
    /// leave build output out when a folder is sent or requested
    pub skip_build: bool,
    /// names or patterns (`*.pdb`) skipped when `skip_build` is on
    pub excludes: String,
    /// when a received project folder already exists: merge / replace / both
    pub dest_exists: String,
    /// PCs whose transfers come in without asking (never installers), until when; by their proven id
    pub trusted: Vec<Trust>,
    /// settings version (1: "ask before accepting" on by default, as Deck asks for everything;
    /// 2: PC ids are proven by each PC's key, so older trust was asked again); missing in older
    /// files = 0, not the default's 2
    #[serde(default)]
    pub v: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Trust {
    pub id: String,
    pub name: String,
    /// unix ms; 0 = always
    pub until: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DevicePolicy {
    pub id: String,
    pub name: String,
    pub mode: String,
}

/// Deck's own ports (the standalone AppBeam uses 47831/47832: the two don't talk to each other)
pub const TCP_PORT: u16 = 47834;

pub const DEFAULT_EXCLUDES: &str = "target, node_modules, .git, __pycache__, *.pdb, *.obj, dist, build";

pub fn parse_list(s: &str) -> Vec<String> {
    s.split([',', ';', '\n']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

impl Config {
    /// "off", "ask" (default) or "always"
    /// Transfers from this PC come in without asking.
    pub fn trusts(&self, id: &str) -> bool {
        self.trusted.iter().any(|t| t.id == id && (t.until == 0 || t.until > now_ms()))
    }

    pub fn policy_of(&self, id: &str) -> String {
        self.policies.iter().find(|p| p.id == id).map(|p| p.mode.clone()).unwrap_or_else(|| "ask".into())
    }
}

impl Default for Config {
    fn default() -> Self {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\Public".into());
        Config {
            machine_id: crate::beam::crypto::hex(&crate::beam::crypto::random::<8>()),
            machine_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
            code: crate::beam::crypto::gen_code(),
            port: TCP_PORT,
            manual_peers: vec![],
            inbox_dir: PathBuf::from(home).join("Downloads").join("From my PCs").display().to_string(),
            ask_before_receive: true,
            trust_install: false,
            shared_folders: vec![],
            policies: vec![],
            skip_build: true,
            excludes: DEFAULT_EXCLUDES.into(),
            dest_exists: "merge".into(),
            trusted: vec![],
            v: 2,
        }
    }
}

pub struct Paths {
    pub data: PathBuf,
}

impl Paths {
    pub fn new(data: Option<String>) -> Paths {
        let data = data.map(PathBuf::from).unwrap_or_else(|| crate::settings::Settings::dir().join("transfer"));
        let _ = std::fs::create_dir_all(&data);
        Paths { data }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum XState {
    Waiting,
    Active,
    Done,
    Failed(String),
    Cancelled,
    Declined,
}

impl XState {
    pub fn finished(&self) -> bool {
        !matches!(self, XState::Waiting | XState::Active)
    }
}

#[derive(Clone)]
pub struct Xfer {
    pub id: u64,
    pub send: bool,
    pub peer: String,
    pub label: String,
    pub total: u64,
    pub done: u64,
    pub files: usize,
    pub state: XState,
    pub started: Instant,
    pub finished_at: Option<Instant>,
    pub speed: f64,
    /// shown under the progress bar (e.g. "connection lost - continuing in 10 s")
    pub note: String,
    last_t: Instant,
    last_done: u64,
    pub cancel: Arc<AtomicBool>,
}

/// an offer from the other PC waiting for the user's answer
#[derive(Clone)]
pub struct Pending {
    pub id: u64,
    pub peer: String,
    pub label: String,
    pub files: usize,
    pub total: u64,
    pub install: bool,
    pub peer_id: String,
    /// files in the destination folder that the sender wants removed (0 = none)
    pub deletes: usize,
    pub answer: Arc<Mutex<Option<bool>>>,
}

/// another PC asking to browse or copy from this one (auto-declines after 60 s)
#[derive(Clone)]
pub struct ReqPrompt {
    pub id: u64,
    pub peer: String,
    pub what: String,
    pub since: Instant,
    /// 1 = allow, 2 = deny, 3 = always allow this device
    pub answer: Arc<Mutex<Option<u8>>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InboxItem {
    pub name: String,
    /// folder the files were saved in
    pub dir: String,
    /// installer inside `dir` (relative path), if one was found
    pub entry: Option<String>,
    /// "inno", "nsis", "burn", "msi", "msix", "exe", "zip" or ""
    pub kind: String,
    pub from: String,
    pub size: u64,
    pub files: usize,
    pub time: u64,
    #[serde(skip)]
    pub note: String,
}

pub struct Shared {
    pub paths: Paths,
    /// this PC's own key (its id is the key's)
    pub device: crate::beam::device::Device,
    cfg: Mutex<Config>,
    pub xfers: Mutex<Vec<Xfer>>,
    pub pending: Mutex<Vec<Pending>>,
    pub reqs: Mutex<Vec<ReqPrompt>>,
    /// device id -> when a browse was allowed (browsing doesn't ask again for 5 minutes)
    pub grants: Mutex<std::collections::HashMap<String, Instant>>,
    /// tests only: answer every prompt immediately
    pub test_answer: Mutex<Option<bool>>,
    pub inbox: Mutex<Vec<InboxItem>>,
    /// inbox.json's time when this process last read or wrote it (the command line adds to it too)
    inbox_seen: Mutex<Option<std::time::SystemTime>>,
    pub logs: Mutex<VecDeque<String>>,
    /// Transfer switched off in Deck: nothing is announced, answered or accepted
    pub off: AtomicBool,
    /// the command line (`deck.exe --pc-send`): finds the other PCs but never announces itself or
    /// accepts anything, so the PCs keep talking to the running Deck
    pub quiet: AtomicBool,
    next_id: AtomicU64,
    /// Deck's page is told something changed (`attention`: something waits for the user's answer)
    notify: Mutex<Option<Box<dyn Fn(bool) + Send>>>,
    last_notify: Mutex<Option<Instant>>,
}

impl Shared {
    pub fn new(paths: Paths) -> Shared {
        let mut cfg: Config = std::fs::read(paths.data.join("config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // older settings: Deck asks before anything comes in unless the PC is trusted
        if cfg.v < 1 {
            cfg.ask_before_receive = true;
            cfg.v = 1;
        }
        cfg.trusted.retain(|t| t.until == 0 || t.until > now_ms());
        // a sealed code this Windows account can't open (a copied profile): this PC pairs again
        if cfg.code.is_empty() {
            cfg.code = crate::beam::crypto::gen_code();
        }
        // this PC's id is its key's (device.rs); trust and "always allow" set before PCs proved
        // their ids named ids anyone could claim, so they are asked again once
        let device = crate::beam::device::Device::load_or_create(&paths.data);
        cfg.machine_id = device.id();
        if cfg.v < 2 {
            cfg.trusted.clear();
            cfg.policies.retain(|p| p.mode == "off");
            cfg.v = 2;
        }
        let inbox: Vec<InboxItem> = std::fs::read(paths.data.join("inbox.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let sh = Shared {
            paths,
            device,
            cfg: Mutex::new(cfg.clone()),
            xfers: Mutex::new(vec![]),
            pending: Mutex::new(vec![]),
            reqs: Mutex::new(vec![]),
            grants: Mutex::new(Default::default()),
            test_answer: Mutex::new(None),
            inbox: Mutex::new(inbox),
            inbox_seen: Mutex::new(None),
            logs: Mutex::new(VecDeque::new()),
            off: AtomicBool::new(false),
            quiet: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            notify: Mutex::new(None),
            last_notify: Mutex::new(None),
        };
        sh.save_cfg(&cfg);
        sh
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn cfg(&self) -> Config {
        self.cfg.lock_anyway().clone()
    }

    pub fn set_cfg(&self, f: impl FnOnce(&mut Config)) {
        let c = {
            let mut g = self.cfg.lock_anyway();
            f(&mut g);
            g.clone()
        };
        self.save_cfg(&c);
    }

    fn save_cfg(&self, c: &Config) {
        let _ = crate::util::write_atomic(&self.paths.data.join("config.json"), &serde_json::to_vec_pretty(c).unwrap_or_default());
    }

    fn inbox_file(&self) -> PathBuf {
        self.paths.data.join("inbox.json")
    }

    fn read_inbox_file(&self) -> Vec<InboxItem> {
        std::fs::read(self.inbox_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save_inbox(&self) {
        let mut v = self.inbox.lock_anyway().clone();
        if self.quiet.load(Ordering::Relaxed) {
            // the command line only adds what it received to what the running Deck has
            let disk = self.read_inbox_file();
            v.retain(|i| !disk.iter().any(|d| d.dir == i.dir));
            v.extend(disk);
            v.truncate(200);
        }
        let _ = crate::util::write_atomic(&self.inbox_file(), &serde_json::to_vec_pretty(&v).unwrap_or_default());
        *self.inbox_seen.lock_anyway() = std::fs::metadata(self.inbox_file()).and_then(|m| m.modified()).ok();
    }

    /// Something else (the command line) added to inbox.json: read it again.
    pub fn reload_inbox_if_changed(&self) {
        let now = std::fs::metadata(self.inbox_file()).and_then(|m| m.modified()).ok();
        let mut seen = self.inbox_seen.lock_anyway();
        if seen.is_none() {
            *seen = now;
            return;
        }
        if now == *seen {
            return;
        }
        *seen = now;
        let mut disk = self.read_inbox_file();
        let mut ib = self.inbox.lock_anyway();
        for d in disk.iter_mut() {
            if let Some(m) = ib.iter().find(|m| m.dir == d.dir) {
                d.note = m.note.clone();
            }
        }
        *ib = disk;
    }

    pub fn set_notify(&self, f: impl Fn(bool) + Send + 'static) {
        *self.notify.lock_anyway() = Some(Box::new(f));
    }

    /// something changed: Deck's page refreshes (at most 4 times a second for progress)
    pub fn repaint(&self) {
        {
            let mut l = self.last_notify.lock_anyway();
            if l.map(|t| t.elapsed().as_millis() < 250).unwrap_or(false) {
                return;
            }
            *l = Some(Instant::now());
        }
        if let Some(f) = self.notify.lock_anyway().as_ref() {
            f(false);
        }
    }

    /// something needs the user's answer: Deck shows itself
    pub fn attention(&self) {
        if let Some(f) = self.notify.lock_anyway().as_ref() {
            f(true);
        }
    }

    pub fn log(&self, msg: impl Into<String>) {
        let line = format!("{}  {}", clock(), msg.into());
        {
            let mut l = self.logs.lock_anyway();
            l.push_back(line.clone());
            while l.len() > 300 {
                l.pop_front();
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.paths.data.join("log.txt")) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
        self.repaint();
    }

    pub fn new_xfer(&self, send: bool, peer: &str, label: &str, total: u64, files: usize) -> (u64, Arc<AtomicBool>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let now = Instant::now();
        let mut x = self.xfers.lock_anyway();
        // keep the list short: drop the oldest finished ones
        while x.len() > 30 {
            match x.iter().position(|t| t.state.finished()) {
                Some(i) => {
                    x.remove(i);
                }
                None => break,
            }
        }
        x.push(Xfer {
            id,
            send,
            peer: peer.into(),
            label: label.into(),
            total,
            done: 0,
            files,
            state: XState::Waiting,
            started: now,
            finished_at: None,
            speed: 0.0,
            note: String::new(),
            last_t: now,
            last_done: 0,
            cancel: cancel.clone(),
        });
        drop(x);
        self.repaint();
        (id, cancel)
    }

    pub fn xfer(&self, id: u64, f: impl FnOnce(&mut Xfer)) {
        if let Some(x) = self.xfers.lock_anyway().iter_mut().find(|x| x.id == id) {
            f(x);
        }
    }

    pub fn set_state(&self, id: u64, s: XState) {
        self.xfer(id, |x| {
            if s.finished() {
                x.finished_at = Some(Instant::now());
                x.speed = 0.0;
            }
            if s == XState::Active {
                x.note.clear();
                x.started = Instant::now();
                x.last_t = x.started;
                x.last_done = 0;
            }
            x.state = s;
        });
        self.repaint();
    }

    /// a transfer starts over (reconnected): bytes are counted again from what is already there
    pub fn reset_attempt(&self, id: u64) {
        self.xfer(id, |x| {
            x.done = 0;
            x.last_done = 0;
            x.speed = 0.0;
            x.last_t = Instant::now();
        });
    }

    pub fn progress(&self, id: u64, n: u64) {
        let mut repaint = false;
        self.xfer(id, |x| {
            x.done += n;
            let dt = x.last_t.elapsed().as_secs_f64();
            if dt >= 0.25 {
                let inst = (x.done - x.last_done) as f64 / dt;
                x.speed = if x.speed == 0.0 { inst } else { x.speed * 0.6 + inst * 0.4 };
                x.last_t = Instant::now();
                x.last_done = x.done;
                repaint = true;
            }
        });
        if repaint {
            self.repaint();
        }
    }
}

/// local wall-clock time, HH:MM:SS
fn clock() -> String {
    crate::ccsync::config::clock((now_ms() / 1000) as i64)
}

pub fn human(b: u64) -> String {
    let b = b as f64;
    if b >= 1073741824.0 {
        format!("{:.2} GB", b / 1073741824.0)
    } else if b >= 1048576.0 {
        format!("{:.1} MB", b / 1048576.0)
    } else if b >= 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{b:.0} B")
    }
}

