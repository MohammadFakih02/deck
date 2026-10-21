//! One-time share links (PC to PC → Share link): a file or folder for someone who isn't one of
//! your PCs, downloaded in any browser, no Deck needed.
//!
//! A small web server of its own (port 47837, nothing else on it) runs only while a link is open.
//! At home the link is this PC's address; over the internet it goes through Tailscale Funnel,
//! which publishes only `/deck-share/` and only while an internet link is open. A link stops
//! after its downloads (1 by default; a download that breaks off doesn't count) or when it
//! expires. A folder is zipped first. Links survive a restart of Deck (`shares.json`).

use crate::app::{App, Toast};
use axum::extract::Path as AxPath;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub const PORT: u16 = 47837;
/// The path Tailscale Funnel publishes (nothing else of this PC is reachable through it).
const FUNNEL_PATH: &str = "/deck-share";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Share {
    pub token: String,
    pub name: String,
    /// what is served: the file, or the zip made of a folder
    pub path: String,
    /// the zip is Deck's own (deleted with the link)
    pub zipped: bool,
    pub size: u64,
    pub created: u64,
    /// unix ms
    pub expires: u64,
    /// downloads allowed (0 = any number until it expires)
    pub max: u32,
    pub done: u32,
    pub internet: bool,
    /// still being zipped
    #[serde(default)]
    pub preparing: bool,
    #[serde(default)]
    pub error: String,
    /// downloads running right now (each holds one of `max` until it ends or breaks off)
    #[serde(skip)]
    pub active: u32,
}

struct State {
    shares: Vec<Share>,
    server: Option<tauri::async_runtime::JoinHandle<()>>,
    /// the internet address (https://name.tailnet.ts.net) while Funnel is on
    funnel: Option<String>,
    funnel_error: String,
}

static ST: Mutex<State> = Mutex::new(State { shares: Vec::new(), server: None, funnel: None, funnel_error: String::new() });
static APP: OnceLock<Arc<App>> = OnceLock::new();

fn dir() -> PathBuf {
    crate::settings::Settings::dir().join("shares")
}

fn save(shares: &[Share]) {
    let _ = std::fs::create_dir_all(dir());
    let _ = crate::util::write_atomic(&dir().join("shares.json"), &serde_json::to_vec_pretty(shares).unwrap_or_default());
}

fn changed() {
    if let Some(a) = APP.get() {
        a.ui_changed("share");
    }
}

/// Deck started: links still open are served again (expired ones go).
pub fn init(app: Arc<App>) {
    let _ = APP.set(app);
    let mut shares: Vec<Share> = std::fs::read(dir().join("shares.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    shares.retain(|s| !s.preparing);
    let internet = shares.iter().any(|s| s.internet);
    ST.lock().shares = shares;
    // Funnel may still be on from before (Deck closed or crashed with an internet link open):
    // with no internet link left it goes off now, not only after the next change
    if !internet && crate::rdesk::tailscale_exe().is_some() {
        std::thread::spawn(|| funnel(false));
    }
    tidy();
    // expiry is checked once a minute
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        tidy();
    });
}

/// Drops finished and expired links (and their zips), and runs the server and Funnel only while needed.
fn tidy() {
    let now = crate::util::now_ms();
    let (gone, any, internet) = {
        let mut st = ST.lock();
        let (keep, gone): (Vec<Share>, Vec<Share>) = st.shares.drain(..).partition(|s| s.preparing || (s.expires > now && (s.max == 0 || s.done < s.max) && Path::new(&s.path).exists()));
        st.shares = keep;
        save(&st.shares);
        (gone, !st.shares.is_empty(), st.shares.iter().any(|s| s.internet && !s.preparing))
    };
    for s in &gone {
        if s.zipped {
            let _ = std::fs::remove_file(&s.path);
        }
    }
    if any {
        start_server();
    } else {
        stop_server();
    }
    if internet {
        if ST.lock().funnel.is_none() {
            funnel(true);
        }
    } else if ST.lock().funnel.is_some() {
        funnel(false);
    }
    if !gone.is_empty() {
        changed();
    }
}

fn start_server() {
    let mut st = ST.lock();
    if st.server.is_some() {
        return;
    }
    // other devices on the Wi-Fi reach it through Deck's firewall rule (home and work networks)
    std::thread::spawn(crate::remote::allow_between_pcs);
    use axum::extract::ConnectInfo;
    let page = |AxPath(t): AxPath<String>, ConnectInfo(from): ConnectInfo<SocketAddr>, headers: axum::http::HeaderMap| async move { page(&t, via_funnel(&headers, from.ip())) };
    let dl = |AxPath(t): AxPath<String>, ConnectInfo(from): ConnectInfo<SocketAddr>, headers: axum::http::HeaderMap| async move { download(&t, &headers, via_funnel(&headers, from.ip())).await };
    let router = Router::new()
        .route("/s/{t}", get(page))
        .route("/s/{t}/download", get(dl))
        // Funnel hands requests on with or without its own path in front
        .route("/deck-share/{t}", get(page))
        .route("/deck-share/{t}/download", get(dl))
        .route("/{t}", get(page))
        .route("/{t}/download", get(dl));
    st.server = Some(tauri::async_runtime::spawn(async move {
        let ip = if crate::settings::is_test_profile() { [127, 0, 0, 1] } else { [0, 0, 0, 0] };
        match tokio::net::TcpListener::bind(SocketAddr::from((ip, PORT))).await {
            Ok(l) => {
                let _ = axum::serve(l, router.into_make_service_with_connect_info::<SocketAddr>()).await;
            }
            Err(e) => {
                ST.lock().funnel_error = format!("Port {PORT} is taken ({e}): links can't be served");
                changed();
            }
        }
    }));
}

fn stop_server() {
    if let Some(h) = ST.lock().server.take() {
        h.abort();
    }
}

/// Tailscale Funnel on or off for `/deck-share`. The first time, Tailscale asks the account
/// owner to allow Funnel: its link is shown on the page.
fn funnel(on: bool) {
    let Some(exe) = crate::rdesk::tailscale_exe() else {
        ST.lock().funnel_error = "Tailscale isn't installed (needed for links that work over the internet)".into();
        return;
    };
    let exe = exe.display().to_string();
    if !on {
        let _ = crate::util::run_hidden(&exe, &["funnel", "--yes", "--https=443", "--set-path", FUNNEL_PATH, "off"]);
        let mut st = ST.lock();
        st.funnel = None;
        st.funnel_error.clear();
        return;
    }
    let target = format!("http://127.0.0.1:{PORT}{FUNNEL_PATH}");
    // Funnel waits for the owner to allow it the first time: give up after 20 s and show the link
    let out = std::process::Command::new(&exe)
        .args(["funnel", "--bg", "--yes", "--set-path", FUNNEL_PATH, &target])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags_hidden()
        .spawn()
        .and_then(|mut c| {
            let t0 = std::time::Instant::now();
            loop {
                if let Some(_s) = c.try_wait()? {
                    return c.wait_with_output();
                }
                if t0.elapsed() > std::time::Duration::from_secs(20) {
                    let _ = c.kill();
                    return c.wait_with_output();
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
    let text = out.map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))).unwrap_or_default();
    let dns = crate::rdesk::tailscale(true)["dns"].as_str().map(|s| s.trim_end_matches('.').to_string());
    let mut st = ST.lock();
    if let Some(url) = text.split_whitespace().find(|w| w.starts_with("https://login.tailscale.com")) {
        st.funnel_error = format!("Allow Funnel for your Tailscale account once: {url}");
        st.funnel = None;
    } else if text.to_lowercase().contains("error") || dns.is_none() {
        st.funnel_error = format!("Tailscale Funnel didn't start: {}", text.lines().find(|l| !l.trim().is_empty()).unwrap_or("Tailscale isn't connected"));
        st.funnel = None;
    } else {
        st.funnel = dns.map(|d| format!("https://{d}"));
        st.funnel_error.clear();
    }
    drop(st);
    changed();
}

trait Hidden {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}
impl Hidden for std::process::Command {
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(crate::util::CREATE_NO_WINDOW)
    }
}

/// A new link for `path`: expires in `hours`, `max` downloads (0 = any), over the internet too.
pub fn create(path: &str, hours: u64, max: u32, internet: bool) -> Result<Value, String> {
    let p = PathBuf::from(path);
    let meta = std::fs::metadata(&p).map_err(|_| "That file or folder isn't there".to_string())?;
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("Pick a file or folder")?;
    let token = crate::util::random_token(24);
    let now = crate::util::now_ms();
    let folder = meta.is_dir();
    // Deck has admin rights: only what the signed-in user could open themselves is shared
    let check = if folder { crate::fsguard::user_may_read_tree(&p) } else { crate::fsguard::open_read_for_user(&p).map(drop) };
    check.map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { format!("Can't share that: {e}") } else { format!("Can't read that: {e}") })?;
    let share = Share {
        token: token.clone(),
        name: if folder { format!("{name}.zip") } else { name.clone() },
        path: if folder { dir().join(format!("{token}.zip")).display().to_string() } else { p.display().to_string() },
        zipped: folder,
        size: if folder { 0 } else { meta.len() },
        created: now,
        expires: now + hours.clamp(1, 24 * 30) * 3_600_000,
        max,
        done: 0,
        internet,
        preparing: folder,
        error: String::new(),
        active: 0,
    };
    {
        let mut st = ST.lock();
        st.shares.push(share.clone());
        save(&st.shares);
    }
    crate::log::info(format!(
        "share: link {now} made for a {} ({}), {} h, {} download(s), {}",
        if folder { "folder" } else { "file" },
        if folder { "zipping".to_string() } else { crate::util::fmt_bytes(meta.len()) },
        hours.clamp(1, 24 * 30),
        if max == 0 { "any number of".to_string() } else { max.to_string() },
        if internet { "open to the internet" } else { "home network only" }
    ));
    if folder {
        // zipped by Windows' own tar (bsdtar), next to the shares list
        let (zip, parent, leaf, tok) = (share.path.clone(), p.parent().map(Path::to_path_buf).unwrap_or_default(), name.clone(), token.clone());
        std::thread::spawn(move || {
            // the zip is written (by tar, with Deck's rights) only into Deck's own folder, held
            // open meanwhile so it can't be swapped for a link to somewhere else
            let held = crate::fsguard::open_for_user(&dir());
            let ok = held.as_ref().is_ok_and(|d| {
                let out = d.real().join(format!("{tok}.zip"));
                std::process::Command::new("tar.exe").args(["-a", "-c", "-f"]).arg(&out).arg("-C").arg(&parent).arg(&leaf).creation_flags_hidden().status().map(|s| s.success()).unwrap_or(false)
                    && crate::fsguard::real_path(&out).is_ok_and(|r| r.parent() == Some(d.real()))
            });
            drop(held);
            let size = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
            {
                let mut st = ST.lock();
                if let Some(s) = st.shares.iter_mut().find(|s| s.token == tok) {
                    s.preparing = false;
                    s.size = size;
                    if !ok || size == 0 {
                        crate::log::warn(format!("share: link {} ended: the folder couldn't be zipped", s.created));
                        s.error = "Couldn't zip the folder".into();
                        s.expires = 0;
                    }
                }
                save(&st.shares);
            }
            tidy();
            changed();
        });
    }
    tidy();
    changed();
    Ok(state())
}

pub fn stop(token: &str) -> Value {
    {
        let mut st = ST.lock();
        if let Some(s) = st.shares.iter_mut().find(|s| s.token == token) {
            crate::log::info(format!("share: link {} ended by the user", s.created));
            s.expires = 0;
            s.preparing = false;
        }
    }
    tidy();
    state()
}

/// The Share link tab: open links with their addresses.
pub fn state() -> Value {
    let st = ST.lock();
    let lan = crate::net::lan_ip();
    let shares: Vec<Value> = st
        .shares
        .iter()
        .map(|s| {
            json!({
                "token": s.token, "name": s.name, "size": s.size, "created": s.created, "expires": s.expires, "max": s.max, "done": s.done,
                "internet": s.internet, "preparing": s.preparing, "error": s.error,
                "home": lan.as_ref().map(|ip| format!("http://{ip}:{PORT}/s/{}", s.token)),
                "web": if s.internet { st.funnel.as_ref().map(|f| format!("{f}{FUNNEL_PATH}/{}", s.token)) } else { None },
            })
        })
        .collect();
    json!({ "shares": shares, "funnel": st.funnel, "funnel_error": st.funnel_error, "tailscale": crate::rdesk::tailscale_exe().is_some() })
}

fn live(token: &str, via_funnel: bool) -> Option<Share> {
    let now = crate::util::now_ms();
    ST.lock().shares.iter().find(|s| s.token == token && !s.preparing && s.expires > now && (s.max == 0 || s.done < s.max) && may_serve(s, via_funnel)).cloned()
}

/// A download of `token` starting at `now`: takes one of the link's places, or false when the link
/// has ended or every place is taken (a download in progress holds one).
fn take_slot(shares: &mut [Share], token: &str, now: u64) -> bool {
    let Some(s) = shares.iter_mut().find(|s| s.token == token && !s.preparing && s.expires > now) else { return false };
    if s.max > 0 && s.done + s.active >= s.max {
        return false;
    }
    s.active += 1;
    true
}

/// A download ended: `done` it reached the file's end and uses the place up, otherwise (broken off)
/// the place is free again.
fn end_slot(shares: &mut [Share], token: &str, done: bool) {
    if let Some(s) = shares.iter_mut().find(|s| s.token == token) {
        s.active = s.active.saturating_sub(1);
        if done {
            s.done += 1;
        }
    }
}

/// Whether `s` may be served to this request: one that came through Tailscale Funnel (from the
/// internet) gets only links made for the internet.
fn may_serve(s: &Share, via_funnel: bool) -> bool {
    s.internet || !via_funnel
}

/// Whether a request came in through Tailscale Funnel: Funnel marks what it forwards, and it hands
/// it on from this PC itself (home links are opened by the PC's network address, not 127.0.0.1).
fn via_funnel(headers: &axum::http::HeaderMap, from: std::net::IpAddr) -> bool {
    let test = crate::settings::is_test_profile();
    headers.contains_key("tailscale-funnel-request") || (from.is_loopback() && !test)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const PAGE: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex">
<title>{TITLE}</title><style>
:root{color-scheme:light dark;--bg:#f4f6fb;--card:#fff;--text:#141824;--dim:#5b6475;--acc:#3b6cf6}
@media (prefers-color-scheme:dark){:root{--bg:#0d1017;--card:#161b26;--text:#e8ecf4;--dim:#98a2b6;--acc:#5b8cff}}
body{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--text);font:15px/1.5 system-ui,Segoe UI,sans-serif;padding:16px;box-sizing:border-box}
.c{background:var(--card);border-radius:18px;padding:28px;max-width:420px;width:100%;box-shadow:0 10px 40px #0002;text-align:center}
h1{font-size:19px;margin:10px 0 4px;word-break:break-word}.d{color:var(--dim);font-size:13px}
a.b{display:inline-block;margin-top:20px;background:var(--acc);color:#fff;text-decoration:none;font-weight:600;padding:12px 26px;border-radius:12px}
.i{width:46px;height:46px;border-radius:12px;background:var(--acc);display:inline-grid;place-items:center;color:#fff;font-size:22px}
</style></head><body><div class="c">{BODY}</div></body></html>"#;

fn page(token: &str, via_funnel: bool) -> Response {
    let Some(s) = live(token, via_funnel) else {
        let body = r#"<div class="i">✕</div><h1>This link has ended</h1><div class="d">It was already downloaded, it expired, or it was stopped. Ask for a new one.</div>"#;
        return (StatusCode::GONE, Html(PAGE.replace("{TITLE}", "Link ended").replace("{BODY}", body))).into_response();
    };
    let left = s.expires.saturating_sub(crate::util::now_ms()) / 60_000;
    let until = if left >= 1440 { format!("{} days", left / 1440) } else if left >= 60 { format!("{} hours", left / 60) } else { format!("{} minutes", left.max(1)) };
    let once = if s.max == 1 { "One download only. ".to_string() } else if s.max > 1 { format!("{} downloads left. ", s.max - s.done) } else { String::new() };
    let body = format!(
        r#"<div class="i">↓</div><h1>{}</h1><div class="d">{} · {once}Ends in {until}</div><a class="b" href="{}/download" download>Download</a><div class="d" style="margin-top:18px">Shared from a PC with Deck</div>"#,
        esc(&s.name),
        crate::util::fmt_bytes(s.size),
        esc(&s.token)
    );
    ([(header::CACHE_CONTROL, "no-store"), (header::REFERRER_POLICY, "no-referrer")], Html(PAGE.replace("{TITLE}", &esc(&s.name)).replace("{BODY}", &body))).into_response()
}

/// One download's place on its link, held from its start. Dropped without `done` (broken off, or
/// the file couldn't be read) it frees the place; the browser can continue, and the continued part
/// that reaches the end counts.
struct Slot {
    token: String,
    name: String,
    done: bool,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let left = {
            let mut st = ST.lock();
            end_slot(&mut st.shares, &self.token, self.done);
            // a link by when it was made: its token is the key to the file and stays out of the log
            let made = st.shares.iter().find(|s| s.token == self.token).map(|s| s.created).unwrap_or(0);
            if !self.done {
                crate::log::info(format!("share: a download of link {made} broke off (its place is free again)"));
                return;
            }
            crate::log::info(format!("share: link {made} was downloaded"));
            save(&st.shares);
            st.shares.iter().find(|s| s.token == self.token).map(|s| if s.max == 0 { None } else { Some(s.max.saturating_sub(s.done)) })
        };
        let Some(left) = left else { return };
        counted(&self.token, &self.name, left);
    }
}

/// A download reached the file's end.
fn counted(token: &str, name: &str, left: Option<u32>) {
    if let Some(a) = APP.get() {
        let body = match left {
            Some(0) => "The link has ended.".to_string(),
            Some(n) => format!("{n} download{} left.", if n == 1 { "" } else { "s" }),
            None => "The link stays open until it expires.".to_string(),
        };
        a.toast(Toast::simple(&format!("share:{token}"), &format!("{name} was downloaded"), &body));
    }
    std::thread::spawn(tidy);
}

async fn download(token: &str, headers: &axum::http::HeaderMap, via_funnel: bool) -> Response {
    let Some(s) = live(token, via_funnel) else { return page(token, via_funnel) };
    if !take_slot(&mut ST.lock().shares, token, crate::util::now_ms()) {
        return (StatusCode::CONFLICT, Html(PAGE.replace("{TITLE}", "Already downloading").replace("{BODY}", r#"<div class="i">…</div><h1>Already downloading</h1><div class="d">This link is being downloaded right now. If that download stops, try again.</div>"#))).into_response();
    }
    let slot = Slot { token: s.token.clone(), name: s.name.clone(), done: false };
    // the closure owns the whole slot (not just its flag), so the place is held while the file streams
    let on_end = move || {
        let mut slot = slot;
        slot.done = true;
    };
    crate::remote::serve_file(&s.path, &s.name, headers, Some(Box::new(on_end))).await
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    fn link(max: u32, internet: bool) -> Share {
        Share { token: "t".into(), name: "n".into(), path: "p".into(), zipped: false, size: 1, created: 0, expires: u64::MAX, max, done: 0, internet, preparing: false, error: String::new(), active: 0 }
    }

    /// A download takes its place when it starts: two at once can't both use a one-download link.
    /// One that breaks off gives its place back (the browser may continue it).
    #[test]
    fn downloads_reserve_their_place() {
        let mut v = vec![link(1, false)];
        assert!(take_slot(&mut v, "t", 1));
        assert!(!take_slot(&mut v, "t", 1), "a second download at the same time");
        end_slot(&mut v, "t", false);
        assert!(take_slot(&mut v, "t", 1), "a broken-off download gave its place back");
        end_slot(&mut v, "t", true);
        assert_eq!((v[0].done, v[0].active), (1, 0));
        assert!(!take_slot(&mut v, "t", 1), "used up");
        let mut any = vec![link(0, false)];
        assert!(take_slot(&mut any, "t", 1) && take_slot(&mut any, "t", 1), "no limit");
        assert!(!take_slot(&mut any, "nope", 1));
    }

    /// Through Tailscale Funnel (the internet) only links made for the internet are served.
    #[test]
    fn home_links_are_not_served_to_the_internet() {
        assert!(may_serve(&link(1, true), true) && may_serve(&link(1, true), false));
        assert!(may_serve(&link(1, false), false));
        assert!(!may_serve(&link(1, false), true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-download link: its page, the file byte for byte, then "ended"; a folder arrives zipped.
    #[test]
    #[ignore = "opens a network port; run with --ignored"]
    fn one_time_link() {
        std::env::set_var("DECK_PROFILE", "test");
        let tmp = std::env::temp_dir().join(format!("deck-share-test-{}", crate::util::now_ms()));
        std::fs::create_dir_all(tmp.join("folder")).unwrap();
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(tmp.join("photo.bin"), &data).unwrap();
        std::fs::write(tmp.join("folder").join("a.txt"), b"hello").unwrap();
        create(&tmp.join("photo.bin").display().to_string(), 1, 1, false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let tok = ST.lock().shares[0].token.clone();
        let base = format!("http://127.0.0.1:{PORT}/s/{tok}");
        let page = ureq::get(&base).call().unwrap().into_string().unwrap();
        assert!(page.contains("photo.bin") && page.contains("One download only"), "{page}");
        // a download that broke off after 1 MB, continued by the browser from there
        let mut got = Vec::new();
        let r = ureq::get(&format!("{base}/download")).set("Range", "bytes=0-1048575").call().unwrap();
        assert_eq!(r.status(), 206);
        r.into_reader().read_to_end(&mut got).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(ST.lock().shares.iter().find(|s| s.token == tok).map(|s| s.done), Some(0), "a part doesn't use the link up");
        let r = ureq::get(&format!("{base}/download")).set("Range", "bytes=1048576-").call().unwrap();
        assert_eq!(r.header("Content-Range"), Some(format!("bytes 1048576-{}/{}", data.len() - 1, data.len()).as_str()));
        r.into_reader().read_to_end(&mut got).unwrap();
        assert_eq!(got, data, "the file arrives byte for byte");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let again = ureq::get(&base).call();
        // "ended" page, or nothing at all when it was the only link (the server stops with it)
        assert!(matches!(again, Err(ureq::Error::Status(410, _)) | Err(ureq::Error::Transport(_))), "the link ends after one download");
        assert!(ST.lock().shares.is_empty());
        // a folder: zipped, then served
        create(&tmp.join("folder").display().to_string(), 1, 0, false).unwrap();
        for _ in 0..50 {
            if ST.lock().shares.iter().any(|s| s.name == "folder.zip" && !s.preparing) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let s = ST.lock().shares.iter().find(|s| s.name == "folder.zip").cloned().unwrap();
        assert!(s.size > 0 && s.error.is_empty(), "{s:?}");
        let mut zip = Vec::new();
        ureq::get(&format!("http://127.0.0.1:{PORT}/s/{}/download", s.token)).call().unwrap().into_reader().read_to_end(&mut zip).unwrap();
        assert_eq!(&zip[..2], b"PK", "a zip");
        stop(&s.token);
        assert!(!Path::new(&s.path).exists(), "the zip goes with the link");

        // two downloads at once of a one-download link: the second waits its turn; the first
        // breaking off frees the place, and the next whole download uses the link up
        create(&tmp.join("photo.bin").display().to_string(), 1, 1, false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let tok = ST.lock().shares.iter().find(|s| s.name == "photo.bin").unwrap().token.clone();
        let url = format!("http://127.0.0.1:{PORT}/s/{tok}/download");
        let mut first = ureq::get(&url).call().unwrap().into_reader();
        let mut head = [0u8; 1000];
        first.read_exact(&mut head).unwrap();
        let second = ureq::get(&url).call();
        assert!(matches!(second, Err(ureq::Error::Status(409, _))), "a second download at the same time is refused");
        drop(first);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut whole = Vec::new();
        ureq::get(&url).call().expect("the place was given back").into_reader().read_to_end(&mut whole).unwrap();
        assert_eq!(whole, data);
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(ST.lock().shares.iter().all(|s| s.token != tok), "used up by the one whole download");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
