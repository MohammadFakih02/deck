//! Phone remote: a web app on the local network (installable to the Android
//! app drawer) that controls media, volume, power and passes text, links
//! and files between the phone and the PC.

use crate::app::{App, Shared};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path as AxPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const PHONE_HTML: &str = include_str!("../phone/index.html");
/// The phone page's H.264 decoder (a Web Worker with tinyh264); the name changes with its content,
/// so phones may keep it a long time.
const H264_DEC: &str = include_str!("../phone/h264dec.js");
const ICON_192: &[u8] = include_bytes!("../icons/phone-192.png");
const ICON_512: &[u8] = include_bytes!("../icons/phone-512.png");
const ICON_MASKABLE: &[u8] = include_bytes!("../icons/phone-maskable.png");

/// Chrome only offers "Install app" for pages with a service worker.
const SERVICE_WORKER: &str = "self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', (e) => e.respondWith(fetch(e.request)));
";

pub struct Remote {
    pub port: Mutex<Option<u16>>,
    pub error: Mutex<Option<String>>,
    server: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl Remote {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { port: Mutex::new(None), error: Mutex::new(None), server: Mutex::new(None) })
    }
}

pub fn qr_svg(text: &str) -> String {
    match qrcode::QrCode::new(text.as_bytes()) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(220, 220)
            .dark_color(qrcode::render::svg::Color("#0c0e14"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}

pub fn info(app: &App, remote: &Remote) -> Value {
    let (token, on) = {
        let s = app.settings.lock();
        (s.remote_token.clone(), s.remote_on)
    };
    let port = *remote.port.lock();
    let lan = match (crate::net::lan_ip(), port) {
        (Some(ip), Some(p)) => Some(format!("http://{ip}:{p}/#k={token}")),
        _ => None,
    };
    let origin = match (crate::net::lan_ip(), port) {
        (Some(ip), Some(p)) => Some(format!("http://{ip}:{p}")),
        _ => None,
    };
    // the same page from anywhere, over Tailscale (a private network between the user's devices)
    let away = match (crate::rdesk::tailscale_ip(), port) {
        (Some(ip), Some(p)) => Some(format!("http://{ip}:{p}/#k={token}")),
        _ => None,
    };
    let phones = devices();
    json!({
        "enabled": on,
        "away": away,
        "away_qr": away.as_deref().map(qr_svg),
        "away_peers": crate::rdesk::tailscale(false)["peers"].clone(),
        "away_account": crate::rdesk::tailscale(false)["account"].clone(),
        // Deck's own "get Tailscale" page on the home network (store links plus the direct
        // download, for countries where the Play Store hides the app); Tailscale's site without it
        "away_get_qr": away.as_ref().map(|_| qr_svg(&match (crate::net::lan_ip(), port) {
            (Some(ip), Some(p)) => format!("http://{ip}:{p}/tailscale"),
            _ => "https://tailscale.com/download".into(),
        })),
        "lan": lan,
        "origin": origin,
        "qr": lan.as_deref().map(qr_svg),
        "port": port,
        "error": *remote.error.lock(),
        // phones (one per address, however many pages or views each has open), at home and online
        "clients": phones.0 + phones.1,
        "local": phones.0,
        "online": phones.1,
        "screen_viewers": app.screen_viewers.load(Ordering::Relaxed),
        "shared": *app.shared.lock(),
    })
}

type St = Arc<App>;

const FIREWALL_RULE: &str = "name=Deck phone remote";

/// Windows Firewall blocks incoming connections unless a rule allows them, and an elevated Deck
/// started by its logon task never gets Windows' "Allow access" prompt, so the phone's requests
/// just time out. With admin rights Deck adds its own rule: this exe, TCP, private and domain
/// networks (not public ones, such as café Wi-Fi). Without admin rights Windows asks as usual.
fn allow_through_firewall() {
    allow_program(FIREWALL_RULE, "TCP");
}

/// Transfer and Claude Sync between your PCs: their TCP channels and the UDP beacons the PCs find
/// each other by (home and work networks only, like the phone's rule).
pub fn allow_between_pcs() {
    if std::env::var("DECK_PROFILE").map(|p| p != "test").unwrap_or(true) {
        allow_program("name=Deck between your PCs", "any");
    }
}

/// An inbound rule `rule` ("name=…") for this exe and `protocol` (needs admin; Deck usually has it).
fn allow_program(rule: &str, protocol: &str) {
    if !crate::elevation::is_admin() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.display().to_string();
    let show = crate::util::run_hidden("netsh.exe", &["advfirewall", "firewall", "show", "rule", rule, "verbose"]);
    // already there for this exe (an update to another folder needs a new one)
    if show.map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&exe.to_lowercase())).unwrap_or(false) {
        return;
    }
    let _ = crate::util::run_hidden("netsh.exe", &["advfirewall", "firewall", "delete", "rule", rule]);
    let prog = format!("program={exe}");
    let _ = crate::util::run_hidden("netsh.exe", &["advfirewall", "firewall", "add", "rule", rule, "dir=in", "action=allow", &prog, &format!("protocol={protocol}"), "profile=private,domain", "enable=yes"]);
}

pub fn start(app: Arc<App>, remote: Arc<Remote>) {
    stop(&remote);
    if std::env::var("DECK_PROFILE").map(|p| p != "test").unwrap_or(true) {
        std::thread::spawn(allow_through_firewall);
    }
    let preferred = app.settings.lock().remote_port;
    let r2 = remote.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut listener = None;
        // Test runs stay on this PC (no firewall prompt, nothing on the network).
        let ip = if std::env::var("DECK_PROFILE").map(|p| p == "test").unwrap_or(false) { [127, 0, 0, 1] } else { [0, 0, 0, 0] };
        for port in [preferred, preferred + 1, preferred + 2, 0] {
            if let Ok(l) = tokio::net::TcpListener::bind(SocketAddr::from((ip, port))).await {
                listener = Some(l);
                break;
            }
        }
        let Some(listener) = listener else {
            *r2.error.lock() = Some("Couldn't open a network port for the phone".into());
            app.changed("remote");
            return;
        };
        *r2.port.lock() = listener.local_addr().map(|a| a.port()).ok();
        *r2.error.lock() = None;
        app.changed("remote");
        let router = Router::new()
            .route("/", get(page))
            .route("/ws", get(ws))
            .route("/screen", get(screen_ws))
            .route("/audio", get(audio_ws))
            .route("/check", get(check))
            .route("/upload", post(upload).get(upload_have))
            .route("/file/{id}", get(file))
            .route("/icon.png", get(|| png(ICON_192)))
            .route("/icon-512.png", get(|| png(ICON_512)))
            .route("/icon-maskable.png", get(|| png(ICON_MASKABLE)))
            .route("/sw.js", get(service_worker))
            .route("/art", get(art))
            .route("/h264dec-2.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript"), (header::CACHE_CONTROL, "max-age=31536000, immutable")], H264_DEC) }))
            .route("/tailscale", get(tailscale_page))
            .route("/manifest.webmanifest", get(manifest))
            .with_state(app.clone());
        // no Nagle: the Screen view's pictures and the phone's answers go out at once (the last
        // part of each picture otherwise waits for the phone's delayed acknowledgement)
        use axum::serve::ListenerExt;
        let listener = listener.tap_io(|tcp| {
            let _ = tcp.set_nodelay(true);
        });
        // the phone's address: home or online, and one count per phone
        if let Err(e) = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await {
            *r2.error.lock() = Some(format!("Phone remote stopped: {e}"));
        }
    });
    *remote.server.lock() = Some(handle);
}

pub fn stop(remote: &Remote) {
    if let Some(h) = remote.server.lock().take() {
        h.abort();
    }
    *remote.port.lock() = None;
}

async fn page() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Html(PHONE_HTML))
}

async fn png(bytes: &'static [u8]) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "max-age=86400")], bytes)
}

/// The newest universal Android APK on Tailscale's own download server (read once per hour).
fn tailscale_apk() -> Option<String> {
    static CACHE: Mutex<Option<(std::time::Instant, Option<String>)>> = Mutex::new(None);
    let mut c = CACHE.lock();
    if let Some((t, v)) = c.as_ref() {
        if t.elapsed() < Duration::from_secs(3600) {
            return v.clone();
        }
    }
    let page = ureq::get("https://pkgs.tailscale.com/stable/").timeout(Duration::from_secs(8)).call().ok().and_then(|r| r.into_string().ok());
    let apk = page.and_then(|p| {
        let i = p.find("tailscale-android-universal-")?;
        let end = i + p[i..].find(".apk")? + 4;
        Some(format!("https://pkgs.tailscale.com/stable/{}", &p[i..end]))
    });
    *c = Some((std::time::Instant::now(), apk.clone()));
    apk
}

/// "Get Tailscale" for the phone, opened from the setup code on the Phone page's Away tab: the
/// App Store on iPhones; on Android the Play Store and the direct download from Tailscale (the
/// Play Store doesn't offer Tailscale in every country).
async fn tailscale_page() -> impl IntoResponse {
    let apk = tauri::async_runtime::spawn_blocking(tailscale_apk).await.ok().flatten().unwrap_or_else(|| "https://pkgs.tailscale.com/stable/#android".into());
    let html = format!(r##"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Get Tailscale</title><style>
body{{margin:0;font:16px/1.5 system-ui,sans-serif;background:#0c0e14;color:#e8eaf0;padding:24px 18px}}
h1{{font-size:22px;margin:0 0 6px}}p{{color:#9aa3b5;margin:0 0 18px}}
a.b{{display:block;text-align:center;padding:14px;border-radius:12px;margin:0 0 12px;text-decoration:none;font-weight:600;background:#1b2030;color:#e8eaf0;border:1px solid #2a3145}}
a.b.p{{background:#4f8cff;border-color:#4f8cff;color:#fff}}small{{display:block;color:#7c8598;margin:-4px 0 18px}}.ios,.android{{display:none}}
</style></head><body>
<h1>Get Tailscale</h1><p>Tailscale connects this phone to your PC from anywhere. After installing, sign in with the same account as the PC and turn it on.</p>
<div class="android"><a class="b p" href="https://play.google.com/store/apps/details?id=com.tailscale.ipn">Play Store</a>
<a class="b" href="{apk}">Download the app directly</a><small>If the Play Store says it's not available in your country. From Tailscale's own site; allow your browser to install apps when asked. It doesn't update by itself: get the new one here now and then.</small></div>
<div class="ios"><a class="b p" href="https://apps.apple.com/app/tailscale/id1470499037">App Store</a></div>
<script>var a=/android/i.test(navigator.userAgent),i=/iphone|ipad|ipod/i.test(navigator.userAgent)||(navigator.platform==="MacIntel"&&navigator.maxTouchPoints>1);
document.querySelectorAll(a?".android":i?".ios":".android,.ios").forEach(function(e){{e.style.display="block"}});</script>
</body></html>"##);
    ([(header::CACHE_CONTROL, "no-store")], Html(html))
}

async fn service_worker() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript"), (header::CACHE_CONTROL, "no-cache")], SERVICE_WORKER)
}

async fn manifest() -> impl IntoResponse {
    let m = json!({
        "id": "/",
        "name": "Deck Remote",
        "short_name": "Deck",
        "description": "Control your PC: media, volume, power, and send files and text",
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "orientation": "portrait",
        "background_color": "#0c0e14",
        "theme_color": "#0c0e14",
        "icons": [
            { "src": "/icon.png", "sizes": "192x192", "type": "image/png", "purpose": "any" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" },
            { "src": "/icon-maskable.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
        ]
    });
    ([(header::CONTENT_TYPE, "application/manifest+json")], m.to_string())
}

#[derive(Deserialize)]
struct Key {
    k: Option<String>,
    name: Option<String>,
    /// resumable upload: the phone's id for the file (name, size and date), its size, and where this piece starts
    id: Option<String>,
    size: Option<u64>,
    off: Option<u64>,
}

// ------------------------------------------------------------------ who is connected
/// One open phone connection (its page, a Screen view or the Screen view's sound).
struct Conn {
    id: u64,
    ip: IpAddr,
}
static CONNS: Mutex<Vec<Conn>> = parking_lot::const_mutex(Vec::new());
static NEXT_CONN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Away from home: a Tailscale address, or any address that isn't this network.
pub fn is_online(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v) => v.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v)),
        v4 => v4,
    };
    if crate::rdesk::is_tailscale(ip) {
        return true;
    }
    match ip {
        IpAddr::V4(v) => !(v.is_private() || v.is_loopback() || v.is_link_local()),
        IpAddr::V6(v) => !(v.is_loopback() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80),
    }
}

/// Phones connected now, (at home, online): one per address.
fn count(list: &[Conn]) -> (usize, usize) {
    let mut seen = std::collections::HashSet::new();
    let (mut home, mut online) = (0, 0);
    for c in list {
        if seen.insert(c.ip) {
            if is_online(c.ip) {
                online += 1;
            } else {
                home += 1;
            }
        }
    }
    (home, online)
}

pub fn devices() -> (usize, usize) {
    count(&CONNS.lock())
}

/// The phone limits from the settings.
#[derive(Clone, Copy, Default)]
struct Limits {
    max_home: u32,
    max_away: u32,
    home_blocks_away: bool,
    away_blocks_home: bool,
}

fn limits(app: &App) -> Limits {
    let s = app.settings.lock();
    Limits { max_home: s.remote_max_local, max_away: s.remote_max_online, home_blocks_away: s.remote_local_blocks_online, away_blocks_home: s.remote_online_blocks_local }
}

/// The rule on its own: may a phone at `ip` connect, with `list` connected now? The error is what
/// the phone shows. A phone already connected (another page, its Screen view or sound) always may.
fn decide(list: &[Conn], ip: IpAddr, l: Limits) -> Result<(), String> {
    if list.iter().any(|c| c.ip == ip) {
        return Ok(());
    }
    let (home, online) = count(list);
    let away = is_online(ip);
    if away && l.home_blocks_away && home > 0 {
        return Err("A phone at home is using this PC. Connections from away are off while it is.".into());
    }
    if !away && l.away_blocks_home && online > 0 {
        return Err("A phone away from home is using this PC. Connections at home are off while it is.".into());
    }
    let (max, n, place) = if away { (l.max_away, online, "away from home") } else { (l.max_home, home, "at home") };
    if max > 0 && n as u32 >= max {
        return Err(format!("This PC allows {max} phone{} {place} at a time, and that many are connected.", if max == 1 { "" } else { "s" }));
    }
    Ok(())
}

/// Checks and registers in one step, so two phones connecting at the same moment can't both take
/// the last place. The guard keeps the connection counted until it is dropped.
fn admit(app: &App, ip: IpAddr) -> Result<Registered, String> {
    let l = limits(app);
    let mut list = CONNS.lock();
    decide(&list, ip, l)?;
    let id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    list.push(Conn { id, ip });
    Ok(Registered(id))
}

/// For plain requests (uploads, files): a phone that is turned away can't use them either.
fn allowed(app: &App, ip: IpAddr) -> Result<(), String> {
    let l = limits(app);
    decide(&CONNS.lock(), ip, l)
}

/// Keeps a connection in the list while it is open.
struct Registered(u64);
impl Drop for Registered {
    fn drop(&mut self) {
        CONNS.lock().retain(|c| c.id != self.0);
    }
}

/// A browser can't read an HTTP refusal of a WebSocket: open it, say why, close it properly.
fn refuse(up: WebSocketUpgrade, why: String) -> Response {
    up.on_upgrade(move |mut socket| async move {
        let _ = socket.send(Message::Text(json!({ "t": "refused", "text": why }).to_string().into())).await;
        let _ = socket.send(Message::Close(None)).await;
    })
}

fn authorized(app: &App, k: &Option<String>) -> bool {
    let t = app.settings.lock().remote_token.clone();
    k.as_deref() == Some(t.as_str())
}

async fn check(State(app): State<St>, Query(q): Query<Key>) -> StatusCode {
    if authorized(&app, &q.k) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::UNAUTHORIZED
    }
}

/// Where files from the phone land.
pub fn inbox_dir() -> PathBuf {
    crate::util::downloads().join("From phone")
}

fn safe_name(n: &str) -> String {
    let cleaned: String = n.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let t = cleaned.trim().trim_matches('.').to_string();
    if t.is_empty() {
        "file".into()
    } else {
        t.chars().take(150).collect()
    }
}

fn unique(dir: &std::path::Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let path = std::path::Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..10000).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap_or(p)
}

/// Uploads stop when the drive would have less than this left.
const KEEP_FREE: u64 = 2 << 30;

/// Where a resumable upload collects its pieces (in the inbox folder, hidden by its name).
fn upload_part(id: &str) -> Option<PathBuf> {
    let ok = !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| inbox_dir().join(format!(".deck-upload-{id}.part")))
}

/// How much of a resumable upload the PC already has (the phone asks before sending, and again
/// after a dropped connection, so nothing is sent twice). Parts nobody came back for in 3 days go.
async fn upload_have(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Query(q): Query<Key>) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    if let Err(why) = allowed(&app, addr.ip()) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    if let Ok(rd) = std::fs::read_dir(inbox_dir()) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d > Duration::from_secs(3 * 86400));
            if old && e.file_name().to_string_lossy().starts_with(".deck-upload-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let have = q.id.as_deref().and_then(upload_part).and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
    axum::Json(json!({ "have": have })).into_response()
}

/// One piece of a resumable upload: written only when all of it arrived (a piece cut off by a
/// dropped connection is left out and sent again), and only where the part file ends.
async fn upload_piece(app: &App, q: &Key, id: &str, body: Body) -> Response {
    let Some(part) = upload_part(id) else { return (StatusCode::BAD_REQUEST, "bad id").into_response() };
    let (size, off) = (q.size.unwrap_or(0), q.off.unwrap_or(0));
    let dir = inbox_dir();
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't create folder").into_response();
    }
    let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    if off != have {
        return (StatusCode::CONFLICT, axum::Json(json!({ "have": have }))).into_response();
    }
    if crate::util::free_bytes(&dir).is_some_and(|free| free < KEEP_FREE + size.saturating_sub(have)) {
        return (StatusCode::INSUFFICIENT_STORAGE, "The PC's drive is almost full").into_response();
    }
    let Ok(bytes) = axum::body::to_bytes(body, 64 << 20).await else {
        return (StatusCode::BAD_REQUEST, axum::Json(json!({ "have": have }))).into_response();
    };
    if have + bytes.len() as u64 > size {
        return (StatusCode::BAD_REQUEST, "more than the file's size").into_response();
    }
    let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open(&part).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't write").into_response();
    };
    if f.write_all(&bytes).await.is_err() || f.flush().await.is_err() {
        drop(f);
        // whatever of this piece got written is cut off again: the phone resends the whole piece
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&part) {
            let _ = f.set_len(have);
        }
        return (StatusCode::INTERNAL_SERVER_ERROR, "disk full?").into_response();
    }
    drop(f);
    let have = have + bytes.len() as u64;
    if have < size {
        return axum::Json(json!({ "have": have })).into_response();
    }
    let name = safe_name(q.name.as_deref().unwrap_or("file"));
    let path = unique(&dir, &name);
    if tokio::fs::rename(&part, &path).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't save").into_response();
    }
    received(app, &path, size);
    axum::Json(json!({ "have": have, "done": true })).into_response()
}

/// A file from the phone is in: the list and a notification.
fn received(app: &App, path: &std::path::Path, size: u64) {
    let p = path.to_string_lossy().to_string();
    let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    push_shared(app, Shared { id: 0, kind: "file".into(), text: String::new(), path: Some(p.clone()), name: fname.clone(), size, ts: crate::util::now_ms(), from: "phone".into() });
    app.toast(crate::app::Toast {
        id: format!("recv:{p}"),
        kind: "file".into(),
        title: "File from your phone".into(),
        body: format!("{fname} · {}", crate::util::fmt_bytes(size)),
        actions: vec![("show".into(), "Show in folder".into()), ("open".into(), "Open".into())],
        sticky: false,
        sound: false,
    });
}

async fn upload(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Query(q): Query<Key>, body: Body) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    if let Err(why) = allowed(&app, addr.ip()) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    // the phone page sends in pieces it can continue; an older page still sends the whole file
    if let Some(id) = q.id.clone() {
        return upload_piece(&app, &q, &id, body).await;
    }
    let dir = inbox_dir();
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't create folder").into_response();
    }
    let name = safe_name(q.name.as_deref().unwrap_or("file"));
    let path = unique(&dir, &name);
    let part = path.with_extension(format!("{}.part", path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()));
    let Ok(mut f) = tokio::fs::File::create(&part).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't write").into_response();
    };
    let mut stream = body.into_data_stream();
    let mut size = 0u64;
    let mut checked = 0u64;
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            let _ = tokio::fs::remove_file(&part).await;
            return (StatusCode::BAD_REQUEST, "upload interrupted").into_response();
        };
        size += chunk.len() as u64;
        // never fill the drive: stop (and clean up) while there is still room for Windows
        if size - checked >= 32 << 20 || checked == 0 {
            checked = size;
            if crate::util::free_bytes(&dir).is_some_and(|free| free < KEEP_FREE + chunk.len() as u64) {
                drop(f);
                let _ = tokio::fs::remove_file(&part).await;
                return (StatusCode::INSUFFICIENT_STORAGE, "The PC's drive is almost full").into_response();
            }
        }
        if f.write_all(&chunk).await.is_err() {
            let _ = tokio::fs::remove_file(&part).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, "disk full?").into_response();
        }
    }
    let _ = f.flush().await;
    drop(f);
    if tokio::fs::rename(&part, &path).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't save").into_response();
    }
    received(&app, &path, size);
    (StatusCode::OK, "ok").into_response()
}

pub fn push_shared(app: &App, mut s: Shared) {
    let mut v = app.shared.lock();
    s.id = v.iter().map(|x| x.id).max().unwrap_or(0) + 1;
    v.push(s);
    if v.len() > 60 {
        v.remove(0);
    }
    drop(v);
    app.changed("shared");
}

async fn file(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, AxPath(id): AxPath<u64>, Query(q): Query<Key>, headers: axum::http::HeaderMap) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    if let Err(why) = allowed(&app, addr.ip()) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    let item = app.shared.lock().iter().find(|s| s.id == id).cloned();
    let Some(path) = item.and_then(|s| s.path) else { return (StatusCode::NOT_FOUND, "gone").into_response() };
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    serve_file(&path, &name, &headers, None).await
}

/// A file as a download that a browser can continue after a dropped connection (byte ranges).
/// `on_end` runs once the file's last byte has gone out (a whole download, or the end of a continued one).
pub(crate) async fn serve_file(path: &str, name: &str, headers: &axum::http::HeaderMap, on_end: Option<Box<dyn FnOnce() + Send>>) -> Response {
    use tokio::io::AsyncSeekExt;
    let Ok(mut f) = tokio::fs::File::open(path).await else { return (StatusCode::NOT_FOUND, "gone").into_response() };
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
    // "bytes=START-" or "bytes=START-END" (one range; anything else gets the whole file)
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|r| r.strip_prefix("bytes=")).and_then(|r| {
        let (a, b) = r.split_once('-')?;
        let start: u64 = a.trim().parse().ok()?;
        let end: u64 = if b.trim().is_empty() { len.checked_sub(1)? } else { b.trim().parse::<u64>().ok()?.min(len.checked_sub(1)?) };
        (start <= end).then_some((start, end))
    });
    if let Some(r) = headers.get(header::RANGE) {
        if range.is_none() && r.to_str().map(|s| s.starts_with("bytes=")).unwrap_or(false) && len > 0 {
            return (StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{len}"))]).into_response();
        }
    }
    let (start, end) = range.unwrap_or((0, len.saturating_sub(1)));
    let count = if len == 0 { 0 } else { end - start + 1 };
    if start > 0 && f.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "can't read").into_response();
    }
    use tokio::io::AsyncReadExt;
    let reader = f.take(count);
    // the last byte going out is what counts as "downloaded"
    let reaches_end = len == 0 || end + 1 == len;
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    struct End(Option<Box<dyn FnOnce() + Send>>, std::sync::Arc<std::sync::atomic::AtomicU64>, u64, bool);
    impl Drop for End {
        fn drop(&mut self) {
            if self.3 && self.1.load(std::sync::atomic::Ordering::Relaxed) >= self.2 {
                if let Some(f) = self.0.take() {
                    f();
                }
            }
        }
    }
    let guard = End(on_end, sent.clone(), count, reaches_end);
    let stream = tokio_util::io::ReaderStream::with_capacity(reader, 256 << 10).map(move |c| {
        let _keep = &guard;
        if let Ok(b) = &c {
            sent.fetch_add(b.len() as u64, std::sync::atomic::Ordering::Relaxed);
        }
        c
    });
    let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' && !c.is_control() { c } else { '_' }).collect();
    let mut resp = (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_LENGTH, count.to_string()),
            (header::ACCEPT_RANGES, "bytes".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", pct(name))),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        Body::from_stream(stream),
    )
        .into_response();
    if range.is_some() {
        *resp.status_mut() = StatusCode::PARTIAL_CONTENT;
        if let Ok(v) = format!("bytes {start}-{end}/{len}").parse() {
            resp.headers_mut().insert(header::CONTENT_RANGE, v);
        }
    }
    resp
}

pub(crate) fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

async fn ws(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Query(q): Query<Key>, up: WebSocketUpgrade) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    let reg = match admit(&app, addr.ip()) {
        Ok(r) => r,
        Err(why) => return refuse(up, why),
    };
    up.on_upgrade(move |socket| client(socket, app, reg))
}

/// Everything the phone shows (gathered on a blocking thread: COM calls).
/// The playing track's artwork, served at /art. The state names it by a hash, so the phone fetches
/// each picture once: inside the state it was ~170 KB every 1.5 s, more than a 1 Mb/s upload away
/// from home (buttons lagged, text didn't get through, the Screen view starved).
static ART: parking_lot::Mutex<(u64, String)> = parking_lot::const_mutex((0, String::new()));

fn art_link(app: &App, media: &mut Value) {
    let Some(a) = media.pointer_mut("/current/art") else { return };
    let Some(s) = a.as_str().filter(|s| s.starts_with("data:")) else { return };
    // FNV-1a: the link changes when the picture does
    let h = s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    {
        let mut g = ART.lock();
        if g.0 != h {
            *g = (h, s.to_string());
        }
    }
    *a = json!(format!("/art?h={h:x}&k={}", pct(&app.settings.lock().remote_token)));
}

async fn art(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Query(q): Query<Key>) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    if let Err(why) = allowed(&app, addr.ip()) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    let s = ART.lock().1.clone();
    let Some((head, b64)) = s.split_once(',') else { return StatusCode::NOT_FOUND.into_response() };
    let mime = head.trim_start_matches("data:").split(';').next().unwrap_or("image/png").to_string();
    let Some(bytes) = crate::util::unbase64(b64) else { return StatusCode::NOT_FOUND.into_response() };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "private, max-age=86400".to_string())], bytes).into_response()
}

fn phone_state(app: &App) -> Value {
    let power = app.power.state();
    let shared: Vec<Shared> = app.shared.lock().clone();
    let snap = app.sys.snapshot();
    let units = {
        let s = app.settings.lock();
        json!({ "clock": s.unit_clock, "net": s.unit_net, "temp": s.unit_temp })
    };
    // watts from the hardware engine (any brand); the GPU falls back to NVIDIA's own reading
    let (cpu_w, gpu_w) = app
        .hw_engine()
        .and_then(|h| {
            let t = h.table.lock().ok()?;
            let w = |grp: &str, name: &str| t.rows.iter().find(|r| r.group.starts_with(grp) && r.name == name && r.unit == "W").and_then(|r| r.cur);
            Some((w("CPU", "Package power"), w("GPU", "Power")))
        })
        .unwrap_or_default();
    // the CPU's package temperature (null until the engine has read it)
    let cpu_t = app.hw_engine().and_then(|h| h.live.lock().ok().map(|l| l.cpu_t)).unwrap_or_default();
    let gpu_w = gpu_w.map(|w| json!(w)).unwrap_or_else(|| snap["gpu"]["power_w"].clone());
    let sys = json!({ "cpu": snap["cpu"], "ram": snap["ram_pct"], "gpu": snap["gpu"], "down": snap["down"], "up": snap["up"], "units": units, "cpu_w": cpu_w, "gpu_w": gpu_w, "cpu_t": cpu_t });
    let profiles = {
        let s = app.settings.lock();
        json!({
            "cpu": s.cpu_profiles.iter().map(|p| json!({ "id": p.id, "name": p.name })).collect::<Vec<_>>(),
            "gpu": s.gpu_profiles.iter().map(|p| json!({ "id": p.id, "name": p.name })).collect::<Vec<_>>(),
            "active_cpu": s.active_cpu, "active_gpu": s.active_gpu,
        })
    };
    let mut media = app.media.state();
    art_link(app, &mut media);
    json!({
        "t": "state",
        "pc": std::env::var("COMPUTERNAME").unwrap_or_default(),
        "media": media,
        "master": crate::audio::master(),
        "outputs": crate::audio::outputs(),
        "apps": crate::audio::apps(),
        "power": power,
        "shared": shared,
        "toasts": *app.toasts.lock(),
        "sys": sys,
        "boost": app.boost.brief(),
        "profiles": profiles,
        "screen": app.settings.lock().remote_screen.clone(),
        // the PC on the home network: a phone that opened the Tailscale address uses it when it can
        "lan": crate::net::lan_ip(),
    })
}

#[derive(Deserialize)]
struct AudioQuery {
    k: Option<String>,
    /// best, good or low (see `screencast::start_sound`)
    q: Option<String>,
}

async fn audio_ws(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Query(q): Query<AudioQuery>, up: WebSocketUpgrade) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    if app.settings.lock().remote_screen == "off" {
        return (StatusCode::FORBIDDEN, "Screen view is off on the PC").into_response();
    }
    let reg = match admit(&app, addr.ip()) {
        Ok(r) => r,
        Err(why) => return (StatusCode::FORBIDDEN, why).into_response(),
    };
    let quality = q.q.unwrap_or_default();
    up.on_upgrade(move |socket| audio_client(socket, app, quality, reg))
}

/// The PC's sound for a phone with the Screen view open and its sound on. Ends when the phone
/// turns it off or leaves, or the PC turns the Screen view off.
async fn audio_client(mut socket: WebSocket, app: Arc<App>, quality: String, _reg: Registered) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::screencast::Out>(64);
    let stop = crate::screencast::start_sound(&quality, tx);
    let mut check = tokio::time::interval(Duration::from_millis(1000));
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            o = rx.recv() => {
                let sent = match o {
                    Some(crate::screencast::Out::Frame(b)) => socket.send(Message::Binary(b.into())).await,
                    Some(crate::screencast::Out::Text(s)) => socket.send(Message::Text(s.into())).await,
                    None => break,
                };
                if sent.is_err() { break; }
            }
            _ = check.tick() => {
                if app.settings.lock().remote_screen == "off" { break; }
            }
        }
    }
    stop.store(true, Ordering::SeqCst);
}

#[derive(serde::Deserialize)]
struct ScreenQuery {
    k: Option<String>,
    /// video the phone can decode: h264w (WebCodecs), h264s (the page's decoder), h264m (Media
    /// Source), or jpeg
    v: Option<String>,
    /// "1": the phone is away (over Tailscale), on the PC's upload: Auto starts low
    away: Option<String>,
    /// "1": the phone answers {"t":"pad"} at once, so Auto can measure the line
    pad: Option<String>,
}

async fn screen_ws(State(app): State<St>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Query(q): Query<ScreenQuery>, up: WebSocketUpgrade) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    if app.settings.lock().remote_screen == "off" {
        return (StatusCode::FORBIDDEN, "Screen view is off on the PC").into_response();
    }
    let reg = match admit(&app, addr.ip()) {
        Ok(r) => r,
        Err(why) => return refuse(up, why),
    };
    let video = if app.settings.lock().remote_screen_codec == "jpeg" { crate::screencast::Video::Jpeg } else { crate::screencast::Video::parse(q.v.as_deref().unwrap_or("")) };
    let away = q.away.as_deref() == Some("1");
    let pad = q.pad.as_deref() == Some("1");
    up.on_upgrade(move |socket| screen_client(socket, app, video, away, pad, reg))
}

/// One phone's Screen view: pictures out, taps and typing in (only in "control" mode). Ends when
/// the phone leaves or the PC turns the Screen view off.
async fn screen_client(mut socket: WebSocket, app: Arc<App>, video: crate::screencast::Video, away: bool, pad: bool, reg: Registered) {
    let (quality, fps, kbps, smooth) = {
        let s = app.settings.lock();
        (s.remote_screen_quality.clone(), s.remote_screen_fps, s.remote_screen_kbps, s.remote_screen_priority != "sharp")
    };
    if app.screen_viewers.fetch_add(1, Ordering::Relaxed) == 0 {
        app.toast(crate::app::Toast::simple("screen-view", "Your phone is viewing this screen", "Turn it off in Deck → Phone → Screen on your phone."));
    }
    app.changed("remote");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::screencast::Out>(8);
    let sess = crate::screencast::start(quality, fps, kbps, smooth, video, away, pad, tx);
    let mut check = tokio::time::interval(Duration::from_millis(700));
    // checked at most every second: taps come many times a second
    let mut game: (std::time::Instant, Option<String>) = (std::time::Instant::now() - Duration::from_secs(5), None);
    loop {
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(t) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                match v["t"].as_str().unwrap_or("") {
                    "ack" => sess.ack(),
                    "idr" => sess.key(),
                    "padack" => sess.pad_ack(v["n"].as_u64().unwrap_or(0) as u32),
                    "vmax" => sess.max_w(v["w"].as_u64().unwrap_or(0) as u32),
                    "codec" => {
                        let c = if v["v"].as_str() == Some("jpeg") { "jpeg" } else { "auto" };
                        app.settings.lock().remote_screen_codec = c.into();
                        app.save_settings();
                    }
                    "mon" => sess.monitor(v["i"].as_u64().unwrap_or(0) as usize),
                    "prio" => {
                        let sharp = v["v"].as_str() == Some("sharp");
                        app.settings.lock().remote_screen_priority = if sharp { "sharp" } else { "smooth" }.into();
                        app.save_settings();
                        sess.smooth(!sharp);
                    }
                    "kbps" => {
                        let k = (v["kbps"].as_u64().unwrap_or(0) as u32).min(200_000);
                        let k = if k > 0 { k.max(300) } else { 0 };
                        app.settings.lock().remote_screen_kbps = k;
                        app.save_settings();
                        sess.kbps(k);
                    }
                    "fps" => {
                        let f = match v["fps"].as_u64() { Some(f @ (15 | 24 | 60)) => f as u32, _ => 30 };
                        app.settings.lock().remote_screen_fps = f;
                        app.save_settings();
                        sess.fps(f);
                    }
                    "q" => {
                        let q = v["q"].as_str().unwrap_or("balanced").to_string();
                        if ["sharp", "balanced", "fast"].contains(&q.as_str()) {
                            app.settings.lock().remote_screen_quality = q.clone();
                            app.save_settings();
                            sess.quality(&q);
                        }
                    }
                    _ => {
                        let control = app.settings.lock().remote_screen == "control";
                        let mon = *sess.mon.lock();
                        let (true, Some(m)) = (control, mon) else { continue };
                        if game.0.elapsed() >= Duration::from_secs(1) {
                            let was = game.1.is_some();
                            game = (std::time::Instant::now(), crate::boost::game_in_front(&app));
                            if game.1.is_some() != was {
                                let text = game.1.as_ref().map(|g| format!("Control is paused while {g} is in front: Deck never sends input into games (anti-cheat). Watching still works.")).unwrap_or_default();
                                if socket.send(Message::Text(json!({ "t": "note", "text": text }).to_string().into())).await.is_err() { break; }
                            }
                        }
                        if game.1.is_none() {
                            crate::screencast::input(&m, &v);
                        }
                    }
                }
            }
            o = rx.recv() => {
                let sent = match o {
                    Some(crate::screencast::Out::Frame(b)) => socket.send(Message::Binary(b.into())).await,
                    Some(crate::screencast::Out::Text(s)) => socket.send(Message::Text(s.into())).await,
                    None => break,
                };
                if sent.is_err() { break; }
            }
            _ = check.tick() => {
                let mode = app.settings.lock().remote_screen.clone();
                if mode == "off" {
                    let _ = socket.send(Message::Text(json!({ "t": "off" }).to_string().into())).await;
                    break;
                }
                let _ = socket.send(Message::Text(json!({ "t": "mode", "mode": mode }).to_string().into())).await;
            }
        }
    }
    drop(sess);
    if app.screen_viewers.fetch_sub(1, Ordering::Relaxed) == 1 {
        app.dismiss_toast("screen-view");
    }
    drop(reg);
    app.changed("remote");
}

async fn client(mut socket: WebSocket, app: Arc<App>, reg: Registered) {
    app.remote_clients.fetch_add(1, Ordering::Relaxed);
    app.changed("remote");
    let mut changes = app.changes.subscribe();
    let mut tick = tokio::time::interval(Duration::from_millis(1500));
    loop {
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { break };
                if let Message::Text(t) = msg {
                    if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                        let a2 = app.clone();
                        let reply = tokio::task::spawn_blocking(move || handle(&a2, &v)).await.ok().flatten();
                        if let Some(r) = reply {
                            if socket.send(Message::Text(r.to_string().into())).await.is_err() { break; }
                        }
                        // show the result right away
                        let a2 = app.clone();
                        if let Ok(st) = tokio::task::spawn_blocking(move || phone_state(&a2)).await {
                            if socket.send(Message::Text(st.to_string().into())).await.is_err() { break; }
                        }
                    }
                }
            }
            ch = changes.recv() => {
                match ch {
                    Ok(topic) if ["power", "shared", "toast", "media", "boost"].contains(&topic.as_str()) => {
                        let a2 = app.clone();
                        if let Ok(st) = tokio::task::spawn_blocking(move || phone_state(&a2)).await {
                            if socket.send(Message::Text(st.to_string().into())).await.is_err() { break; }
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
            _ = tick.tick() => {
                let a2 = app.clone();
                if let Ok(st) = tokio::task::spawn_blocking(move || phone_state(&a2)).await {
                    if socket.send(Message::Text(st.to_string().into())).await.is_err() { break; }
                }
            }
        }
    }
    app.remote_clients.fetch_sub(1, Ordering::Relaxed);
    drop(reg);
    app.changed("remote");
}

fn handle(app: &Arc<App>, v: &Value) -> Option<Value> {
    let a = v.get("a").and_then(Value::as_str).unwrap_or("");
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let r: Result<(), String> = match a {
        "media" => app.media.control(&s("id"), &s("do"), f("pos")),
        "master" => crate::audio::set_master(f("vol").map(|x| x as f32), v.get("muted").and_then(Value::as_bool)),
        "appvol" => crate::audio::set_app(&s("key"), f("vol").map(|x| x as f32), v.get("muted").and_then(Value::as_bool)),
        "output" => crate::audio::set_default_output(&s("id")),
        "profile" => {
            let (kind, id, a2) = (s("kind"), s("id"), app.clone());
            if kind == "cpu" || kind == "gpu" {
                std::thread::spawn(move || crate::profiles::switch(&a2, &kind, &id, "phone"));
                Ok(())
            } else {
                Err("unknown profile kind".into())
            }
        }
        "awake" => {
            let mut aw = app.power.awake.lock();
            if aw.mode != s("mode") {
                aw.dl_seen = false;
                aw.dl_quiet_since = None;
            }
            aw.mode = s("mode");
            if let Some(m) = f("minutes") {
                aw.until = crate::util::now_s() + (m as u64) * 60;
            }
            drop(aw);
            app.changed("power");
            Ok(())
        }
        "plan" => {
            let p: Option<crate::power::Plan> = v.get("plan").and_then(|p| serde_json::from_value(p.clone()).ok());
            match p {
                Some(mut p) => {
                    p.state = "waiting".into();
                    p.created = crate::util::now_s();
                    *app.power.plan.lock() = Some(p);
                    app.changed("power");
                    Ok(())
                }
                None => Err("bad plan".into()),
            }
        }
        "plan_cancel" => {
            *app.power.plan.lock() = None;
            app.dismiss_toast("plan");
            app.changed("power");
            Ok(())
        }
        "timer" => {
            app.power.add_timer(f("secs").unwrap_or(60.0) as u64, &s("label"));
            app.changed("power");
            Ok(())
        }
        "timer_cmd" => {
            app.power.timer_cmd(v.get("id").and_then(Value::as_u64).unwrap_or(0), &s("do"));
            app.changed("power");
            Ok(())
        }
        "power" => {
            let what = s("do");
            if ["lock", "sleep", "screen_off", "shutdown", "restart", "hibernate"].contains(&what.as_str()) {
                crate::power::run_action(&what)
            } else {
                Err("unknown".into())
            }
        }
        "toast" => {
            app.toast_action(&s("id"), &s("do"));
            Ok(())
        }
        // Full power on/off or Clean up; takes a few seconds, the result comes as a toast.
        "boost" | "clean" => {
            let (app, what) = (app.clone(), a.to_string());
            std::thread::spawn(move || crate::boost_from_tray(&app, &what));
            Ok(())
        }
        "send_text" => {
            let text = s("text");
            let kind = if text.starts_with("http://") || text.starts_with("https://") { "link" } else { "text" };
            let mode = s("mode");
            push_shared(app, Shared { id: 0, kind: kind.into(), text: text.clone(), path: None, name: String::new(), size: text.len() as u64, ts: crate::util::now_ms(), from: "phone".into() });
            if mode == "open" && kind == "link" {
                crate::util::shell_open(&text)
            } else {
                let r = crate::clipboard::set_text(&text);
                app.toast(crate::app::Toast::simple("phone-text", "Copied from your phone", &text.chars().take(120).collect::<String>()));
                r
            }
        }
        "clip_get" => {
            let t = app.clip.list("", 1).into_iter().find(|i| i.kind == "text").map(|i| i.text).unwrap_or_default();
            return Some(json!({ "t": "clip", "text": t }));
        }
        "shared_remove" => {
            let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
            app.shared.lock().retain(|x| x.id != id);
            app.changed("shared");
            Ok(())
        }
        _ => Err("unknown".into()),
    };
    r.err().map(|e| json!({ "t": "error", "msg": e }))
}

#[cfg(test)]
mod conn_tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn home_and_away_addresses() {
        for home in ["192.168.1.5", "10.0.0.2", "172.16.4.1", "127.0.0.1", "169.254.3.3", "::ffff:192.168.1.5", "fe80::1", "fd00::1", "::1"] {
            assert!(!is_online(ip(home)), "{home} is at home");
        }
        for away in ["100.64.0.1", "100.127.255.1", "8.8.8.8", "100.128.0.1", "::ffff:100.100.1.1", "2001:4860::8888"] {
            assert!(is_online(ip(away)), "{away} is away");
        }
    }

    fn conns(ips: &[&str]) -> Vec<Conn> {
        ips.iter().enumerate().map(|(i, s)| Conn { id: i as u64, ip: ip(s) }).collect()
    }

    #[test]
    fn a_phone_counts_once() {
        assert_eq!(count(&conns(&["192.168.1.5", "192.168.1.5", "192.168.1.5", "100.64.0.9"])), (1, 1));
    }

    #[test]
    fn limits_and_shut_outs() {
        let none = Limits::default();
        let one_home = Limits { max_home: 1, ..none };
        let list = conns(&["192.168.1.5"]);
        assert!(decide(&list, ip("192.168.1.6"), none).is_ok());
        assert!(decide(&list, ip("192.168.1.6"), one_home).is_err(), "second phone at home");
        assert!(decide(&list, ip("192.168.1.5"), one_home).is_ok(), "same phone, another page");
        assert!(decide(&list, ip("100.64.0.9"), one_home).is_ok(), "away has its own limit");
        let shut_away = Limits { home_blocks_away: true, ..none };
        assert!(decide(&list, ip("100.64.0.9"), shut_away).is_err());
        assert!(decide(&[], ip("100.64.0.9"), shut_away).is_ok(), "nobody at home");
        let shut_home = Limits { away_blocks_home: true, ..none };
        assert!(decide(&conns(&["100.64.0.9"]), ip("192.168.1.6"), shut_home).is_err());
        assert!(decide(&conns(&["100.64.0.9", "100.64.0.10"]), ip("100.64.0.11"), Limits { max_away: 2, ..none }).is_err());
    }
}
