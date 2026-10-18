//! Direct PC <-> laptop sync on the home network.
//!
//! Discovery: every machine broadcasts a small UDP beacon on port 47821 with its id, TCP port and a
//! fingerprint of the sync key; machines with the same key list each other as peers.
//! Transfer: TCP. Both sides prove they know the sync key (challenge/response), then every frame is
//! ChaCha20-Poly1305 encrypted with per-connection keys. A peer can ask for our manifest, for a chunk
//! by hash, or "poke" us to sync right away.
use crate::util::LockAnyway;
use crate::ccsync::canon::Canon;
use crate::ccsync::config::Shared;
use crate::ccsync::crypto::{deflate, inflate, random, Key};
use crate::ccsync::model::{Manifest, CHUNK};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const UDP_PORT: u16 = 47835;
const MAX_FRAME: usize = 96 << 20;
/// connections served at once (a sync needs one or two per machine)
const MAX_CONNS: usize = 64;

const T_HELLO: u8 = 0;
const T_GET_MANIFEST: u8 = 1;
const T_MANIFEST: u8 = 2;
const T_GET_CHUNK: u8 = 3;
const T_CHUNK: u8 = 4;
const T_NONE: u8 = 5;
const T_POKE: u8 = 6;
const T_OK: u8 = 7;

/// what we serve: the current manifest and where each chunk can be read from
pub struct ServeSnap {
    pub manifest: Vec<u8>,
    /// chunk hash -> (file, canonical rel, chunk index, file hash)
    pub chunks: HashMap<String, (PathBuf, String, usize, String)>,
    pub canon: Arc<Canon>,
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub seen: Instant,
}

pub struct Lan {
    pub peers: Mutex<HashMap<String, Peer>>,
    pub serve: RwLock<Option<Arc<ServeSnap>>>,
    pub port: Mutex<u16>,
}

impl Lan {
    pub fn new() -> Lan {
        Lan { peers: Mutex::new(HashMap::new()), serve: RwLock::new(None), port: Mutex::new(0) }
    }

    pub fn live_peers(&self) -> Vec<Peer> {
        let mut p = self.peers.lock_anyway();
        p.retain(|_, v| v.seen.elapsed() < Duration::from_secs(60));
        p.values().cloned().collect()
    }
}

// ------------------------------------------------------------------------------------------ channel
struct Chan {
    s: TcpStream,
    tx: ChaCha20Poly1305,
    rx: ChaCha20Poly1305,
    ntx: u64,
    nrx: u64,
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
    fn send(&mut self, t: u8, body: &[u8]) -> io::Result<()> {
        let mut pt = Vec::with_capacity(body.len() + 1);
        pt.push(t);
        pt.extend_from_slice(body);
        let ct = self.tx.encrypt(Nonce::from_slice(&nonce(self.ntx)), pt.as_slice()).map_err(|_| bad("encrypt"))?;
        self.ntx += 1;
        let mut out = (ct.len() as u32).to_le_bytes().to_vec();
        out.extend(ct);
        self.s.write_all(&out)
    }

    fn recv(&mut self) -> io::Result<(u8, Vec<u8>)> {
        let mut l = [0u8; 4];
        self.s.read_exact(&mut l)?;
        let n = u32::from_le_bytes(l) as usize;
        if !(17..=MAX_FRAME).contains(&n) {
            return Err(bad("frame size"));
        }
        let mut ct = vec![0u8; n];
        self.s.read_exact(&mut ct)?;
        let mut pt = self.rx.decrypt(Nonce::from_slice(&nonce(self.nrx)), ct.as_slice()).map_err(|_| bad("wrong sync key or corrupted frame"))?;
        self.nrx += 1;
        let t = pt.remove(0);
        Ok((t, pt))
    }
}

fn session(key: &Key, nc: &[u8], ns: &[u8], client: bool) -> (ChaCha20Poly1305, ChaCha20Poly1305) {
    let c2s = key.mac("c2s", &[nc, ns]);
    let s2c = key.mac("s2c", &[nc, ns]);
    let (tx, rx) = if client { (c2s, s2c) } else { (s2c, c2s) };
    (ChaCha20Poly1305::new(&tx.into()), ChaCha20Poly1305::new(&rx.into()))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |x, (p, q)| x | (p ^ q)) == 0
}

// ------------------------------------------------------------------------------------------ client
pub struct Client {
    ch: Chan,
    pub peer_id: String,
    #[allow(dead_code)]
    pub peer_name: String,
}

impl Client {
    pub fn connect(addr: SocketAddr, key: &Key, my_id: &str, my_name: &str) -> io::Result<Client> {
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(4))?;
        s.set_read_timeout(Some(Duration::from_secs(60)))?;
        s.set_write_timeout(Some(Duration::from_secs(60)))?;
        s.set_nodelay(true)?;
        let nc = random::<16>();
        let mut hello = b"DKC1".to_vec();
        hello.extend_from_slice(&nc);
        s.write_all(&hello)?;
        let mut r = [0u8; 48];
        s.read_exact(&mut r)?;
        let ns = &r[..16];
        if !ct_eq(&r[16..], &key.mac("auth-s", &[&nc, ns])) {
            return Err(bad("peer has a different sync key"));
        }
        s.write_all(&key.mac("auth-c", &[&nc, ns]))?;
        let (tx, rx) = session(key, &nc, ns, true);
        let mut ch = Chan { s, tx, rx, ntx: 0, nrx: 0 };
        ch.send(T_HELLO, serde_json::json!({"id": my_id, "name": my_name}).to_string().as_bytes())?;
        let (t, b) = ch.recv()?;
        if t != T_HELLO {
            return Err(bad("handshake"));
        }
        let v: serde_json::Value = serde_json::from_slice(&b).map_err(|_| bad("hello"))?;
        Ok(Client { ch, peer_id: v["id"].as_str().unwrap_or("").into(), peer_name: v["name"].as_str().unwrap_or("").into() })
    }

    pub fn manifest(&mut self) -> io::Result<Option<Manifest>> {
        self.ch.send(T_GET_MANIFEST, &[])?;
        match self.ch.recv()? {
            (T_MANIFEST, b) => {
                let j = inflate(&b, 512 << 20).ok_or_else(|| bad("manifest"))?;
                Ok(Some(serde_json::from_slice(&j).map_err(|_| bad("manifest json"))?))
            }
            _ => Ok(None),
        }
    }

    pub fn chunk(&mut self, h: &str) -> io::Result<Vec<u8>> {
        self.ch.send(T_GET_CHUNK, h.as_bytes())?;
        match self.ch.recv()? {
            (T_CHUNK, b) => inflate(&b, CHUNK).ok_or_else(|| bad("chunk")),
            _ => Err(io::Error::new(io::ErrorKind::NotFound, "peer no longer has that chunk")),
        }
    }

    pub fn poke(&mut self) -> io::Result<()> {
        self.ch.send(T_POKE, &[])?;
        self.ch.recv().map(|_| ())
    }
}

// ------------------------------------------------------------------------------------------ server
fn serve_conn(mut s: TcpStream, sh: Arc<Shared>, lan: Arc<Lan>) -> io::Result<()> {
    // a stranger gets a few seconds to prove the sync key, then the usual two minutes
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.set_write_timeout(Some(Duration::from_secs(60)))?;
    s.set_nodelay(true)?;
    let cfg = sh.cfg();
    let key = Key::new(&cfg.sync_key);
    let mut h = [0u8; 20];
    s.read_exact(&mut h)?;
    if &h[..4] != b"DKC1" {
        return Err(bad("not a Deck Claude Sync client"));
    }
    let nc = &h[4..];
    let ns = random::<16>();
    let mut r = ns.to_vec();
    r.extend_from_slice(&key.mac("auth-s", &[nc, &ns]));
    s.write_all(&r)?;
    let mut m = [0u8; 32];
    s.read_exact(&mut m)?;
    if !ct_eq(&m, &key.mac("auth-c", &[nc, &ns])) {
        return Err(bad("client has a different sync key"));
    }
    s.set_read_timeout(Some(Duration::from_secs(120)))?;
    let (tx, rx) = session(&key, nc, &ns, false);
    let mut ch = Chan { s, tx, rx, ntx: 0, nrx: 0 };
    let (t, b) = ch.recv()?;
    if t != T_HELLO {
        return Err(bad("handshake"));
    }
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap_or_default();
    let peer_name = v["name"].as_str().unwrap_or("?").to_string();
    ch.send(T_HELLO, serde_json::json!({"id": cfg.machine_id, "name": cfg.machine_name}).to_string().as_bytes())?;
    // the last file read, since a peer asks for a big file's chunks one after another
    let mut cache: Option<(PathBuf, String, Vec<u8>)> = None;
    loop {
        let (t, body) = match ch.recv() {
            Ok(x) => x,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        match t {
            T_GET_MANIFEST => {
                let snap = lan.serve.read().unwrap().clone();
                match snap {
                    Some(sn) => ch.send(T_MANIFEST, &sn.manifest)?,
                    None => ch.send(T_NONE, &[])?,
                }
            }
            T_GET_CHUNK => {
                let want = String::from_utf8_lossy(&body).to_string();
                let snap = lan.serve.read().unwrap().clone();
                let data = snap.and_then(|sn| {
                    let (abs, rel, idx, fh) = sn.chunks.get(&want)?.clone();
                    let fresh = !matches!(&cache, Some((p, h, _)) if *p == abs && *h == fh);
                    if fresh {
                        let raw = std::fs::read(&abs).ok()?;
                        let d = crate::ccsync::scan::canonical_bytes(&sn.canon, &rel, raw);
                        cache = Some((abs.clone(), fh.clone(), d));
                    }
                    let d = &cache.as_ref()?.2;
                    let part = d.get(idx * CHUNK..((idx + 1) * CHUNK).min(d.len()))?;
                    (crate::ccsync::crypto::hash(part) == want).then(|| deflate(part))
                });
                match data {
                    Some(d) => ch.send(T_CHUNK, &d)?,
                    None => ch.send(T_NONE, &[])?,
                }
            }
            T_POKE => {
                ch.send(T_OK, &[])?;
                // a session waiting to be closed here would otherwise log this every few seconds
                static LAST: std::sync::Mutex<Option<std::collections::HashMap<String, std::time::Instant>>> = std::sync::Mutex::new(None);
                let quiet = {
                    let mut g = LAST.lock_anyway();
                    let m = g.get_or_insert_with(Default::default);
                    let recent = m.get(&peer_name).map(|t| t.elapsed().as_secs() < 300).unwrap_or(false);
                    if !recent {
                        m.insert(peer_name.clone(), std::time::Instant::now());
                    }
                    recent
                };
                if !quiet {
                    sh.log(format!("{peer_name} has changes, syncing"));
                }
                sh.wake();
            }
            _ => return Err(bad("unknown request")),
        }
    }
}

// ------------------------------------------------------------------------------------------ startup
#[derive(serde::Serialize, serde::Deserialize)]
struct Beacon {
    ccsync: u32,
    id: String,
    name: String,
    port: u16,
    grp: String,
}


/// "A PC with a different code tried to connect" from `who`: worth one log line every 10 minutes,
/// not one per probe (PCs on Tailscale are tried every few seconds).
fn log_once(who: &str, msg: &str) -> bool {
    static SEEN: std::sync::Mutex<Option<std::collections::HashMap<String, std::time::Instant>>> = std::sync::Mutex::new(None);
    if !msg.contains("different") {
        return true;
    }
    let mut g = SEEN.lock_anyway();
    let m = g.get_or_insert_with(std::collections::HashMap::new);
    let fresh = m.get(who).map(|t| t.elapsed() > Duration::from_secs(600)).unwrap_or(true);
    if fresh {
        m.insert(who.to_string(), std::time::Instant::now());
    }
    fresh
}

pub fn start(sh: Arc<Shared>, lan: Arc<Lan>, port_override: Option<u16>) {
    let want = port_override.unwrap_or_else(|| sh.cfg().lan_port);
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, want)).or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)));
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            sh.log(format!("LAN: can't listen: {e}"));
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    *lan.port.lock_anyway() = port;
    sh.status(|s| s.lan_port = port);
    {
        let (sh, lan) = (sh.clone(), lan.clone());
        std::thread::spawn(move || {
            static CONNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            for c in listener.incoming().flatten() {
                if sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                let Some(slot) = crate::util::Slot::take(&CONNS, MAX_CONNS) else { continue };
                let (sh, lan) = (sh.clone(), lan.clone());
                std::thread::spawn(move || {
                    let _slot = slot;
                    let who = c.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                    if let Err(e) = serve_conn(c, sh.clone(), lan) {
                        if e.kind() != io::ErrorKind::UnexpectedEof && e.kind() != io::ErrorKind::ConnectionReset && log_once(&who, &e.to_string()) {
                            sh.log(format!("LAN: {who}: {e}"));
                        }
                    }
                });
            }
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
            sh.log(format!("LAN discovery off (UDP {UDP_PORT}: {e}); add the other PC under manual peers"));
            return;
        }
    };
    let beacon = {
        let sh = sh.clone();
        move || {
            let c = sh.cfg();
            serde_json::to_vec(&Beacon { ccsync: 1, id: c.machine_id, name: c.machine_name, port, grp: Key::new(&c.sync_key).group() }).unwrap()
        }
    };
    // sender
    {
        let (sh, udp, beacon) = (sh.clone(), udp.try_clone().expect("udp clone"), beacon.clone());
        std::thread::spawn(move || loop {
            if sh.cfg().lan_enabled && !sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = udp.send_to(&beacon(), (Ipv4Addr::BROADCAST, UDP_PORT));
            }
            std::thread::sleep(Duration::from_secs(5));
        });
    }
    // listener
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        loop {
            let Ok((n, from)) = udp.recv_from(&mut buf) else { continue };
            let Ok(b) = serde_json::from_slice::<Beacon>(&buf[..n]) else { continue };
            let c = sh.cfg();
            if b.ccsync != 1 || b.id == c.machine_id || !c.lan_enabled || sh.off.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            if b.grp != Key::new(&c.sync_key).group() {
                continue;
            }
            let addr = SocketAddr::new(from.ip(), b.port);
            let new = {
                let mut p = lan.peers.lock_anyway();
                let new = p.get(&b.id).map(|o| o.addr != addr || o.seen.elapsed() > Duration::from_secs(60)).unwrap_or(true);
                p.insert(b.id.clone(), Peer { id: b.id.clone(), name: b.name.clone(), addr, seen: Instant::now() });
                new
            };
            if new {
                sh.log(format!("Found {} on the network ({})", b.name, addr));
                // answer directly too, in case broadcasts only get through one way
                let _ = udp.send_to(&beacon(), SocketAddr::new(from.ip(), UDP_PORT));
                sh.wake();
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
