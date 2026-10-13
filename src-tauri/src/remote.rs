//! Phone remote: a web app on the local network (installable to the Android
//! app drawer) that controls media, volume, power and passes text, links
//! and files between the phone and the PC.

use crate::app::{App, Shared};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const PHONE_HTML: &str = include_str!("../phone/index.html");
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
    json!({
        "enabled": on,
        "lan": lan,
        "origin": origin,
        "qr": lan.as_deref().map(qr_svg),
        "port": port,
        "error": *remote.error.lock(),
        "clients": app.remote_clients.load(Ordering::Relaxed),
        "shared": *app.shared.lock(),
    })
}

type St = Arc<App>;

pub fn start(app: Arc<App>, remote: Arc<Remote>) {
    stop(&remote);
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
            .route("/check", get(check))
            .route("/upload", post(upload))
            .route("/file/{id}", get(file))
            .route("/icon.png", get(|| png(ICON_192)))
            .route("/icon-512.png", get(|| png(ICON_512)))
            .route("/icon-maskable.png", get(|| png(ICON_MASKABLE)))
            .route("/sw.js", get(service_worker))
            .route("/manifest.webmanifest", get(manifest))
            .with_state(app.clone());
        if let Err(e) = axum::serve(listener, router).await {
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

async fn upload(State(app): State<St>, Query(q): Query<Key>, body: Body) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
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
    let p = path.to_string_lossy().to_string();
    let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    push_shared(&app, Shared { id: 0, kind: "file".into(), text: String::new(), path: Some(p.clone()), name: fname.clone(), size, ts: crate::util::now_ms(), from: "phone".into() });
    app.toast(crate::app::Toast {
        id: format!("recv:{p}"),
        kind: "file".into(),
        title: "File from your phone".into(),
        body: format!("{fname} · {}", crate::util::fmt_bytes(size)),
        actions: vec![("show".into(), "Show in folder".into()), ("open".into(), "Open".into())],
        sticky: false,
        sound: false,
    });
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

async fn file(State(app): State<St>, AxPath(id): AxPath<u64>, Query(q): Query<Key>) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    let item = app.shared.lock().iter().find(|s| s.id == id).cloned();
    let Some(path) = item.and_then(|s| s.path) else { return (StatusCode::NOT_FOUND, "gone").into_response() };
    let Ok(f) = tokio::fs::File::open(&path).await else { return (StatusCode::NOT_FOUND, "gone").into_response() };
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
    let stream = tokio_util::io::ReaderStream::new(f);
    let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' { c } else { '_' }).collect();
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", pct(&name))),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

async fn ws(State(app): State<St>, Query(q): Query<Key>, up: WebSocketUpgrade) -> Response {
    if !authorized(&app, &q.k) {
        return (StatusCode::UNAUTHORIZED, "bad key").into_response();
    }
    up.on_upgrade(move |socket| client(socket, app))
}

/// Everything the phone shows (gathered on a blocking thread: COM calls).
fn phone_state(app: &App) -> Value {
    let power = app.power.state();
    let shared: Vec<Shared> = app.shared.lock().clone();
    let snap = app.sys.snapshot();
    let units = {
        let s = app.settings.lock();
        json!({ "clock": s.unit_clock, "net": s.unit_net, "temp": s.unit_temp })
    };
    let sys = json!({ "cpu": snap["cpu"], "ram": snap["ram_pct"], "gpu": snap["gpu"], "down": snap["down"], "up": snap["up"], "units": units });
    let profiles = {
        let s = app.settings.lock();
        json!({
            "cpu": s.cpu_profiles.iter().map(|p| json!({ "id": p.id, "name": p.name })).collect::<Vec<_>>(),
            "gpu": s.gpu_profiles.iter().map(|p| json!({ "id": p.id, "name": p.name })).collect::<Vec<_>>(),
            "active_cpu": s.active_cpu, "active_gpu": s.active_gpu,
        })
    };
    json!({
        "t": "state",
        "pc": std::env::var("COMPUTERNAME").unwrap_or_default(),
        "media": app.media.state(),
        "master": crate::audio::master(),
        "outputs": crate::audio::outputs(),
        "apps": crate::audio::apps(),
        "power": power,
        "shared": shared,
        "toasts": *app.toasts.lock(),
        "sys": sys,
        "boost": app.boost.brief(),
        "profiles": profiles,
    })
}

async fn client(mut socket: WebSocket, app: Arc<App>) {
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
