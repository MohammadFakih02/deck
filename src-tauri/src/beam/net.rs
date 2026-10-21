//! Direct PC <-> PC transfer on the home network.
//!
//! Discovery: every PC broadcasts a small UDP beacon (id, name, TCP port, fingerprint of the pairing code);
//! PCs with the same code list each other. Manual peers are probed over TCP instead.
//! Transfer: one TCP connection per send. Both sides prove they know the code (challenge/response), then
//! every frame is ChaCha20-Poly1305 encrypted. The sender streams `offer -> (file, data*, end)* -> done`
//! without waiting for per-file answers, so thousands of small files go as fast as one big one.
use crate::util::LockAnyway;
use crate::beam::config::{now_ms, InboxItem, Shared, XState};
use crate::beam::crypto::{random, Key};
use crate::beam::device::Device;
use chacha20poly1305::aead::AeadInPlace;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, Tag};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const UDP_PORT: u16 = 47833;
const CHUNK: usize = 1 << 20;
const MAX_FRAME: usize = 256 << 20;
/// connections served at once (transfers between your PCs need a handful)
const MAX_CONNS: usize = 64;
/// how long a new connection has to prove the pairing code
const PROVE_WITHIN: Duration = Duration::from_secs(10);
/// size of the pieces a file is verified (and resumed) in
const BLOCK: u64 = 32 << 20;

const T_HELLO: u8 = 0;
const T_OFFER: u8 = 1;
const T_ACCEPT: u8 = 2;
const T_REJECT: u8 = 3;
const T_FILE: u8 = 4;
const T_DATA: u8 = 5;
const T_FILE_END: u8 = 6;
const T_DONE: u8 = 7;
const T_RESULT: u8 = 8;
const T_REQUEST: u8 = 9;
const T_LISTING: u8 = 10;
const T_BLOCK: u8 = 11;
/// "still working, don't time out"
const T_WAIT: u8 = 12;

#[derive(Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub seen: Instant,
}

pub struct Lan {
    pub peers: Mutex<HashMap<String, Peer>>,
    pub port: Mutex<u16>,
}

impl Lan {
    pub fn new() -> Lan {
        Lan { peers: Mutex::new(HashMap::new()), port: Mutex::new(0) }
    }

    pub fn live_peers(&self) -> Vec<Peer> {
        let mut p = self.peers.lock_anyway();
        p.retain(|_, v| v.seen.elapsed() < Duration::from_secs(15));
        let mut v: Vec<Peer> = p.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct FileEntry {
    rel: String,
    size: u64,
    /// modified time, seconds since 1970 (0 = unknown)
    #[serde(default)]
    mtime: u64,
}

#[derive(Serialize, Deserialize)]
struct Offer {
    name: String,
    files: Vec<FileEntry>,
    install: bool,
    /// a whole folder was sent: it keeps its structure and file times, and an existing copy is merged into
    #[serde(default)]
    project: bool,
    /// folders of a project (so empty ones survive too)
    #[serde(default)]
    dirs: Vec<String>,
    /// patterns the sender left out (the receiver never counts those as "extra")
    #[serde(default)]
    exclude: Vec<String>,
    /// only send what is new or changed
    #[serde(default)]
    sync: bool,
    /// remove files in the destination that the sender no longer has (the receiver's user confirms)
    #[serde(default)]
    delete_extra: bool,
}

/// the receiver's answer to an offer: files it already has, and partial files it can continue
#[derive(Serialize, Deserialize, Default)]
struct Accept {
    #[serde(default)]
    skip: Vec<usize>,
    /// same size, different time: the sender compares this blake3 to decide
    #[serde(default)]
    hash: HashMap<usize, String>,
    /// a `.part` of this many bytes exists, with this blake3 of its content
    #[serde(default)]
    resume: HashMap<usize, (u64, String)>,
}

/// a pull: ask a paired PC to send a file or folder out of its shared folders (or to list them)
#[derive(Serialize, Deserialize, Default)]
struct Request {
    path: String,
    #[serde(default)]
    list: bool,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    sync: bool,
    #[serde(default)]
    delete_extra: bool,
}

/// one line of a remote folder listing
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SendOpts {
    pub install: bool,
    /// send a folder as a project (structure, times, merge into an existing copy)
    pub project: bool,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub sync: bool,
    pub delete_extra: bool,
}

// ------------------------------------------------------------------------------------------ channel
struct Chan {
    s: TcpStream,
    tx: ChaCha20Poly1305,
    rx: ChaCha20Poly1305,
    /// the other side's protocol version (2 = verifies in blocks and can continue partial files)
    pv: u32,
    ntx: u64,
    nrx: u64,
    wbuf: Vec<u8>,
    rbuf: Vec<u8>,
    rlen: usize,
}

fn nonce(n: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[..8].copy_from_slice(&n.to_le_bytes());
    b
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl Chan {
    fn new(s: TcpStream, key: &Key, nc: &[u8], ns: &[u8], client: bool) -> Chan {
        let c2s = key.mac("c2s", &[nc, ns]);
        let s2c = key.mac("s2c", &[nc, ns]);
        let (tx, rx) = if client { (c2s, s2c) } else { (s2c, c2s) };
        Chan { s, tx: ChaCha20Poly1305::new(&tx.into()), rx: ChaCha20Poly1305::new(&rx.into()), pv: 1, ntx: 0, nrx: 0, wbuf: Vec::new(), rbuf: Vec::new(), rlen: 0 }
    }

    fn send(&mut self, t: u8, body: &[u8]) -> io::Result<()> {
        self.wbuf.clear();
        self.wbuf.extend_from_slice(&[0; 4]);
        self.wbuf.push(t);
        self.wbuf.extend_from_slice(body);
        let tag = self.tx.encrypt_in_place_detached(Nonce::from_slice(&nonce(self.ntx)), b"", &mut self.wbuf[4..]).map_err(|_| bad("encrypt"))?;
        self.ntx += 1;
        self.wbuf.extend_from_slice(&tag);
        let n = (self.wbuf.len() - 4) as u32;
        self.wbuf[..4].copy_from_slice(&n.to_le_bytes());
        self.s.write_all(&self.wbuf)
    }

    /// reads one frame; its type is returned and its payload is in `body()` until the next call
    fn recv(&mut self) -> io::Result<u8> {
        let mut l = [0u8; 4];
        self.s.read_exact(&mut l)?;
        let n = u32::from_le_bytes(l) as usize;
        if !(17..=MAX_FRAME).contains(&n) {
            return Err(bad("frame size"));
        }
        self.rbuf.resize(n, 0);
        self.s.read_exact(&mut self.rbuf[..n])?;
        let tag = Tag::clone_from_slice(&self.rbuf[n - 16..n]);
        self.rx.decrypt_in_place_detached(Nonce::from_slice(&nonce(self.nrx)), b"", &mut self.rbuf[..n - 16], &tag).map_err(|_| bad("wrong pairing code or corrupted data"))?;
        self.nrx += 1;
        self.rlen = n - 16;
        Ok(self.rbuf[0])
    }

    fn body(&self) -> &[u8] {
        &self.rbuf[1..self.rlen]
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |x, (p, q)| x | (p ^ q)) == 0
}

fn tune(s: &TcpStream) {
    let _ = s.set_nodelay(true);
    let r = socket2::SockRef::from(s);
    let _ = r.set_send_buffer_size(4 << 20);
    let _ = r.set_recv_buffer_size(4 << 20);
    // notice a vanished PC (Wi-Fi dropped, laptop asleep) within about a minute instead of hanging
    let _ = r.set_tcp_keepalive(&socket2::TcpKeepalive::new().with_time(Duration::from_secs(15)).with_interval(Duration::from_secs(5)));
}

/// connects, proves the pairing code, then both PCs prove which PC they are (device.rs) and swap
/// names; returns the channel plus the other PC's (verified) id and name
fn connect(addr: SocketAddr, key: &Key, dev: &Device, my_name: &str) -> io::Result<(Chan, String, String)> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(4))?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    s.set_write_timeout(Some(Duration::from_secs(60)))?;
    tune(&s);
    let nc = random::<16>();
    let mut hello = b"DKT1".to_vec();
    hello.extend_from_slice(&nc);
    s.write_all(&hello)?;
    let mut r = [0u8; 48];
    s.read_exact(&mut r)?;
    let ns = &r[..16];
    if !ct_eq(&r[16..], &key.mac("auth-s", &[&nc, ns])) {
        return Err(bad("the other PC has a different pairing code"));
    }
    s.write_all(&key.mac("auth-c", &[&nc, ns]))?;
    let ns = ns.to_vec();
    let mut ch = Chan::new(s, key, &nc, &ns, true);
    ch.send(T_HELLO, dev.hello(my_name, &nc, &ns, true).to_string().as_bytes())?;
    if ch.recv()? != T_HELLO {
        return Err(bad("handshake"));
    }
    let v: serde_json::Value = serde_json::from_slice(ch.body()).map_err(|_| bad("hello"))?;
    let (id, name) = crate::beam::device::check_hello(&v, &nc, &ns, false).map_err(bad)?;
    ch.pv = v["v"].as_u64().unwrap_or(1) as u32;
    Ok((ch, id, name))
}

// ------------------------------------------------------------------------------------------ names
fn clean_component(c: &str) -> Option<String> {
    if c.is_empty() || c == "." || c == ".." || c.ends_with('.') || c.ends_with(' ') {
        return None;
    }
    if c.chars().any(|ch| ch.is_control() || "<>:\"|?*\\/".contains(ch)) {
        return None;
    }
    let base = c.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str()) || (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT")) && base.as_bytes()[3].is_ascii_digit());
    (!reserved).then(|| c.to_string())
}

/// a relative path from the other PC, or None if it could escape the destination folder
fn clean_rel(rel: &str) -> Option<String> {
    let parts: Option<Vec<String>> = rel.split('/').map(clean_component).collect();
    parts.map(|p| p.join("/"))
}

fn clean_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_control() || "<>:\"|?*\\/".contains(c) { '_' } else { c }).collect();
    let t = t.trim().trim_end_matches('.').chars().take(80).collect::<String>();
    if t.is_empty() { "Received".into() } else { t }
}

fn unique_dir(base: &Path, name: &str) -> PathBuf {
    let mut p = base.join(name);
    let mut n = 2;
    while p.exists() {
        p = base.join(format!("{name} ({n})"));
        n += 1;
    }
    p
}

// ------------------------------------------------------------------------------------------ filters
fn glob_match(p: &[char], t: &[char]) -> bool {
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn glob(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().replace('\\', "/").chars().collect();
    let t: Vec<char> = text.to_lowercase().replace('\\', "/").chars().collect();
    glob_match(&p, &t)
}

#[derive(Default, Clone)]
pub struct Filter {
    pub exclude: Vec<String>,
    pub include: Vec<String>,
}

impl Filter {
    /// `inner` = path inside the folder being sent ("src/lib"), `name` = its last component
    fn excluded(&self, inner: &str, name: &str) -> bool {
        self.exclude.iter().any(|p| if p.contains('/') || p.contains('\\') { glob(p, inner) } else { glob(p, name) })
    }

    fn wanted(&self, inner: &str, name: &str) -> bool {
        self.include.is_empty() || self.include.iter().any(|p| if p.contains('/') || p.contains('\\') { glob(p, inner) } else { glob(p, name) })
    }
}

fn mtime_of(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

// ------------------------------------------------------------------------------------------ sending
struct Src {
    abs: PathBuf,
    rel: String,
    size: u64,
    mtime: u64,
}

fn walk(dir: &Path, prefix: &str, inner: &str, f: &Filter, files: &mut Vec<Src>, dirs: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut es: Vec<_> = rd.flatten().collect();
    es.sort_by_key(|e| e.file_name());
    for e in es {
        let name = e.file_name().to_string_lossy().to_string();
        let rel = format!("{prefix}/{name}");
        let inner = if inner.is_empty() { name.clone() } else { format!("{inner}/{name}") };
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                if f.excluded(&inner, &name) {
                    continue;
                }
                dirs.push(rel.clone());
                walk(&e.path(), &rel, &inner, f, files, dirs);
            }
            Ok(t) if t.is_file() => {
                if f.excluded(&inner, &name) || !f.wanted(&inner, &name) {
                    continue;
                }
                if name == ".appbeam-resume" || name.ends_with(".part.blk") {
                    continue;
                }
                let m = e.metadata();
                files.push(Src { abs: e.path(), rel, size: m.as_ref().map(|m| m.len()).unwrap_or(0), mtime: m.as_ref().map(mtime_of).unwrap_or(0) });
            }
            _ => {}
        }
    }
}

fn expand(paths: &[PathBuf], f: &Filter) -> (Vec<Src>, Vec<String>) {
    let (mut out, mut dirs) = (Vec::new(), Vec::new());
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "files".into());
        if p.is_dir() {
            walk(p, &name, "", f, &mut out, &mut dirs);
        } else if let Ok(m) = std::fs::metadata(p) {
            out.push(Src { abs: p.clone(), rel: name, size: m.len(), mtime: mtime_of(&m) });
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.rel.to_lowercase()));
    (out, dirs)
}

/// (files, bytes) a folder or file would send with these exclude patterns
pub fn scan(path: &Path, exclude: &[String]) -> (u64, u64) {
    let (files, _) = expand(&[path.to_path_buf()], &Filter { exclude: exclude.to_vec(), include: vec![] });
    (files.len() as u64, files.iter().map(|f| f.size).sum())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// blake3 of a file; `tick` runs after every megabyte (used to keep the other side from timing out)
fn hash_file_tick(p: &Path, tick: &mut dyn FnMut() -> io::Result<()>) -> io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = read_full(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        tick()?;
    }
    Ok(h.finalize().to_hex().to_string())
}

fn label_of(paths: &[PathBuf]) -> String {
    match paths {
        [p] => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "files".into()),
        p => format!("{} items", p.len()),
    }
}

/// sends files/folders to `peer` on a background thread; progress shows up in `sh.xfers`
pub fn send(sh: Arc<Shared>, peer: Peer, paths: Vec<PathBuf>, install: bool) {
    send_with(sh, peer, paths, SendOpts { install, ..Default::default() })
}

pub fn send_with(sh: Arc<Shared>, peer: Peer, paths: Vec<PathBuf>, o: SendOpts) {
    let label = label_of(&paths);
    let (id, cancel) = sh.new_xfer(true, &peer.name, &label, 0, 0);
    sh.set_resume(id, crate::beam::config::Resume { peer_id: peer.id.clone(), paths: paths.iter().map(|p| p.display().to_string()).collect(), opts: o.clone() });
    std::thread::spawn(move || {
        let r = with_retries(&sh, id, &cancel, &peer.name, || send_job(&sh, id, &cancel, &peer, &label, &paths, &o));
        finish(&sh, id, r, &peer.name, true);
    });
}

/// turns a job's result into the transfer's final state (and a log line for failures)
fn finish(sh: &Shared, id: u64, r: io::Result<()>, peer: &str, sending: bool) {
    // the receiving side may already have recorded its own outcome (e.g. the user declined)
    if r.is_ok() && sh.xfers.lock_anyway().iter().any(|x| x.id == id && x.state.finished()) {
        return;
    }
    let st = match r {
        Ok(()) => XState::Done,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => XState::Cancelled,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && sending && e.to_string().contains("in time") => XState::Failed(format!("nobody on {peer} answered in time")),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && sending => XState::Declined,
        Err(e) => XState::Failed(match e.kind() {
            io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted | io::ErrorKind::BrokenPipe => "the other PC closed the connection".into(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => format!("{peer} didn't answer in time"),
            _ => e.to_string(),
        }),
    };
    if let XState::Failed(m) = &st {
        sh.log(format!("{} {peer} failed: {m}", if sending { "Send to" } else { "Transfer from" }));
    }
    sh.set_state(id, st);
}

/// worth another try: the connection broke or the other PC wasn't reachable (as opposed to "no", "cancelled" or a wrong code)
fn retryable(e: &io::Error) -> bool {
    let m = e.to_string();
    !matches!(e.kind(), io::ErrorKind::Interrupted | io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound)
        && !m.contains("refused")
        && !m.contains("pairing code")
        && !(e.kind() == io::ErrorKind::Other && m.contains("the other PC reported") && !m.contains("damaged") && !m.contains("mismatch"))
}

/// runs a transfer attempt again after a dropped connection (Wi-Fi hiccup, sleeping laptop) until it completes;
/// every new attempt continues where the last one stopped, and it gives up only after 40 tries without progress
fn with_retries(sh: &Shared, id: u64, cancel: &AtomicBool, peer: &str, mut attempt: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let (mut fails, mut best, mut n) = (0u32, 0u64, 1u32);
    loop {
        sh.reset_attempt(id);
        let r = attempt();
        let e = match r {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        if cancel.load(Ordering::Relaxed) || !retryable(&e) {
            return Err(e);
        }
        let done = sh.xfers.lock_anyway().iter().find(|x| x.id == id).map(|x| x.done).unwrap_or(0);
        if done > best {
            best = done;
            fails = 0;
        } else {
            fails += 1;
        }
        if fails >= 40 {
            return Err(e);
        }
        n += 1;
        let wait = (5 + fails as u64 * 5).min(30);
        let why = match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => format!("{peer} stopped answering"),
            io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted | io::ErrorKind::BrokenPipe => "the connection dropped".to_string(),
            _ => e.to_string(),
        };
        sh.log(format!("Transfer with {peer}: {why} - trying again in {wait} s (attempt {n}); what arrived is kept"));
        for left in (1..=wait).rev() {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            sh.xfer(id, |x| x.note = format!("Connection lost ({why}) - continuing in {left} s (attempt {n})"));
            sh.repaint();
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn send_job(sh: &Shared, id: u64, cancel: &AtomicBool, peer: &Peer, label: &str, paths: &[PathBuf], o: &SendOpts) -> io::Result<()> {
    let cfg = sh.cfg();
    let (mut ch, _, _) = connect(peer.addr, &Key::new(&cfg.code), &sh.device, &cfg.machine_name)?;
    stream_paths(sh, id, cancel, &mut ch, &peer.name, label, paths, o)
}

/// offer -> accept -> files -> done on an open channel (used both for sends and for answering a request).
/// Every 32 MiB block is verified by the receiver as it arrives, so a dropped connection loses at most one block.
fn stream_paths(sh: &Shared, id: u64, cancel: &AtomicBool, ch: &mut Chan, peer: &str, label: &str, paths: &[PathBuf], o: &SendOpts) -> io::Result<()> {
    let flt = Filter { exclude: o.exclude.clone(), include: o.include.clone() };
    let (files, dirs) = expand(paths, &flt);
    if files.is_empty() && dirs.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "nothing to send (empty folder, or everything was skipped)"));
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    sh.xfer(id, |x| {
        x.total = total;
        x.files = files.len();
    });
    let project = o.project && paths.len() == 1 && paths[0].is_dir();
    let offer = Offer {
        name: label.to_string(),
        files: files.iter().map(|f| FileEntry { rel: f.rel.clone(), size: f.size, mtime: f.mtime }).collect(),
        install: o.install,
        project,
        dirs: if project { dirs } else { vec![] },
        exclude: if project { o.exclude.clone() } else { vec![] },
        sync: o.sync && project,
        delete_extra: o.delete_extra && project,
    };
    ch.send(T_OFFER, &serde_json::to_vec(&offer).map_err(|_| bad("offer"))?)?;
    // the other PC may be waiting for its user to press Accept (or checking what it already has)
    ch.s.set_read_timeout(Some(Duration::from_secs(180)))?;
    let acc: Accept = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        match ch.recv()? {
            T_ACCEPT => break if ch.body().is_empty() { Accept::default() } else { serde_json::from_slice(ch.body()).unwrap_or_default() },
            T_REJECT => return Err(io::Error::new(io::ErrorKind::PermissionDenied, String::from_utf8_lossy(ch.body()).to_string())),
            T_WAIT => continue,
            _ => return Err(bad("unexpected answer")),
        }
    };
    ch.s.set_read_timeout(Some(Duration::from_secs(60)))?;
    sh.set_state(id, XState::Active);
    let blockmode = ch.pv >= 2;

    // files the other PC already has are not sent again
    let mut skip: std::collections::HashSet<usize> = acc.skip.iter().cloned().collect();
    let mut last_wait = Instant::now();
    for (i, theirs) in &acc.hash {
        if let Some(f) = files.get(*i) {
            let same = hash_file_tick(&f.abs, &mut || {
                // the receiver is waiting for us: say we're alive while a big file is read
                if blockmode && last_wait.elapsed() > Duration::from_secs(8) {
                    last_wait = Instant::now();
                    ch.send(T_WAIT, &[])?;
                }
                Ok(())
            })
            .map(|h| &h == theirs)
            .unwrap_or(false);
            if same {
                skip.insert(*i);
            }
        }
    }
    sh.progress(id, skip.iter().filter_map(|i| files.get(*i)).map(|f| f.size).sum());

    let mut buf = vec![0u8; CHUNK];
    let mut wire = 0u64;
    for (i, f) in files.iter().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        // Deck has admin rights: only a file the signed-in user could open themselves is sent
        let mut fh = crate::fsguard::open_read_for_user(&f.abs)?;
        // continue a partial copy: the other PC verified every block it holds; we re-check just the last one
        let mut start = 0u64;
        if blockmode {
            if let Some((len, theirs)) = acc.resume.get(&i) {
                if *len > 0 && *len <= f.size {
                    let bs = ((*len - 1) / BLOCK) * BLOCK;
                    fh.seek(SeekFrom::Start(bs))?;
                    let mut h = blake3::Hasher::new();
                    let mut left = *len - bs;
                    while left > 0 {
                        let n = fh.read(&mut buf[..(left as usize).min(CHUNK)])?;
                        if n == 0 {
                            break;
                        }
                        h.update(&buf[..n]);
                        left -= n as u64;
                    }
                    if left == 0 && h.finalize().to_hex().as_str() == theirs.as_str() {
                        start = *len;
                    }
                    fh.seek(SeekFrom::Start(start))?;
                }
            }
        }
        let mut hdr = (i as u32).to_le_bytes().to_vec();
        if start > 0 {
            hdr.extend_from_slice(&start.to_le_bytes());
            sh.progress(id, start);
        }
        ch.send(T_FILE, &hdr)?;
        let mut sent = start;
        let (mut whole, mut sess, mut blk, mut in_blk) = (blake3::Hasher::new(), blake3::Hasher::new(), blake3::Hasher::new(), 0u64);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = read_full(&mut fh, &mut buf)?;
            if n == 0 {
                break;
            }
            if blockmode {
                blk.update(&buf[..n]);
                in_blk += n as u64;
            } else {
                whole.update(&buf[..n]);
            }
            ch.send(T_DATA, &buf[..n])?;
            sent += n as u64;
            wire += n as u64;
            sh.progress(id, n as u64);
            if blockmode && in_blk == BLOCK {
                let hh = blk.finalize();
                ch.send(T_BLOCK, hh.as_bytes())?;
                sess.update(hh.as_bytes());
                blk = blake3::Hasher::new();
                in_blk = 0;
            }
        }
        if blockmode && in_blk > 0 {
            let hh = blk.finalize();
            ch.send(T_BLOCK, hh.as_bytes())?;
            sess.update(hh.as_bytes());
        }
        if sent != f.size {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} changed while it was being sent", f.rel)));
        }
        let fin = if blockmode { sess.finalize() } else { whole.finalize() };
        ch.send(T_FILE_END, fin.as_bytes())?;
    }
    ch.send(T_DONE, &[])?;
    // the receiver may still be flushing a big last file
    loop {
        match ch.recv()? {
            T_WAIT => continue,
            T_RESULT if ch.body() == b"ok" => break,
            T_RESULT => return Err(io::Error::other(format!("the other PC reported: {}", String::from_utf8_lossy(ch.body())))),
            _ => return Err(bad("unexpected answer")),
        }
    }
    sh.log(format!("Sent {label} to {peer}: {} files, {wire} bytes sent, {} files already up to date", files.len(), skip.len()));
    Ok(())
}

// ------------------------------------------------------------------------------------------ requests: serving
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{r}"))
    } else if let Some(r) = s.strip_prefix(r"\\?\") {
        PathBuf::from(r)
    } else {
        p
    }
}

fn canon(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok().map(strip_verbatim)
}

/// shared folders as (name shown to the other PC, real folder), names made unique
fn shared_roots(cfg: &crate::beam::config::Config) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for f in &cfg.shared_folders {
        let Some(c) = canon(Path::new(f.trim())).filter(|c| c.is_dir()) else { continue };
        if out.iter().any(|(_, r)| *r == c) {
            continue;
        }
        let base = c.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| c.display().to_string().replace([':', '\\'], ""));
        let mut name = base.clone();
        let mut n = 2;
        while out.iter().any(|(x, _)| x.eq_ignore_ascii_case(&name)) {
            name = format!("{base} ({n})");
            n += 1;
        }
        out.push((name, c));
    }
    out
}

/// maps a requested path (absolute, or "SharedName/sub/folder") to a real path that is inside a shared folder
pub fn resolve_shared(cfg: &crate::beam::config::Config, req: &str) -> Result<PathBuf, String> {
    let roots = shared_roots(cfg);
    if roots.is_empty() {
        return Err("this PC doesn't share any folders yet".into());
    }
    let req = req.trim();
    if req.is_empty() || req.contains('\0') {
        return Err("no path given".into());
    }
    if req.replace('\\', "/").split('/').any(|c| c == "..") {
        return Err("\"..\" is not allowed in a path".into());
    }
    let abs = if Path::new(req).is_absolute() {
        PathBuf::from(req)
    } else {
        let norm = req.replace('\\', "/");
        let mut it = norm.trim_matches('/').splitn(2, '/');
        let first = it.next().unwrap_or("");
        let rest = it.next().unwrap_or("");
        let root = roots.iter().find(|(n, _)| n.eq_ignore_ascii_case(first)).ok_or("that isn't one of the shared folders")?;
        root.1.join(rest.replace('/', "\\"))
    };
    // resolve links and case first, then check the real location is inside a shared folder
    let c = canon(&abs).ok_or("that path wasn't found")?;
    if !roots.iter().any(|(_, r)| c.starts_with(r)) {
        return Err("that path is outside the folders shared by this PC".into());
    }
    Ok(c)
}

fn list_shared(cfg: &crate::beam::config::Config, vpath: &str) -> Result<Vec<Item>, String> {
    let vpath = vpath.trim().trim_matches(['/', '\\']);
    if vpath.is_empty() {
        let roots = shared_roots(cfg);
        if roots.is_empty() {
            return Err("this PC doesn't share any folders yet".into());
        }
        return Ok(roots.into_iter().map(|(name, _)| Item { name, dir: true, size: 0 }).collect());
    }
    let p = resolve_shared(cfg, vpath)?;
    if !p.is_dir() {
        return Err("that isn't a folder".into());
    }
    let mut v: Vec<Item> = std::fs::read_dir(&p)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let t = e.file_type().ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            if t.is_dir() {
                Some(Item { name, dir: true, size: 0 })
            } else if t.is_file() {
                Some(Item { name, dir: false, size: e.metadata().map(|m| m.len()).unwrap_or(0) })
            } else {
                None
            }
        })
        .take(5000)
        .collect();
    v.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(v)
}

fn handle_request(ch: &mut Chan, sh: &Arc<Shared>, pid: &str, pname: &str, body: &[u8]) -> io::Result<()> {
    let req: Request = serde_json::from_slice(body).map_err(|_| bad("request"))?;
    let cfg = sh.cfg();
    let what = if req.list { format!("browse \"{}\"", req.path) } else { format!("copy \"{}\"", req.path) };
    let deny = |ch: &mut Chan, why: &str| -> io::Result<()> {
        sh.log(format!("Request from {pname}: {what} - DENIED: {why}"));
        ch.send(T_REJECT, why.as_bytes())
    };
    // remember the device so Settings can list it
    if !cfg.policies.iter().any(|p| p.id == pid && p.name == pname) {
        sh.set_cfg(|c| match c.policies.iter_mut().find(|p| p.id == pid) {
            Some(p) => p.name = pname.to_string(),
            None => c.policies.push(crate::beam::config::DevicePolicy { id: pid.into(), name: pname.into(), mode: "ask".into() }),
        });
    }
    let mode = cfg.policy_of(pid);
    if mode == "off" {
        return deny(ch, "this PC doesn't allow file requests from your device");
    }
    // the path is checked before anyone is asked
    let listing_roots = req.list && req.path.trim().trim_matches(['/', '\\']).is_empty();
    let resolved = if listing_roots {
        None
    } else {
        match resolve_shared(&cfg, &req.path) {
            Ok(p) => Some(p),
            Err(e) => return deny(ch, &e),
        }
    };
    if listing_roots && shared_roots(&cfg).is_empty() {
        return deny(ch, "this PC doesn't share any folders yet");
    }

    // browsing is remembered for 5 minutes; a copy for 30 minutes after it last ran, so a transfer that had to
    // reconnect continues without asking again
    let gkey = if req.list { pid.to_string() } else { format!("{pid}|{}", req.path.to_lowercase()) };
    let gmax = if req.list { 300 } else { 1800 };
    let granted = mode == "always" || sh.grants.lock_anyway().get(&gkey).map(|t| t.elapsed() < Duration::from_secs(gmax)).unwrap_or(false);
    if !granted {
        let answer = Arc::new(Mutex::new(None));
        let pid_ = sh.next_id();
        let desc = if req.list { "browse the shared folders on this PC".to_string() } else { format!("copy \"{}\" from this PC", req.path) };
        sh.reqs.lock_anyway().push(crate::beam::config::ReqPrompt { id: pid_, peer: pname.into(), what: desc, since: Instant::now(), answer: answer.clone() });
        sh.attention();
        sh.repaint();
        let t0 = Instant::now();
        let mut res = None;
        while t0.elapsed() < Duration::from_secs(60) {
            if let Some(a) = *sh.test_answer.lock_anyway() {
                res = Some(if a { 1 } else { 2 });
                break;
            }
            if let Some(a) = *answer.lock_anyway() {
                res = Some(a);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        sh.reqs.lock_anyway().retain(|r| r.id != pid_);
        sh.repaint();
        match res {
            None => return deny(ch, "no answer within 60 seconds"),
            Some(2) => return deny(ch, "declined"),
            Some(3) => sh.set_cfg(|c| {
                if let Some(p) = c.policies.iter_mut().find(|p| p.id == pid) {
                    p.mode = "always".into();
                }
            }),
            _ => {}
        }
    }
    sh.grants.lock_anyway().insert(gkey, Instant::now());
    sh.log(format!("Request from {pname}: {what} - allowed"));

    if req.list {
        let r = list_shared(&cfg, &req.path);
        return match r {
            Ok(items) => ch.send(T_LISTING, &serde_json::to_vec(&items).unwrap_or_default()),
            Err(e) => deny(ch, &e),
        };
    }
    let path = resolved.unwrap_or_default();
    let label = label_of(std::slice::from_ref(&path));
    let (id, cancel) = sh.new_xfer(true, pname, &label, 0, 0);
    let o = SendOpts { install: false, project: path.is_dir(), exclude: req.exclude, include: req.include, sync: req.sync, delete_extra: req.delete_extra };
    let r = stream_paths(sh, id, &cancel, ch, pname, &label, &[path], &o);
    finish(sh, id, r, pname, true);
    Ok(())
}

// ------------------------------------------------------------------------------------------ requests: asking
pub struct ReqOpts {
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub sync: bool,
    pub delete_extra: bool,
}

/// asks `peer` to send `path` (inside one of its shared folders); the files arrive like any other transfer
pub fn request(sh: Arc<Shared>, peer: Peer, path: String, o: ReqOpts) {
    let label = path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("files").to_string();
    let (id, cancel) = sh.new_xfer(false, &peer.name, &label, 0, 0);
    std::thread::spawn(move || {
        let r = with_retries(&sh, id, &cancel, &peer.name, || request_job(&sh, id, &cancel, &peer, &path, &o));
        finish(&sh, id, r, &peer.name, false);
    });
}

fn request_job(sh: &Arc<Shared>, id: u64, cancel: &Arc<AtomicBool>, peer: &Peer, path: &str, o: &ReqOpts) -> io::Result<()> {
    let cfg = sh.cfg();
    let (mut ch, _, _) = connect(peer.addr, &Key::new(&cfg.code), &sh.device, &cfg.machine_name)?;
    let req = Request { path: path.into(), list: false, include: o.include.clone(), exclude: o.exclude.clone(), sync: o.sync, delete_extra: o.delete_extra };
    ch.send(T_REQUEST, &serde_json::to_vec(&req).map_err(|_| bad("request"))?)?;
    // the other PC's user may need up to a minute to answer
    ch.s.set_read_timeout(Some(Duration::from_secs(90)))?;
    sh.log(format!("Requested \"{path}\" from {}", peer.name));
    // one request, one answer: cancel before the (blocking) receive, then dispatch the reply.
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    match ch.recv()? {
        T_OFFER => {
            let body = ch.body().to_vec();
            receive_offer(&mut ch, sh, &peer.id, &peer.name, &body, Some((id, cancel.clone())))
        }
        T_REJECT => Err(io::Error::other(format!("{} refused: {}", peer.name, String::from_utf8_lossy(ch.body())))),
        _ => Err(bad("unexpected answer")),
    }
}

/// lists a shared folder of `peer` ("" = the list of shared folders); blocks until answered
pub fn list_remote(sh: &Shared, peer: &Peer, path: &str) -> Result<Vec<Item>, String> {
    let cfg = sh.cfg();
    let (mut ch, _, _) = connect(peer.addr, &Key::new(&cfg.code), &sh.device, &cfg.machine_name).map_err(|e| e.to_string())?;
    let req = Request { path: path.into(), list: true, ..Default::default() };
    ch.send(T_REQUEST, &serde_json::to_vec(&req).map_err(|_| "request".to_string())?).map_err(|e| e.to_string())?;
    ch.s.set_read_timeout(Some(Duration::from_secs(90))).map_err(|e| e.to_string())?;
    match ch.recv().map_err(|e| e.to_string())? {
        T_LISTING => serde_json::from_slice(ch.body()).map_err(|_| "unreadable listing".to_string()),
        T_REJECT => Err(format!("{} refused: {}", peer.name, String::from_utf8_lossy(ch.body()))),
        _ => Err("unexpected answer".into()),
    }
}

// ------------------------------------------------------------------------------------------ receiving
struct Cur {
    f: File,
    /// verified block hashes of the part file (32 bytes each), kept next to it for resuming
    side: Option<File>,
    /// the folder the file lands in, held (see crate::fsguard), and the names in it
    d: crate::fsguard::Dir,
    part: String,
    side_name: String,
    leaf: String,
    dest: PathBuf,
    /// whole-file hash (older peers)
    h: blake3::Hasher,
    /// hash of the block being received, and of all block hashes of this session
    blk: blake3::Hasher,
    sess: blake3::Hasher,
    in_blk: u64,
    got: u64,
    want: u64,
    mtime: u64,
}

fn serve_conn(mut s: TcpStream, sh: Arc<Shared>) -> io::Result<()> {
    // a stranger gets a few seconds to prove the pairing code, then the usual minute
    s.set_read_timeout(Some(PROVE_WITHIN))?;
    s.set_write_timeout(Some(Duration::from_secs(60)))?;
    tune(&s);
    let cfg = sh.cfg();
    let key = Key::new(&cfg.code);
    let mut h = [0u8; 20];
    s.read_exact(&mut h)?;
    if &h[..4] != b"DKT1" {
        return Err(bad("not an AppBeam client"));
    }
    let nc = &h[4..];
    let ns = random::<16>();
    let mut r = ns.to_vec();
    r.extend_from_slice(&key.mac("auth-s", &[nc, &ns]));
    s.write_all(&r)?;
    let mut m = [0u8; 32];
    s.read_exact(&mut m)?;
    if !ct_eq(&m, &key.mac("auth-c", &[nc, &ns])) {
        return Err(bad("a PC with a different pairing code tried to connect"));
    }
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut ch = Chan::new(s, &key, nc, &ns, false);
    if ch.recv()? != T_HELLO {
        return Err(bad("handshake"));
    }
    let v: serde_json::Value = serde_json::from_slice(ch.body()).unwrap_or_default();
    // the id that trust and "always allow" go by is one the other PC proved, not one it just said
    let (peer_id, peer_name) = crate::beam::device::check_hello(&v, nc, &ns, true).map_err(bad)?;
    ch.pv = v["v"].as_u64().unwrap_or(1) as u32;
    ch.send(T_HELLO, sh.device.hello(&cfg.machine_name, nc, &ns, false).to_string().as_bytes())?;
    loop {
        match ch.recv() {
            Ok(T_OFFER) => {
                let body = ch.body().to_vec();
                receive_offer(&mut ch, &sh, &peer_id, &peer_name, &body, None)?;
            }
            Ok(T_REQUEST) => {
                let body = ch.body().to_vec();
                handle_request(&mut ch, &sh, &peer_id, &peer_name, &body)?;
            }
            Ok(_) => return Err(bad("unexpected request")),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

fn strip_top(files: &mut [(String, u64)], dirs: &mut [String]) {
    let first = files.first().map(|f| f.0.as_str()).or_else(|| dirs.first().map(|d| d.as_str()));
    let Some(top) = first.and_then(|f| f.split('/').next()).map(|t| t.to_string()) else { return };
    let under = |p: &str| p.split_once('/').map(|(t, _)| t == top).unwrap_or(false);
    if files.iter().all(|f| under(&f.0)) && dirs.iter().all(|d| under(d)) {
        for f in files.iter_mut() {
            f.0 = f.0[top.len() + 1..].to_string();
        }
        for d in dirs.iter_mut() {
            *d = d[top.len() + 1..].to_string();
        }
    }
}

/// Whether an incoming offer waits for the user here. Something to install always does: the
/// sending PC can't start a program on this one without a click here, whoever it is. Deleting
/// files always does too. Otherwise a trusted PC, a transfer asked for from here, or one being
/// continued comes in by itself, and with "Ask me" off so do plain files.
fn must_ask(cfg: &crate::beam::config::Config, peer_id: &str, install: bool, requested: bool, resuming: bool, deletes: bool) -> bool {
    if install || deletes {
        return true;
    }
    !requested && !resuming && !cfg.trusts(peer_id) && cfg.ask_before_receive
}

fn resume_marker(dir: &Path) -> PathBuf {
    dir.join(".appbeam-resume")
}

/// files below `dir` that the offer doesn't contain (and the sender didn't deliberately skip)
fn extras(dir: &Path, offered: &std::collections::HashSet<String>, exclude: &[String]) -> Vec<PathBuf> {
    fn rec(base: &Path, dir: &Path, offered: &std::collections::HashSet<String>, f: &Filter, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let inner = e.path().strip_prefix(base).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if f.excluded(&inner, &name) {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => rec(base, &e.path(), offered, f, out),
                Ok(t) if t.is_file() && !name.ends_with(".part") && name != ".appbeam-resume" && !offered.contains(&inner.to_lowercase()) => {
                    out.push(e.path());
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    rec(dir, dir, offered, &Filter { exclude: exclude.to_vec(), include: vec![] }, &mut out);
    out
}

/// the folder of an interrupted copy of exactly this offer, if there is one
fn find_resume_dir(base: &Path, label: &str, ident: &str) -> Option<PathBuf> {
    let mut cands = vec![base.join(label)];
    for n in 2..12 {
        cands.push(base.join(format!("{label} ({n})")));
    }
    cands.into_iter().find(|d| std::fs::read_to_string(resume_marker(d)).map(|s| s.trim() == ident).unwrap_or(false))
}

fn blk_path(part: &Path) -> PathBuf {
    let mut p = part.to_path_buf().into_os_string();
    p.push(".blk");
    PathBuf::from(p)
}

/// how much of a `.part` file is already verified (whole blocks listed in its `.blk` file) and the hash of its last block
fn part_resume(part: &Path, want: u64) -> Option<(u64, String)> {
    let pl = std::fs::metadata(part).ok()?.len();
    let blk = blk_path(part);
    let entries = std::fs::metadata(&blk).ok()?.len() / 32;
    let len = (entries * BLOCK).min(pl).min(want);
    if len == 0 {
        return None;
    }
    let idx = (len - 1) / BLOCK;
    let mut f = File::open(&blk).ok()?;
    f.seek(SeekFrom::Start(idx * 32)).ok()?;
    let mut h = [0u8; 32];
    f.read_exact(&mut h).ok()?;
    Some((len, h.iter().map(|b| format!("{b:02x}")).collect()))
}

/// `req` = this transfer is the answer to a request we made: its transfer entry exists already, nothing needs
/// confirming (we asked for it) and a failure is left to the caller, which reconnects and continues
fn receive_offer(ch: &mut Chan, sh: &Arc<Shared>, peer_id: &str, peer: &str, body: &[u8], req: Option<(u64, Arc<AtomicBool>)>) -> io::Result<()> {
    let offer: Offer = serde_json::from_slice(body).map_err(|_| bad("offer"))?;
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut mtimes: Vec<u64> = Vec::new();
    for f in &offer.files {
        files.push((clean_rel(&f.rel).ok_or_else(|| bad("unsafe file name in offer"))?, f.size));
        mtimes.push(f.mtime);
    }
    let mut dirs: Vec<String> = Vec::new();
    for d in &offer.dirs {
        dirs.push(clean_rel(d).ok_or_else(|| bad("unsafe folder name in offer"))?);
    }
    let project = offer.project;
    if files.is_empty() && !(project && !dirs.is_empty()) {
        return Err(bad("empty offer"));
    }
    // a single folder was sent: its files already land in a folder of that name, so drop the extra level
    strip_top(&mut files, &mut dirs);
    let total: u64 = files.iter().fold(0u64, |a, f| a.saturating_add(f.1));
    let label = clean_name(&offer.name);
    let cfg = sh.cfg();
    let (id, cancel) = match &req {
        Some((id, c)) => {
            sh.xfer(*id, |x| {
                x.label = label.clone();
                x.total = total;
                x.files = files.len();
            });
            (*id, c.clone())
        }
        None => sh.new_xfer(false, peer, &label, total, files.len()),
    };
    let blockmode = ch.pv >= 2;

    // identifies this exact offer, so an interrupted copy of it can be continued
    let ident = {
        let mut h = blake3::Hasher::new();
        for (i, (r, s)) in files.iter().enumerate() {
            h.update(format!("{r}\n{s}\n{}\n", mtimes[i]).as_bytes());
        }
        h.finalize().to_hex().to_string()
    };
    // where it goes: an interrupted copy of the same offer is continued; a project folder that already exists is
    // merged into (or replaced file by file) unless the user chose "keep both"; everything else gets a fresh folder
    let base = PathBuf::from(&cfg.inbox_dir);
    // one file gets a folder named without its extension (notes.txt goes in a "notes" folder)
    let folder = if !project && files.len() == 1 { Path::new(&label).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| label.clone()) } else { label.clone() };
    let resume_dir = find_resume_dir(&base, &folder, &ident);
    let exists = project && base.join(&label).is_dir();
    let merging = resume_dir.is_some() || (exists && cfg.dest_exists != "both");
    let dir = resume_dir.clone().unwrap_or_else(|| if merging { base.join(&label) } else { unique_dir(&base, &folder) });
    let resuming = resume_dir.is_some();

    // what the destination already has
    let mut acc = Accept::default();
    let mut extra: Vec<PathBuf> = Vec::new();
    if merging {
        let use_skip = offer.sync || resuming;
        let mut last_wait = Instant::now();
        for (i, (rel, size)) in files.iter().enumerate() {
            let dest = dir.join(rel.replace('/', "\\"));
            let mut part = dest.clone().into_os_string();
            part.push(".part");
            let part = PathBuf::from(part);
            let mut skipped = false;
            if let Ok(m) = std::fs::metadata(&dest) {
                let diff = mtime_of(&m) as i64 - mtimes[i] as i64;
                if m.len() == *size && diff.abs() <= 2 && use_skip {
                    acc.skip.push(i);
                    skipped = true;
                } else if project && cfg.dest_exists == "merge" && diff > 2 {
                    // the copy here is newer: it wins
                    acc.skip.push(i);
                    skipped = true;
                } else if use_skip && m.len() == *size {
                    // same size, other time: the sender compares checksums (big files take a while - keep it posted)
                    let h = hash_file_tick(&dest, &mut || {
                        if blockmode && last_wait.elapsed() > Duration::from_secs(8) {
                            last_wait = Instant::now();
                            ch.send(T_WAIT, &[])?;
                        }
                        Ok(())
                    });
                    if let Ok(h) = h {
                        acc.hash.insert(i, h);
                    }
                }
            }
            if !skipped && blockmode {
                if let Some(r) = part_resume(&part, *size) {
                    acc.resume.insert(i, r);
                }
            }
        }
        if offer.delete_extra {
            let offered: std::collections::HashSet<String> = files.iter().map(|f| f.0.to_lowercase()).collect();
            extra = extras(&dir, &offered, &offer.exclude);
        }
    }

    // deleting files here is always asked; everything else is asked unless the sending PC is
    // trusted (or, with "Ask me" off, plain files come in by themselves). Asked for here (`req`)
    // and a continued transfer were already agreed to.
    let need_ask = must_ask(&cfg, peer_id, offer.install, req.is_some(), resuming, !extra.is_empty());
    if need_ask {
        let answer = Arc::new(Mutex::new(None));
        sh.pending.lock_anyway().push(crate::beam::config::Pending { id, peer: peer.into(), peer_id: peer_id.into(), label: label.clone(), files: files.len(), total, install: offer.install, deletes: extra.len(), answer: answer.clone() });
        sh.attention();
        sh.repaint();
        let t0 = Instant::now();
        let mut res = None;
        while t0.elapsed() < Duration::from_secs(150) {
            if let Some(a) = *sh.test_answer.lock_anyway() {
                res = Some(a);
                break;
            }
            if let Some(a) = *answer.lock_anyway() {
                res = Some(a);
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if blockmode && t0.elapsed().as_secs() % 8 == 7 {
                // keeps the sender's wait from timing out while the user decides
                ch.send(T_WAIT, &[])?;
                std::thread::sleep(Duration::from_millis(1000));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        sh.pending.lock_anyway().retain(|p| p.id != id);
        if res != Some(true) {
            ch.send(T_REJECT, if res.is_none() { b"nobody answered in time".as_slice() } else { b"declined".as_slice() })?;
            sh.set_state(id, XState::Declined);
            return Ok(());
        }
    }

    let target = match open_target(&base, &dir) {
        Ok(t) => t,
        Err(e) => {
            let _ = ch.send(T_REJECT, format!("can't create the folder: {e}").as_bytes());
            sh.set_state(id, XState::Failed(format!("can't create {}: {e}", dir.display())));
            return Ok(());
        }
    };
    for d in &dirs {
        let _ = target.walk(&format!("{d}/x"), true);
    }
    // left behind if this is interrupted: the next try of the same offer continues instead of starting over
    if let Ok(mut f) = target.create(".appbeam-resume", true) {
        let _ = f.write_all(ident.as_bytes());
    }
    let body = if acc.skip.is_empty() && acc.hash.is_empty() && acc.resume.is_empty() { Vec::new() } else { serde_json::to_vec(&acc).unwrap_or_default() };
    ch.send(T_ACCEPT, &body)?;
    sh.set_state(id, XState::Active);
    sh.progress(id, acc.skip.iter().filter_map(|i| files.get(*i)).map(|f| f.1).sum());
    let cont = if acc.skip.is_empty() && acc.resume.is_empty() { String::new() } else { format!(", continuing: {} files already here{}", acc.skip.len(), if acc.resume.is_empty() { String::new() } else { format!(" + {} partial", acc.resume.len()) }) };
    sh.log(format!("Receiving {label} from {peer} ({} files{cont})", files.len()));

    let r = receive_files(ch, sh, id, &cancel, &target, &files, &mtimes, project, &acc, blockmode);
    match r {
        Ok(()) => {
            let _ = target.remove(".appbeam-resume");
            if !extra.is_empty() {
                let n = extra.iter().filter(|p| remove_below(&target, &dir, p).is_ok()).count();
                sh.log(format!("Removed {n} file{} from {} that {peer} no longer has", if n == 1 { "" } else { "s" }, dir.display()));
            }
            sh.xfer(id, |x| x.done = x.total);
            let entry = crate::beam::setup::find_entry(&files);
            let is_setup = crate::beam::snapshot::read_manifest(&dir).is_some();
            let is_apps = crate::beam::apps::read_list(&dir).is_some();
            let entry = if is_setup || is_apps { None } else { entry };
            let kind = if is_apps { "apps".to_string() } else if is_setup { "setup".to_string() } else { entry.as_ref().map(|e| crate::beam::setup::kind_of(&dir.join(e.replace('/', "\\")))).unwrap_or_default() };
            let item = InboxItem { name: label.clone(), dir: dir.display().to_string(), entry: entry.clone(), kind: kind.clone(), from: peer.into(), size: total, files: files.len(), time: now_ms(), note: String::new() };
            {
                let mut ib = sh.inbox.lock_anyway();
                ib.retain(|i| i.dir != item.dir);
                ib.insert(0, item);
                ib.truncate(200);
            }
            sh.save_inbox();
            sh.set_state(id, XState::Done);
            sh.log(format!("Received {label} from {peer}"));
            ch.send(T_RESULT, b"ok")?;
            if offer.install {
                if kind == "apps" {
                    crate::beam::apps::install_async(sh.clone(), dir.display().to_string());
                } else if entry.is_some() {
                    crate::beam::setup::install_async(sh.clone(), dir.display().to_string(), true);
                } else {
                    sh.log(format!("{label}: asked to install, but no single installer was found in it"));
                }
            }
            Ok(())
        }
        Err(e) => {
            let cancelled = e.kind() == io::ErrorKind::Interrupted;
            // an interrupted copy keeps everything that arrived (and the marker): the next try continues from there.
            // Only a cancelled copy into a brand-new folder is cleaned up.
            if cancelled && !merging {
                // the held folder can't be deleted while it is open
                drop(target);
                let _ = std::fs::remove_dir_all(&dir);
            }
            if req.is_none() {
                let st = if cancelled {
                    XState::Cancelled
                } else if e.kind() == io::ErrorKind::UnexpectedEof || e.kind() == io::ErrorKind::ConnectionReset {
                    XState::Failed("the other PC stopped sending".into())
                } else {
                    XState::Failed(e.to_string())
                };
                if let XState::Failed(m) = &st {
                    sh.log(format!("Receiving {label} failed: {m} (what arrived is kept; sending it again continues)"));
                }
                sh.set_state(id, st);
            }
            let _ = ch.send(T_RESULT, e.to_string().as_bytes());
            Err(e)
        }
    }
}

/// The folder a transfer lands in (`dir`, directly inside the inbox folder `base`), made if needed
/// and held. Deck has admin rights and these folders are the user's: everything received is
/// written relative to the held folder, so a folder swapped for a link to elsewhere (Windows'
/// own folders) is refused instead of written into.
fn open_target(base: &Path, dir: &Path) -> io::Result<crate::fsguard::Dir> {
    let name = dir.file_name().filter(|_| dir.parent() == Some(base)).ok_or_else(|| bad("the folder isn't in the inbox"))?;
    // the inbox folder is a setting the user can change: it must be a folder they could write in
    let root = crate::fsguard::open_for_user(base)?;
    root.child(&name.to_string_lossy(), true)
}

/// Deletes `p` (a file below `dir`, which `target` holds) without going through any link.
fn remove_below(target: &crate::fsguard::Dir, dir: &Path, p: &Path) -> io::Result<()> {
    let rel = p.strip_prefix(dir).map_err(|_| bad("not in the folder"))?;
    let (d, leaf) = target.walk(&rel.to_string_lossy(), false)?;
    d.remove(&leaf)
}

fn receive_files(ch: &mut Chan, sh: &Shared, id: u64, cancel: &AtomicBool, target: &crate::fsguard::Dir, files: &[(String, u64)], mtimes: &[u64], project: bool, acc: &Accept, blockmode: bool) -> io::Result<()> {
    let mut next = 0usize;
    let mut cur: Option<Cur> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        match ch.recv()? {
            T_WAIT => {}
            T_FILE => {
                let b = ch.body();
                if cur.is_some() || (b.len() != 4 && b.len() != 12) {
                    return Err(bad("file out of order"));
                }
                let idx = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                let start = if b.len() == 12 { u64::from_le_bytes(b[4..12].try_into().unwrap()) } else { 0 };
                // when files may be skipped (a project or a continued copy) the numbers only have to go up
                if idx >= files.len() || idx < next || (!project && acc.skip.is_empty() && acc.hash.is_empty() && idx != next) {
                    return Err(bad("file out of order"));
                }
                let (rel, want) = &files[idx];
                let (d, leaf) = target.walk(rel, true)?;
                let dest = d.real().join(&leaf);
                let part = format!("{leaf}.part");
                let side_name = format!("{part}.blk");
                let (f, side) = if start > 0 {
                    if !blockmode || acc.resume.get(&idx).map(|r| r.0) != Some(start) || start > *want {
                        return Err(bad("unexpected resume offset"));
                    }
                    let mut f = d.edit(&part)?;
                    f.set_len(start)?;
                    f.seek(SeekFrom::End(0))?;
                    let mut side = d.edit(&side_name)?;
                    side.set_len(start.div_ceil(BLOCK) * 32)?;
                    side.seek(SeekFrom::End(0))?;
                    sh.progress(id, start);
                    (f, Some(side))
                } else {
                    (d.create(&part, true)?, if blockmode { Some(d.create(&side_name, true)?) } else { None })
                };
                cur = Some(Cur { f, side, d, part, side_name, leaf, dest, h: blake3::Hasher::new(), blk: blake3::Hasher::new(), sess: blake3::Hasher::new(), in_blk: 0, got: start, want: *want, mtime: mtimes[idx] });
                next = idx + 1;
            }
            T_DATA => {
                let c = cur.as_mut().ok_or_else(|| bad("data without a file"))?;
                let b = ch.body();
                c.got += b.len() as u64;
                if c.got > c.want {
                    return Err(bad("more data than announced"));
                }
                c.f.write_all(b)?;
                if blockmode {
                    c.blk.update(b);
                    c.in_blk += b.len() as u64;
                } else {
                    c.h.update(b);
                }
                sh.progress(id, b.len() as u64);
            }
            T_BLOCK if blockmode => {
                let c = cur.as_mut().ok_or_else(|| bad("block without a file"))?;
                let hh = c.blk.finalize();
                if c.in_blk == 0 || !ct_eq(hh.as_bytes(), ch.body()) {
                    // drop the damaged block; what came before it stays verified
                    let ok_len = c.got - c.in_blk;
                    let _ = c.f.set_len(ok_len);
                    return Err(bad("a block of the file was damaged in transit"));
                }
                // data first, then the note that it is verified: a crash in between only costs one block
                c.f.sync_data()?;
                if let Some(s) = c.side.as_mut() {
                    s.write_all(hh.as_bytes())?;
                }
                c.sess.update(hh.as_bytes());
                c.blk = blake3::Hasher::new();
                c.in_blk = 0;
            }
            T_FILE_END => {
                let c = cur.take().ok_or_else(|| bad("end without a file"))?;
                if c.got != c.want {
                    return Err(bad("file size mismatch"));
                }
                let good = if blockmode { c.in_blk == 0 && ct_eq(c.sess.finalize().as_bytes(), ch.body()) } else { ct_eq(c.h.finalize().as_bytes(), ch.body()) };
                if !good {
                    let rel = c.dest.display().to_string();
                    drop(c.f);
                    drop(c.side);
                    let _ = c.d.remove(&c.part);
                    let _ = c.d.remove(&c.side_name);
                    return Err(bad(&format!("checksum mismatch - {rel} was damaged in transit")));
                }
                if c.mtime > 0 {
                    let _ = c.f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(c.mtime));
                }
                let _ = c.f.sync_data();
                drop(c.f);
                drop(c.side);
                let _ = c.d.remove(&c.side_name);
                c.d.rename(&c.part, &c.leaf, true)?;
            }
            T_DONE => {
                if cur.is_some() || (!project && acc.skip.is_empty() && acc.hash.is_empty() && next != files.len()) {
                    return Err(bad("transfer incomplete"));
                }
                return Ok(());
            }
            _ => return Err(bad("unexpected frame")),
        }
    }
}


/// "A PC with a different code tried to connect" from `who`: worth one log line every 10 minutes,
/// not one per probe (PCs on Tailscale are tried every few seconds).
fn log_once(who: &str, msg: &str) -> bool {
    static SEEN: std::sync::Mutex<Option<HashMap<String, Instant>>> = std::sync::Mutex::new(None);
    if !msg.contains("different") {
        return true;
    }
    let mut g = SEEN.lock_anyway();
    let m = g.get_or_insert_with(HashMap::new);
    let fresh = m.get(who).map(|t| t.elapsed() > Duration::from_secs(600)).unwrap_or(true);
    if fresh {
        m.insert(who.to_string(), Instant::now());
    }
    fresh
}

// ------------------------------------------------------------------------------------------ startup
#[derive(Serialize, Deserialize)]
struct Beacon {
    abm: u32,
    id: String,
    name: String,
    port: u16,
    grp: String,
}

pub fn start(sh: Arc<Shared>, lan: Arc<Lan>, port_override: Option<u16>) {
    let quiet = sh.quiet.load(std::sync::atomic::Ordering::Relaxed);
    let want = if quiet { 0 } else { port_override.unwrap_or_else(|| sh.cfg().port) };
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, want)).or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)));
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            sh.log(format!("Can't listen for incoming files: {e}"));
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    *lan.port.lock_anyway() = port;
    {
        let sh = sh.clone();
        std::thread::spawn(move || {
            static CONNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            for c in listener.incoming().flatten() {
                if quiet || sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                let Some(slot) = crate::util::Slot::take(&CONNS, MAX_CONNS) else { continue };
                let sh = sh.clone();
                std::thread::spawn(move || {
                    let _slot = slot;
                    let who = c.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                    if let Err(e) = serve_conn(c, sh.clone()) {
                        if e.kind() != io::ErrorKind::UnexpectedEof && e.kind() != io::ErrorKind::ConnectionReset && log_once(&who, &e.to_string()) {
                            sh.log(format!("Connection from {who}: {e}"));
                        }
                    }
                });
            }
        });
    }

    // manual peers (networks that block broadcasts, or tests): shake hands now and then
    {
        let (sh, lan) = (sh.clone(), lan.clone());
        std::thread::spawn(move || loop {
            let c = sh.cfg();
            let off = sh.off.load(std::sync::atomic::Ordering::Relaxed);
            // typed addresses, and the user's other PCs on Tailscale (away from home, like the phone)
            let mut addrs: Vec<SocketAddr> = c.manual_peers.iter().filter_map(|m| resolve(m, c.port)).collect();
            for ip in crate::rdesk::tailscale_pcs() {
                let a = SocketAddr::new(ip, super::config::TCP_PORT);
                if !addrs.contains(&a) {
                    addrs.push(a);
                }
            }
            for addr in addrs.into_iter().filter(|_| !off) {
                if let Ok((_, id, name)) = connect(addr, &Key::new(&c.code), &sh.device, &c.machine_name) {
                    if id != c.machine_id {
                        let mut p = lan.peers.lock_anyway();
                        // a PC also found on the home network keeps that (faster) address
                        let home = p.get(&id).map(|o| !crate::rdesk::is_tailscale(o.addr.ip()) && o.seen.elapsed() < Duration::from_secs(15)).unwrap_or(false);
                        if !(home && crate::rdesk::is_tailscale(addr.ip())) {
                            p.insert(id.clone(), Peer { id, name, addr, seen: Instant::now() });
                        }
                        drop(p);
                        sh.repaint();
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        });
    }

    let udp = (|| -> io::Result<UdpSocket> {
        use socket2::{Domain, Protocol, Socket, Type};
        let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        s.set_reuse_address(true)?;
        s.set_broadcast(true)?;
        s.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, UDP_PORT)).into())?;
        Ok(s.into())
    })();
    let udp = match udp {
        Ok(u) => u,
        Err(e) => {
            sh.log(format!("Automatic discovery is off (UDP {UDP_PORT}: {e}); add the other PC's address in Settings"));
            return;
        }
    };
    let beacon = {
        let sh = sh.clone();
        move || {
            let c = sh.cfg();
            serde_json::to_vec(&Beacon { abm: 1, id: c.machine_id, name: c.machine_name, port, grp: Key::new(&c.code).group() }).unwrap()
        }
    };
    {
        let (udp, beacon, sh) = (udp.try_clone().expect("udp clone"), beacon.clone(), sh.clone());
        std::thread::spawn(move || loop {
            if !quiet && !sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = udp.send_to(&beacon(), (Ipv4Addr::BROADCAST, UDP_PORT));
            }
            std::thread::sleep(Duration::from_secs(3));
        });
    }
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        loop {
            let Ok((n, from)) = udp.recv_from(&mut buf) else { continue };
            let Ok(b) = serde_json::from_slice::<Beacon>(&buf[..n]) else { continue };
            let c = sh.cfg();
            if sh.off.load(std::sync::atomic::Ordering::Relaxed) || b.abm != 1 || b.id == c.machine_id || b.grp != Key::new(&c.code).group() {
                continue;
            }
            let addr = SocketAddr::new(from.ip(), b.port);
            let new = {
                let mut p = lan.peers.lock_anyway();
                let new = p.get(&b.id).map(|o| o.seen.elapsed() > Duration::from_secs(15)).unwrap_or(true);
                p.insert(b.id.clone(), Peer { id: b.id.clone(), name: b.name.clone(), addr, seen: Instant::now() });
                new
            };
            if new && !quiet {
                sh.log(format!("Found {} on the network ({})", b.name, addr));
                // answer directly too, in case broadcasts only get through one way
                let _ = udp.send_to(&beacon(), SocketAddr::new(from.ip(), UDP_PORT));
                sh.repaint();
            }
        }
    });
}

/// "host:port" or "host" (default port) -> address
pub fn resolve(s: &str, default_port: u16) -> Option<SocketAddr> {
    use std::net::ToSocketAddrs;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, default_port));
    }
    let with_port = if s.contains(':') { s.to_string() } else { format!("{s}:{default_port}") };
    with_port.to_socket_addrs().ok()?.find(|a| a.is_ipv4())
}

#[cfg(test)]
mod trust_tests {
    use super::*;
    use crate::beam::config::{Config, Paths, Trust};

    /// An installer always waits for a click on this PC, even from a trusted PC, one asked for
    /// from here, or a transfer being continued; so does deleting files. Plain files from a
    /// trusted PC come in by themselves.
    #[test]
    fn installers_always_ask() {
        let mut cfg = Config { ask_before_receive: true, trust_install: true, ..Default::default() };
        cfg.trusted.push(Trust { id: "pc-a".into(), name: "A".into(), until: 0 });
        assert!(must_ask(&cfg, "pc-a", true, false, false, false), "trusted, still asked to install");
        assert!(must_ask(&cfg, "pc-a", true, true, true, false), "asked for and continued, still asked to install");
        assert!(must_ask(&cfg, "pc-a", false, false, false, true), "deleting is asked");
        assert!(!must_ask(&cfg, "pc-a", false, false, false, false), "plain files from a trusted PC");
        assert!(must_ask(&cfg, "pc-b", false, false, false, false), "plain files from another PC with Ask on");
        cfg.ask_before_receive = false;
        assert!(!must_ask(&cfg, "pc-b", false, false, false, false), "Ask off: plain files come in");
        assert!(must_ask(&cfg, "pc-b", true, false, false, false), "Ask off: installers still ask");
    }

    fn shared(name: &str, code: &str) -> Arc<Shared> {
        let dir = std::env::temp_dir().join(format!("deck-beam-{name}-{}", now_ms()));
        let sh = Arc::new(Shared::new(Paths::new(Some(dir.display().to_string()))));
        sh.set_cfg(|c| c.code = code.into());
        sh
    }

    /// Over a real connection (this PC only): each side learns the other's proven id; a PC with
    /// another pairing code is turned away before any message is read.
    #[test]
    fn connections_prove_both_pcs() {
        let (a, b) = (shared("a", "AAAA-BBBB-CCCC-DDDD-EEEE"), shared("b", "AAAA-BBBB-CCCC-DDDD-EEEE"));
        assert_ne!(a.device.id(), b.device.id());
        assert_eq!(a.cfg().machine_id, a.device.id(), "a PC's id is its key's");
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let server = {
            let b = b.clone();
            std::thread::spawn(move || {
                let (s, _) = l.accept().unwrap();
                let _ = serve_conn(s, b.clone());
                let (s, _) = l.accept().unwrap();
                serve_conn(s, b).map_err(|e| e.to_string())
            })
        };
        let (ch, id, name) = connect(addr, &Key::new(&a.cfg().code), &a.device, "PC-A").unwrap();
        assert_eq!(id, b.device.id());
        assert_eq!(name, b.cfg().machine_name);
        drop(ch);
        let wrong = shared("w", "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ");
        let e = connect(addr, &Key::new(&wrong.cfg().code), &wrong.device, "W").err().expect("refused");
        assert!(e.to_string().contains("different pairing code"), "{e}");
        assert!(server.join().unwrap().is_err(), "the server got no message from it");
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::beam::config::Config;

    /// Names in an offer from the other PC can't leave the destination folder, name a device, or
    /// reach an alternate data stream.
    #[test]
    fn names_from_the_other_pc_stay_inside() {
        assert_eq!(clean_rel("photos/2026/a.jpg").as_deref(), Some("photos/2026/a.jpg"));
        for bad in ["../x", "a/../b", "a/./b", "C:/x", "a\\b", "file.txt:secret", "x::$DATA", "CON", "com1.txt", "LPT9", "nul.tar.gz", "a./b", "a /b", "", "a//b", "/abs", "a/b\u{0}c", "x?y", "AUX/inside"] {
            assert_eq!(clean_rel(bad), None, "{bad:?} must be refused");
        }
        assert_eq!(clean_component("COM10").as_deref(), Some("COM10"), "only COM1-COM9 are devices");
        assert_eq!(clean_name("..\\..\\Windows"), ".._.._Windows", "a plain folder name");
        assert_eq!(clean_name("   "), "Received");
    }

    /// A request for a path is answered only inside a shared folder: no "..", no other folder,
    /// and no link inside a shared folder that leads out of it.
    #[test]
    fn requests_stay_inside_shared_folders() {
        let base = std::env::temp_dir().join(format!("deck-share-req-{}", now_ms()));
        let (shared, outside) = (base.join("Shared"), base.join("Private"));
        std::fs::create_dir_all(shared.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(shared.join("sub").join("a.txt"), b"a").unwrap();
        std::fs::write(outside.join("secret.txt"), b"s").unwrap();
        let cfg = Config { shared_folders: vec![shared.display().to_string()], ..Default::default() };
        assert!(resolve_shared(&cfg, "Shared/sub/a.txt").is_ok());
        assert!(resolve_shared(&cfg, "Shared/../Private/secret.txt").is_err());
        assert!(resolve_shared(&cfg, &outside.join("secret.txt").display().to_string()).is_err(), "an absolute path elsewhere");
        assert!(resolve_shared(&cfg, "Private/secret.txt").is_err(), "not a shared folder's name");
        use std::os::windows::process::CommandExt;
        let ok = std::process::Command::new("cmd.exe").args(["/c", "mklink", "/J"]).arg(shared.join("link")).arg(&outside).creation_flags(crate::util::CREATE_NO_WINDOW).output().unwrap();
        assert!(ok.status.success());
        assert!(resolve_shared(&cfg, "Shared/link/secret.txt").is_err(), "a link out of the shared folder");
        assert!(resolve_shared(&Config::default(), "Shared/sub/a.txt").is_err(), "nothing shared");
        let _ = std::fs::remove_dir(shared.join("link"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A frame announcing a size outside the allowed range is refused before anything is read
    /// or allocated for it.
    #[test]
    fn oversized_frames_are_refused() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut peer = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let mut ch = Chan::new(s, &Key::new("AAAA-BBBB-CCCC-DDDD-EEEE"), b"n1", b"n2", false);
        peer.write_all(&u32::MAX.to_le_bytes()).unwrap();
        assert!(ch.recv().unwrap_err().to_string().contains("frame size"));
        assert!(ch.rbuf.capacity() < 1 << 20, "nothing was allocated for it");
    }
}
