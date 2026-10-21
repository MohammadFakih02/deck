//! Updates from GitHub releases.
//!
//! Once a day (the first look 2 minutes after start) Deck asks GitHub for the latest release of its
//! own repo. A newer one (numeric `X.Y.Z`, never older or equal) with a `*-setup.exe` and its
//! `.sha256` is downloaded, checked against that checksum and, when it is safe to, run silently:
//!  - Deck runs with admin rights through its tasks and sits in a folder only admins can change:
//!    the setup is saved there, held open while it starts, and runs without a prompt. It waits until
//!    no game or fullscreen program is in front.
//!  - Otherwise (no admin tasks) Deck only tells the user; the update installs when they press
//!    "Install", with one UAC prompt: a Deck started with admin rights (`--install-update`) copies
//!    the setup into the admin-only `updates`, checks it again and runs it. A Deck in a folder the
//!    user can change doesn't update itself.
//!
//! The setup (hooks.nsh) closes Deck, installs, makes the admin tasks again and starts Deck hidden.
//! The checksum comes from the same release as the setup: it guards against a damaged download, not
//! against a hijacked release (that would need signing).

use crate::app::{App, Toast};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const API: &str = "https://api.github.com/repos/MohammadFakih02/deck/releases/latest";
/// Where a release's files live; nothing else is downloaded.
const FILES: &str = "https://github.com/MohammadFakih02/deck/releases/download/";
const MAX_SETUP: u64 = 150 * 1024 * 1024;
const MAX_API: u64 = 512 * 1024;
const FIRST_LOOK: Duration = Duration::from_secs(120);
const CHECK_EVERY_S: u64 = 24 * 3600;
const RETRY: Duration = Duration::from_secs(3600);
/// While an update waits for a game to end.
const POLL: Duration = Duration::from_secs(60);
/// The longest a setup is waited for (a slow disk, a virus scan, a UAC prompt left open).
const SETUP_MAX: Duration = Duration::from_secs(30 * 60);
/// After the setup ended: it may only just be closing Deck.
const AFTER_SETUP: Duration = Duration::from_secs(20);

/// `X.Y.Z` (an optional leading `v`); anything else (a pre-release, extra parts) is not a release.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().trim_start_matches('v').split('.');
    let v = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    it.next().is_none().then_some(v)
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub setup_name: String,
    pub setup_url: String,
    pub sha_url: String,
    pub notes: String,
}

/// The release in GitHub's "latest release" answer: a published, plain-numbered version with a setup
/// and its checksum file, both under this repo's release downloads.
pub fn pick(v: &Value) -> Option<Release> {
    if v["draft"].as_bool() != Some(false) || v["prerelease"].as_bool() != Some(false) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    parse_version(tag)?;
    let assets = v["assets"].as_array()?;
    let url_of = |name: &str| assets.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| a["browser_download_url"].as_str()).filter(|u| u.starts_with(FILES) && !u.contains("..") && !u.contains(['\\', ' ']));
    let setup = assets.iter().filter_map(|a| a["name"].as_str()).find(|n| n.ends_with("-setup.exe") && !n.contains(['/', '\\', ':']))?;
    let notes = notes_of(v["body"].as_str().unwrap_or(""));
    Some(Release { version: tag.trim_start_matches('v').to_string(), setup_name: setup.to_string(), setup_url: url_of(setup)?.to_string(), sha_url: url_of(&format!("{setup}.sha256"))?.to_string(), notes })
}

/// Release notes longer than this are cut (at a line end) for the Updates card.
const NOTES_MAX: usize = 20_000;

/// A release's notes as kept: all of them, or whole lines up to `NOTES_MAX` characters.
pub fn notes_of(body: &str) -> String {
    if body.chars().count() <= NOTES_MAX {
        return body.to_string();
    }
    let cut: String = body.chars().take(NOTES_MAX).collect();
    match cut.rfind('\n') {
        Some(i) => cut[..i].to_string(),
        None => cut,
    }
}

/// The SHA-256 in a `.sha256` file ("<hex>  <name>" or just "<hex>"), lower case.
pub fn parse_sha(text: &str) -> Option<String> {
    let h = text.split_whitespace().next()?.to_lowercase();
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

// ------------------------------------------------------------ what the page shows

#[derive(Clone, Serialize, Debug)]
pub struct Status {
    pub current: String,
    /// "idle", "checking", "uptodate", "available", "downloading", "installing", "error"
    pub state: &'static str,
    pub latest: Option<String>,
    pub notes: String,
    pub checked: u64,
    pub error: Option<String>,
    /// The update installs by itself (admin start in a safe folder) and is waiting for a game to end.
    pub waiting: bool,
    /// An install can happen without a prompt (Deck is admin and in a safe place).
    pub silent: bool,
    /// While downloading: bytes so far, and the setup's size (0 when GitHub didn't say).
    pub got: u64,
    pub size: u64,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

fn fresh() -> Status {
    Status { current: env!("CARGO_PKG_VERSION").into(), state: "idle", latest: None, notes: String::new(), checked: 0, error: None, waiting: false, silent: false, got: 0, size: 0 }
}

fn status() -> Status {
    STATUS.lock().get_or_insert_with(fresh).clone()
}

fn set(app: &App, f: impl FnOnce(&mut Status)) {
    f(STATUS.lock().get_or_insert_with(fresh));
    app.changed("update");
}

/// Changes the status and tells the page only when something differs (the background loop runs
/// every minute while it waits).
fn set_quiet(app: &App, f: impl FnOnce(&mut Status)) {
    let changed = {
        let mut g = STATUS.lock();
        let s = g.get_or_insert_with(fresh);
        let before = (s.state, s.latest.clone(), s.checked, s.waiting, s.error.clone());
        f(s);
        before != (s.state, s.latest.clone(), s.checked, s.waiting, s.error.clone())
    };
    if changed {
        app.changed("update");
    }
}

pub fn state() -> Value {
    let mut s = status();
    s.silent = silent_ok();
    serde_json::to_value(s).unwrap_or(json!({}))
}

// ------------------------------------------------------------ remembered between runs

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    checked: u64,
    etag: Option<String>,
    release: Option<Release>,
    /// The version that ran last, to say "updated to X" once.
    last_run: Option<String>,
    /// A version whose install failed (not retried by itself before `retry_at`).
    failed: Option<String>,
    retry_at: u64,
    /// The version the user was last told about (when Deck can't install it by itself).
    told: Option<String>,
}

fn saved_path() -> PathBuf {
    crate::settings::Settings::dir().join("update.json")
}

fn load() -> Saved {
    std::fs::read_to_string(saved_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store(s: &Saved) {
    let _ = std::fs::create_dir_all(crate::settings::Settings::dir());
    if let Err(e) = crate::util::write_atomic(&saved_path(), serde_json::to_string(s).unwrap_or_default().as_bytes()) {
        crate::log::warn(format!("update: what was found couldn't be saved ({e}); the next start asks GitHub again"));
    }
}

// ------------------------------------------------------------ checking

/// Gives up on a connection that stalls for a minute, not on a download that takes longer than one
/// (a 10 MB setup takes about a minute on a slow line).
fn agent() -> ureq::Agent {
    crate::log::http(ureq::AgentBuilder::new()).timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(60)).timeout_write(Duration::from_secs(60)).build()
}

/// Asks GitHub for the latest release (one request; a 304 costs nothing). Updates the saved
/// answer and returns the release when it is newer than this build.
fn check() -> Result<Option<Release>, String> {
    let op = crate::log::Op::start("update", "check for a newer version");
    let r = check_inner();
    match &r {
        Ok(Some(rel)) => op.ok(format!("{} is out (this is {})", rel.version, env!("CARGO_PKG_VERSION"))),
        Ok(None) => op.ok_quiet("nothing newer"),
        Err(e) => op.fail(e),
    }
    r
}

fn check_inner() -> Result<Option<Release>, String> {
    let mut saved = load();
    let mut req = agent().get(API).timeout(Duration::from_secs(60)).set("User-Agent", "Deck").set("Accept", "application/vnd.github+json");
    if let (Some(tag), Some(_)) = (&saved.etag, &saved.release) {
        req = req.set("If-None-Match", tag);
    }
    let now = crate::util::now_s();
    match req.call() {
        // ureq only makes 4xx/5xx errors: "not modified" (the saved answer is still right) arrives here
        Ok(resp) if resp.status() == 304 => {}
        Ok(resp) => {
            let etag = resp.header("ETag").map(String::from);
            let mut body = Vec::new();
            resp.into_reader().take(MAX_API + 1).read_to_end(&mut body).map_err(|e| e.to_string())?;
            if body.len() as u64 > MAX_API {
                return Err("GitHub's answer was too big".into());
            }
            let v: Value = serde_json::from_slice(&body).map_err(|_| "GitHub's answer wasn't understood".to_string())?;
            saved.release = pick(&v);
            saved.etag = etag;
        }
        Err(ureq::Error::Status(404, _)) => saved.release = None,
        Err(e) => return Err(format!("couldn't reach GitHub ({e})")),
    }
    saved.checked = now;
    store(&saved);
    Ok(saved.release.filter(|r| is_newer(&r.version, env!("CARGO_PKG_VERSION"))))
}

// ------------------------------------------------------------ installing

/// How long `silent_ok` is believed (it starts schtasks and walks the folder's permissions).
const SILENT_FOR: Duration = Duration::from_secs(600);
static SILENT: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// An install can run without a prompt: Deck has admin rights, its admin tasks exist, and its folder
/// can't be changed by the everyday account (so nobody can swap what runs with admin rights).
/// Remembered for a while; `silent_now` looks again.
fn silent_ok() -> bool {
    if let Some((at, ok)) = *SILENT.lock() {
        if at.elapsed() < SILENT_FOR {
            return ok;
        }
    }
    silent_now()
}

fn silent_now() -> bool {
    let ok = std::env::current_exe().is_ok_and(|exe| crate::elevation::is_admin() && crate::elevation::task_exists(crate::elevation::BACKGROUND_TASK) && !crate::fsguard::user_could_change(&exe).unwrap_or(true));
    *SILENT.lock() = Some((Instant::now(), ok));
    ok
}

fn fetch_text(url: &str, max: u64) -> Result<String, String> {
    let resp = agent().get(url).set("User-Agent", "Deck").call().map_err(|e| format!("the download failed ({e})"))?;
    let mut body = Vec::new();
    resp.into_reader().take(max + 1).read_to_end(&mut body).map_err(|e| e.to_string())?;
    if body.len() as u64 > max {
        return Err("the checksum file is too big".into());
    }
    String::from_utf8(body).map_err(|_| "the checksum file isn't text".to_string())
}

/// How often the page hears how far the download is.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Downloads `url` into `to`, telling the page how far it is (`Status.got` / `size`).
fn download(app: &App, url: &str, to: &Path, max: u64) -> Result<(), String> {
    let resp = agent().get(url).set("User-Agent", "Deck").call().map_err(|e| format!("the download failed ({e})"))?;
    let size = resp.header("Content-Length").and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| *n <= max).unwrap_or(0);
    set(app, |s| {
        s.got = 0;
        s.size = size;
    });
    let mut out = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut from = resp.into_reader().take(max + 1);
    let mut buf = vec![0u8; 64 * 1024];
    let (mut got, mut told) = (0u64, Instant::now());
    loop {
        let n = from.read(&mut buf).map_err(|e| format!("the download failed ({e})"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        if got > max {
            return Err("the download was bigger than any Deck setup".into());
        }
        if told.elapsed() >= PROGRESS_EVERY {
            told = Instant::now();
            set(app, |s| s.got = got);
        }
    }
    set(app, |s| s.got = got);
    out.flush().map_err(|e| e.to_string())
}

fn sha256_of(f: &mut std::fs::File) -> std::io::Result<String> {
    use std::io::Seek;
    f.seek(std::io::SeekFrom::Start(0))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `updates` next to Deck, emptied: the only place a setup runs from with admin rights (only admins
/// can change it; the callers check that).
fn updates_dir() -> Result<PathBuf, String> {
    let dir = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("no folder")?.join("updates");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Opens `path` for reading, held: nobody can change it while the handle is open.
fn hold(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)
}

/// Downloads and checks the setup; returns it open and held (what is checked is what runs).
fn fetch_setup(app: &App, r: &Release, dir: &Path) -> Result<(PathBuf, std::fs::File), String> {
    let want = parse_sha(&fetch_text(&r.sha_url, 4096)?).ok_or("the checksum file isn't one")?;
    let setup = dir.join(&r.setup_name);
    download(app, &r.setup_url, &setup, MAX_SETUP)?;
    let mut held = hold(&setup).map_err(|e| e.to_string())?;
    if sha256_of(&mut held).map_err(|e| e.to_string())? != want {
        return Err("the download doesn't match its checksum, so it was thrown away".into());
    }
    Ok((setup, held))
}

/// Why an install failed, and in how many seconds the background loop may try it again by itself.
type Failure = (String, u64);
/// A download or start that failed (the network, GitHub): tried again in a few hours.
const RETRY_FAILED_S: u64 = 6 * 3600;
/// A setup that ran and did not install: tried again the next day.
const RETRY_SETUP_S: u64 = CHECK_EVERY_S;

/// Deck admin, in an admin-only folder: the setup is saved in `updates`, run without a prompt and
/// waited for. It closes Deck when it gets to it; returning at all means it didn't install.
fn install_silent(app: &App, r: &Release) -> Result<(), Failure> {
    use std::os::windows::process::CommandExt;
    let dir = updates_dir().map_err(|e| (e, RETRY_FAILED_S))?;
    let (setup, held) = fetch_setup(app, r, &dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&dir);
        (e, RETRY_FAILED_S)
    })?;
    crate::log::info(format!("update {}: downloaded and checked; starting the setup", r.version));
    set(app, |s| s.state = "installing");
    crate::restart::mark_updating();
    let child = std::process::Command::new(&setup).args(["/S", "/UPDATE"]).creation_flags(crate::util::CREATE_NO_WINDOW).spawn();
    // a running program's file can't be changed: the setup holds its own file from here on
    drop(held);
    let mut child = child.map_err(|e| {
        crate::restart::clear_updating();
        (format!("the setup didn't start ({e})"), RETRY_FAILED_S)
    })?;
    let started = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code(),
            Ok(None) if started.elapsed() < SETUP_MAX => std::thread::sleep(Duration::from_secs(2)),
            _ => {
                crate::restart::clear_updating();
                return Err(("The setup didn't finish in 30 minutes.".into(), RETRY_SETUP_S));
            }
        }
    };
    // it may only just be closing Deck
    std::thread::sleep(AFTER_SETUP);
    crate::restart::clear_updating();
    let _ = std::fs::remove_dir_all(&dir);
    Err((format!("The setup ended without installing (code {}).", code.map_or("none".into(), |c| c.to_string())), RETRY_SETUP_S))
}

/// Otherwise: the setup is downloaded and checked in a fresh Temp folder, then a copy of Deck started
/// with admin rights (one UAC prompt) copies it into the admin-only `updates`, checks it again and
/// runs it (`--install-update`). A setup never runs with admin rights from a folder the user can change.
fn install_asking(app: &App, r: &Release) -> Result<(), Failure> {
    let dir = std::env::temp_dir().join(format!("deck-update-{}", crate::util::random_token(12)));
    let result = (|| -> Result<(), Failure> {
        std::fs::create_dir_all(&dir).map_err(|e| (e.to_string(), RETRY_FAILED_S))?;
        let (setup, held) = fetch_setup(app, r, &dir).map_err(|e| (e, RETRY_FAILED_S))?;
        crate::log::info(format!("update {}: downloaded and checked; asking for admin rights to install it", r.version));
        set(app, |s| s.state = "installing");
        let exe = std::env::current_exe().map_err(|e| (e.to_string(), RETRY_FAILED_S))?;
        let params = format!("--install-update \"{}\" \"{}\" {}", setup.display(), r.sha_url, r.version);
        const SW_HIDE: i32 = 0;
        let code = crate::beam::setup::shell_run_as(Some("runas"), &exe.to_string_lossy(), &params, &dir.to_string_lossy(), SW_HIDE);
        drop(held);
        match code {
            Ok(0) => Ok(()),
            Ok(3) => Err(("Deck is in a folder your account can change, so it can't update itself. Download the setup from GitHub and run it.".to_string(), RETRY_SETUP_S)),
            Ok(4) => Err(("The setup didn't match its checksum, so it was thrown away.".to_string(), RETRY_FAILED_S)),
            Ok(c) => Err((format!("The update didn't start (code {c})."), RETRY_FAILED_S)),
            Err(_) => Err(("Admin rights were not given, so nothing was installed.".to_string(), RETRY_SETUP_S)),
        }
    })();
    // the copy that runs is in `updates` now (or nothing runs): the Temp folder isn't needed
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    // the setup closes Deck when it gets to it; still here once it is gone means it didn't install
    let mut sys = sysinfo::System::new();
    std::thread::sleep(Duration::from_secs(10));
    let started = Instant::now();
    while crate::restart::own_setup_running(&mut sys) && started.elapsed() < SETUP_MAX {
        std::thread::sleep(Duration::from_secs(3));
    }
    std::thread::sleep(AFTER_SETUP);
    Err(("The setup didn't finish. It may have been cancelled.".into(), RETRY_SETUP_S))
}

/// `deck.exe --install-update <setup> <checksum url> <version>`, started with admin rights by
/// `install_asking`. Nothing the caller says is trusted: the checksum is fetched again from this
/// repo's releases, the setup is copied into the admin-only `updates` and checked there, and only
/// then run. Exit codes: 0 started, 1 bad request, 2 no admin rights, 3 unsafe folder, 4 checksum.
pub fn install_helper(rest: &[String]) -> i32 {
    let (Some(src), Some(sha_url), Some(version)) = (rest.first(), rest.get(1), rest.get(2)) else { return 1 };
    if !crate::elevation::is_admin() {
        return 2;
    }
    if !sha_url.starts_with(FILES) || sha_url.contains("..") || !is_newer(version, env!("CARGO_PKG_VERSION")) {
        return 1;
    }
    let Ok(exe) = std::env::current_exe() else { return 1 };
    if crate::fsguard::user_could_change(&exe).unwrap_or(true) {
        return 3;
    }
    let src = Path::new(src);
    let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| n.ends_with("-setup.exe") && !n.contains(['/', '\\', ':'])) else { return 1 };
    let Some(want) = fetch_text(sha_url, 4096).ok().and_then(|t| parse_sha(&t)) else { return 4 };
    let Ok(dir) = updates_dir() else { return 1 };
    let setup = dir.join(&name);
    let copied = (|| -> std::io::Result<std::fs::File> {
        let mut from = std::fs::File::open(src)?.take(MAX_SETUP + 1);
        let mut to = std::fs::File::create(&setup)?;
        std::io::copy(&mut from, &mut to)?;
        to.flush()?;
        drop(to);
        hold(&setup)
    })();
    let Ok(mut held) = copied else {
        let _ = std::fs::remove_dir_all(&dir);
        return 1;
    };
    if sha256_of(&mut held).ok().as_deref() != Some(want.as_str()) {
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
        return 4;
    }
    use std::os::windows::process::CommandExt;
    let started = std::process::Command::new(&setup).args(["/S", "/UPDATE"]).creation_flags(crate::util::CREATE_NO_WINDOW).spawn();
    drop(held);
    // exits at once: the setup waits for every deck.exe to be gone before it installs
    if started.is_ok() {
        crate::log::info(format!("update {version}: the setup was started with admin rights"));
        0
    } else {
        1
    }
}

/// Downloads `r` and installs it. The setup closes this Deck itself; coming back from here means it
/// didn't, and the version isn't tried again by itself for a while.
fn install(app: &Arc<App>, r: &Release) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let silent = silent_now();
    set(app, |s| {
        s.state = "downloading";
        s.error = None;
        s.waiting = false;
        s.got = 0;
        s.size = 0;
    });
    let op = crate::log::Op::start("update", format!("install {} ({})", r.version, if silent { "without a prompt" } else { "asking for admin rights" }));
    let result = if silent { install_silent(app, r) } else { install_asking(app, r) };
    match &result {
        Ok(()) => op.ok("the setup runs; it closes this Deck"),
        Err((e, retry_after)) => op.fail(format!("{e} (tried again in {} min)", retry_after / 60)),
    }
    if let Err((e, retry_after)) = result {
        let mut saved = load();
        saved.failed = Some(r.version.clone());
        saved.retry_at = crate::util::now_s() + retry_after;
        store(&saved);
        set(app, |s| {
            s.state = "error";
            s.error = Some(e);
        });
    }
    BUSY.store(false, Ordering::SeqCst);
}

/// A game or fullscreen program is in front: not now.
fn busy_with_game(app: &App) -> bool {
    crate::boost::game_in_front(app).is_some() || crate::hotcorner::fullscreen_in_front()
}

/// Leftovers: the setup in `updates` (the update is over once a Deck starts) and Temp folders from
/// installs that asked for admin rights.
fn clean_up() {
    if crate::elevation::is_admin() {
        if let Ok(exe) = std::env::current_exe() {
            if !crate::fsguard::user_could_change(&exe).unwrap_or(true) {
                if let Some(d) = exe.parent() {
                    let _ = std::fs::remove_dir_all(d.join("updates"));
                }
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("deck-update-") && e.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

// ------------------------------------------------------------ entry points

/// "Check now": looks, and says what it found. Never installs by itself, and leaves an install
/// that is already running alone (it used to show "available" and the Install button again).
pub fn check_now(app: &Arc<App>) -> Result<Value, String> {
    if BUSY.load(Ordering::SeqCst) {
        return Ok(state());
    }
    set(app, |s| {
        s.state = "checking";
        s.error = None;
    });
    match check() {
        Ok(r) => {
            apply_found(app, r.as_ref());
            Ok(state())
        }
        Err(e) => {
            set(app, |s| {
                s.state = "error";
                s.error = Some(e.clone());
            });
            Err(e)
        }
    }
}

/// Shows what is known. An error about this same version stays (until "Check now" or the next
/// try), and nothing changes while an install runs.
fn apply_found(app: &App, found: Option<&Release>) {
    let checked = load().checked;
    let latest = found.map(|r| r.version.clone());
    let notes = found.map(|r| r.notes.clone()).unwrap_or_default();
    set_quiet(app, |s| {
        let keep = matches!(s.state, "downloading" | "installing") || (s.state == "error" && s.latest == latest);
        if !keep {
            s.state = if found.is_some() { "available" } else { "uptodate" };
        }
        if found.is_none() {
            s.waiting = false;
        }
        s.latest = latest;
        s.notes = notes;
        s.checked = checked;
    });
}

/// "Install now" (the page's button, the toast's button): the user asked, so a game in front doesn't
/// hold it back.
pub fn install_now(app: &Arc<App>) -> Result<(), String> {
    let r = load().release.filter(|r| is_newer(&r.version, env!("CARGO_PKG_VERSION"))).ok_or("There is no newer version to install.")?;
    let app = app.clone();
    std::thread::Builder::new().name("update-install".into()).spawn(move || install(&app, &r)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Starts the background look. Says "updated to X" once after an update.
pub fn start(app: &Arc<App>) {
    let version = env!("CARGO_PKG_VERSION");
    // a Deck is running again: whatever update closed the last one is over
    crate::restart::clear_updating();
    let mut saved = load();
    if saved.last_run.as_deref() != Some(version) {
        if saved.last_run.is_some() {
            app.toast(Toast { actions: vec![("ok".into(), "OK".into())], ..Toast::simple("update-done", &format!("Deck updated to {version}"), "The new version is running. What changed is on the Settings page under About.") });
        }
        saved.last_run = Some(version.into());
        store(&saved);
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            clean_up();
            std::thread::sleep(FIRST_LOOK);
            loop {
                let wait = tick(&app);
                std::thread::sleep(wait);
            }
        })
        .ok();
}

/// One round of the background loop; returns how long to sleep.
fn tick(app: &Arc<App>) -> Duration {
    if !app.settings.lock().auto_update {
        set_quiet(app, |s| s.waiting = false);
        return Duration::from_secs(600);
    }
    if BUSY.load(Ordering::SeqCst) {
        return POLL;
    }
    let saved = load();
    let now = crate::util::now_s();
    // what is already known is looked at again every minute (it may be waiting for a game); GitHub is
    // only asked once a day
    let mut release = saved.release.clone().filter(|r| is_newer(&r.version, env!("CARGO_PKG_VERSION")));
    if now.saturating_sub(saved.checked) >= CHECK_EVERY_S {
        match check() {
            Ok(r) => release = r,
            // logged by `check`
            Err(_) => return RETRY,
        }
    }
    apply_found(app, release.as_ref());
    let Some(r) = release else { return Duration::from_secs(CHECK_EVERY_S) };
    if saved.failed.as_deref() == Some(r.version.as_str()) && now < saved.retry_at {
        return Duration::from_secs((saved.retry_at - now).clamp(60, CHECK_EVERY_S / 4));
    }
    if !silent_ok() {
        // nothing is installed without the user: say so once per version
        if saved.told.as_deref() != Some(r.version.as_str()) {
            let mut s = load();
            s.told = Some(r.version.clone());
            store(&s);
            app.toast(Toast { actions: vec![("install".into(), "Install".into()), ("dismiss".into(), "Later".into())], sticky: true, ..Toast::simple("update", &format!("Deck {} is available", r.version), "Installing asks for admin rights once.") });
        }
        return Duration::from_secs(CHECK_EVERY_S / 4);
    }
    if busy_with_game(app) {
        if crate::log::every(&format!("update waits {}", r.version), Duration::from_secs(3600)) {
            crate::log::info(format!("update {}: waiting for the game in front to close", r.version));
        }
        set_quiet(app, |s| s.waiting = true);
        return POLL;
    }
    app.toast(Toast::simple("update", &format!("Updating Deck to {}", r.version), "It restarts by itself in a moment."));
    install(app, &r);
    POLL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("v2.5.11", "2.5.10"));
        assert!(is_newer("2.6.0", "2.5.10"));
        assert!(is_newer("3.0.0", "2.99.99"));
        assert!(!is_newer("2.5.10", "2.5.10"), "the same is not an update");
        assert!(!is_newer("2.5.9", "2.5.10"), "never a downgrade");
        assert!(!is_newer("v2.10.0", "2.10.1"));
        assert!(is_newer("2.10.0", "2.9.9"), "10 is more than 9");
    }

    #[test]
    fn only_plain_versions_count() {
        assert_eq!(parse_version("v2.5.10"), Some((2, 5, 10)));
        assert_eq!(parse_version("2.5.10-beta"), None);
        assert_eq!(parse_version("2.5"), None);
        assert_eq!(parse_version("2.5.10.1"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(!is_newer("2.6.0-rc1", "2.5.10"));
    }

    fn release(tag: &str, setup: &str, setup_url: &str, sha_url: &str) -> Value {
        json!({
            "tag_name": tag, "draft": false, "prerelease": false, "body": "- a fix",
            "assets": [
                { "name": setup, "browser_download_url": setup_url },
                { "name": format!("{setup}.sha256"), "browser_download_url": sha_url },
            ]
        })
    }

    #[test]
    fn release_notes_are_kept_whole_or_cut_at_a_line_end() {
        // 2.7.5's notes were 5,672 characters and ended "a release bi" on the Updates card
        let notes = "### Fixed\n".to_string() + &"- a fix that takes a while to explain\n".repeat(160);
        assert_eq!(notes_of(&notes), notes, "a long changelog is kept whole");
        let huge = "- line\n".repeat(5000);
        let kept = notes_of(&huge);
        assert!(kept.chars().count() <= NOTES_MAX);
        assert!(kept.ends_with("- line"), "never cut mid-line");
    }

    #[test]
    fn a_release_needs_its_setup_and_checksum_from_this_repo() {
        let ok = format!("{FILES}v2.6.0/Deck_2.6.0_x64-setup.exe");
        let sha = format!("{ok}.sha256");
        let r = pick(&release("v2.6.0", "Deck_2.6.0_x64-setup.exe", &ok, &sha)).unwrap();
        assert_eq!((r.version.as_str(), r.setup_name.as_str(), r.setup_url.as_str(), r.sha_url.as_str()), ("2.6.0", "Deck_2.6.0_x64-setup.exe", ok.as_str(), sha.as_str()));
        assert_eq!(r.notes, "- a fix");

        let elsewhere = "https://example.com/Deck_2.6.0_x64-setup.exe";
        assert!(pick(&release("v2.6.0", "Deck_2.6.0_x64-setup.exe", elsewhere, &sha)).is_none(), "a setup from another host");
        assert!(pick(&release("v2.6.0", "Deck_2.6.0_x64-setup.exe", &ok, "http://github.com/x.sha256")).is_none(), "a checksum from another place");
        assert!(pick(&release("nightly", "Deck_x64-setup.exe", &ok, &sha)).is_none(), "not a version");
        assert!(pick(&release("v2.6.0", "..\\Deck-setup.exe", &ok, &sha)).is_none(), "a path in the name");

        let mut draft = release("v2.6.0", "Deck_2.6.0_x64-setup.exe", &ok, &sha);
        draft["draft"] = json!(true);
        assert!(pick(&draft).is_none());
        let mut pre = release("v2.6.0", "Deck_2.6.0_x64-setup.exe", &ok, &sha);
        pre["prerelease"] = json!(true);
        assert!(pick(&pre).is_none());
        let no_sha = json!({ "tag_name": "v2.6.0", "draft": false, "prerelease": false, "assets": [ { "name": "Deck_x64-setup.exe", "browser_download_url": ok } ] });
        assert!(pick(&no_sha).is_none(), "no checksum, no update");
    }

    #[test]
    fn the_checksum_file_is_read_either_way() {
        let h = "a".repeat(64);
        assert_eq!(parse_sha(&format!("{h}  Deck_setup.exe\n")), Some(h.clone()));
        assert_eq!(parse_sha(&h.to_uppercase()), Some(h.clone()));
        assert_eq!(parse_sha("not a hash"), None);
        assert_eq!(parse_sha(&"a".repeat(63)), None);
        assert_eq!(parse_sha(""), None);
    }

    #[test]
    fn a_file_is_hashed_from_the_start() {
        let p = std::env::temp_dir().join(format!("deck-sha-test-{}", crate::util::random_token(8)));
        std::fs::write(&p, b"abc").unwrap();
        let mut f = std::fs::File::open(&p).unwrap();
        let _ = f.read(&mut [0u8; 1]);
        assert_eq!(sha256_of(&mut f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_file(&p).unwrap();
    }
}
