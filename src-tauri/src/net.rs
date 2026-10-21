//! Network: addresses, ping to a few hosts, and a speed test (Cloudflare).

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::net::{Ipv4Addr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};

#[derive(Serialize, Clone, Default)]
pub struct Speed {
    /// "idle" | "latency" | "download" | "upload" | "done" | "error"
    pub phase: String,
    pub down_mbps: Option<f64>,
    pub up_mbps: Option<f64>,
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub progress: f32,
    pub error: Option<String>,
    pub at: u64,
}

pub struct Net {
    pub speed: Mutex<Speed>,
    running: AtomicBool,
    cancel: Arc<AtomicBool>,
    public_ip: Mutex<Option<(Instant, String)>>,
}

pub fn lan_ip() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.0.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

fn resolve_v4(host: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    (host, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        std::net::IpAddr::V4(v4) => Some(v4),
        _ => None,
    })
}

/// One ICMP echo. Returns round trip ms.
pub fn ping(host: &str, timeout_ms: u32) -> Option<u32> {
    let ip = resolve_v4(host)?;
    unsafe {
        let h = IcmpCreateFile().ok()?;
        let data = [0x44u8; 32];
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 16];
        let n = IcmpSendEcho(
            h,
            u32::from_ne_bytes(ip.octets()),
            data.as_ptr() as *const _,
            data.len() as u16,
            None,
            reply.as_mut_ptr() as *mut _,
            reply.len() as u32,
            timeout_ms,
        );
        let _ = IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r: ICMP_ECHO_REPLY = std::ptr::read_unaligned(reply.as_ptr() as *const _);
        (r.Status == 0).then_some(r.RoundTripTime)
    }
}

/// Pings each host a few times: [{host, ms (avg), loss (0..1), min, max}]
pub fn ping_many(hosts: &[String]) -> Vec<Value> {
    let handles: Vec<_> = hosts
        .iter()
        .cloned()
        .map(|h| {
            std::thread::spawn(move || {
                let mut got = Vec::new();
                for _ in 0..4 {
                    if let Some(ms) = ping(&h, 1000) {
                        got.push(ms);
                    }
                    std::thread::sleep(Duration::from_millis(120));
                }
                let avg = (!got.is_empty()).then(|| got.iter().sum::<u32>() as f64 / got.len() as f64);
                json!({
                    "host": h,
                    "ms": avg,
                    "min": got.iter().min(),
                    "max": got.iter().max(),
                    "loss": 1.0 - got.len() as f64 / 4.0,
                })
            })
        })
        .collect();
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

const CANCELLED: &str = "cancelled";

/// Upload body that fails as soon as the test is cancelled.
struct CancelReader<'a> {
    inner: std::io::Cursor<&'a [u8]>,
    stop: Arc<AtomicBool>,
}

impl Read for CancelReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.stop.load(Ordering::SeqCst) {
            return Err(std::io::Error::other(CANCELLED));
        }
        let n = buf.len().min(64 * 1024);
        self.inner.read(&mut buf[..n])
    }
}

fn agent() -> ureq::Agent {
    crate::log::http(ureq::AgentBuilder::new()).timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build()
}

impl Net {
    pub fn new() -> Self {
        Self { speed: Mutex::new(Speed { phase: "idle".into(), ..Default::default() }), running: AtomicBool::new(false), cancel: Arc::new(AtomicBool::new(false)), public_ip: Mutex::new(None) }
    }

    pub fn info(&self) -> Value {
        json!({
            "lan": lan_ip(),
            "public": self.public_ip.lock().as_ref().map(|x| x.1.clone()),
            "speed": *self.speed.lock(),
            "host": std::env::var("COMPUTERNAME").ok(),
        })
    }

    pub fn cancel_speed_test(&self) {
        if self.running.load(Ordering::SeqCst) {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub fn fetch_public_ip(&self) -> Option<String> {
        if let Some((t, ip)) = self.public_ip.lock().as_ref() {
            if t.elapsed().as_secs() < 600 {
                return Some(ip.clone());
            }
        }
        let ip = agent().get("https://api.ipify.org").call().ok()?.into_string().ok()?.trim().to_string();
        *self.public_ip.lock() = Some((Instant::now(), ip.clone()));
        Some(ip)
    }

    /// Latency, download and upload against speed.cloudflare.com.
    pub fn speed_test(self: &Arc<Self>, app: Arc<crate::app::App>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        let me = self.clone();
        std::thread::spawn(move || {
            let prev = me.speed.lock().clone();
            let stop = me.cancel.clone();
            let cancelled = || stop.load(Ordering::SeqCst);
            let set = |f: &dyn Fn(&mut Speed)| {
                f(&mut me.speed.lock());
                app.changed("net");
            };
            set(&|s| *s = Speed { phase: "latency".into(), ..Default::default() });
            let a = agent();
            let r = (|| -> Result<(), String> {
                // latency: small requests
                let mut lat = Vec::new();
                for _ in 0..8 {
                    if cancelled() {
                        return Err(CANCELLED.into());
                    }
                    let t = Instant::now();
                    a.get("https://speed.cloudflare.com/__down?bytes=0").call().map_err(|e| e.to_string())?;
                    lat.push(t.elapsed().as_secs_f64() * 1000.0);
                }
                lat.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let ping = lat[lat.len() / 2];
                let jitter = lat.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (lat.len() - 1) as f64;
                set(&|s| {
                    s.ping_ms = Some(ping);
                    s.jitter_ms = Some(jitter);
                    s.phase = "download".into();
                });
                // download: stream up to 10 s
                let t = Instant::now();
                let mut total = 0u64;
                let mut buf = vec![0u8; 256 * 1024];
                'outer: for size in [10_000_000u64, 25_000_000, 50_000_000, 100_000_000] {
                    let resp = a.get(&format!("https://speed.cloudflare.com/__down?bytes={size}")).call().map_err(|e| e.to_string())?;
                    let mut rd = resp.into_reader();
                    loop {
                        if cancelled() {
                            return Err(CANCELLED.into());
                        }
                        let n = rd.read(&mut buf).map_err(|e| e.to_string())?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        let el = t.elapsed().as_secs_f64();
                        if total % (4 * 1024 * 1024) < n as u64 {
                            let mbps = total as f64 * 8.0 / el / 1e6;
                            set(&|s| {
                                s.down_mbps = Some(mbps);
                                s.progress = (el / 10.0).min(1.0) as f32 * 0.5;
                            });
                        }
                        if el > 10.0 {
                            break 'outer;
                        }
                    }
                }
                let down = total as f64 * 8.0 / t.elapsed().as_secs_f64() / 1e6;
                set(&|s| {
                    s.down_mbps = Some(down);
                    s.phase = "upload".into();
                    s.progress = 0.5;
                });
                // upload: a few POSTs, up to ~8 s
                let chunk = vec![0x5au8; 5_000_000];
                let t = Instant::now();
                let mut sent = 0u64;
                while t.elapsed().as_secs_f64() < 8.0 && sent < 100_000_000 {
                    // The reader stops the upload mid-request when cancelled.
                    let body = CancelReader { inner: std::io::Cursor::new(&chunk[..]), stop: stop.clone() };
                    let r = a.post("https://speed.cloudflare.com/__up").set("Content-Length", &chunk.len().to_string()).send(body);
                    if cancelled() {
                        return Err(CANCELLED.into());
                    }
                    r.map_err(|e| e.to_string())?;
                    sent += chunk.len() as u64;
                    let up = sent as f64 * 8.0 / t.elapsed().as_secs_f64() / 1e6;
                    let p = 0.5 + (t.elapsed().as_secs_f64() / 8.0).min(1.0) as f32 * 0.5;
                    set(&|s| {
                        s.up_mbps = Some(up);
                        s.progress = p;
                    });
                }
                Ok(())
            })();
            if matches!(&r, Err(e) if e == CANCELLED) {
                // Put back the last finished result.
                set(&|s| *s = prev.clone());
                me.running.store(false, Ordering::SeqCst);
                return;
            }
            set(&|s| {
                match &r {
                    Ok(_) => s.phase = "done".into(),
                    Err(e) => {
                        s.phase = "error".into();
                        s.error = Some(format!("Speed test failed: {e}"));
                    }
                }
                s.progress = 1.0;
                s.at = crate::util::now_ms();
            });
            me.running.store(false, Ordering::SeqCst);
        });
    }
}
