//! Deck's own log (`%LOCALAPPDATA%\Deck\logs\deck.log`): what went wrong in the background, where
//! nobody sees a console. Kept small (a few files of 2 MB); its end goes into the support report.
//!
//! A line: `2026-10-21 14:03:09.123Z [warn] [thread] area: what happened`. Levels: `error` (a
//! failure Deck couldn't carry on from, a crash), `warn` (something failed and Deck carried on),
//! `info` (worth knowing later: a change Deck made, an operation's outcome), `debug` (detail, off
//! unless a file named `debug` sits in the log folder or `DECK_LOG=debug` is set).
//!
//! Every line goes through one door, so the rules hold for all of them:
//! - secrets are masked (`token=…`, `password: …`, `Bearer …`) and the user's name is taken out
//!   of paths, whatever the caller passed;
//! - the same line again within a minute is counted instead of written (a failure in a polling
//!   loop can't fill the log), and the count is written once the minute is over;
//! - lines are written by a background thread, so a slow disk never holds up the caller; a crash
//!   or quit writes what is queued straight away (`flush`). A full queue drops lines and says so.
//!
//! `Op` times an operation from start to outcome under one id; `http_agent` / `http` log every
//! web request Deck makes (host and path only: no query, no body).

use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub struct Log {
    dir: PathBuf,
    max: u64,
    keep: usize,
    lock: Mutex<()>,
}

impl Log {
    /// A log in `dir`: a new file after `max` bytes, `keep` older files.
    pub fn new(dir: &Path, max: u64, keep: usize) -> Log {
        Log { dir: dir.to_path_buf(), max, keep, lock: Mutex::new(()) }
    }

    fn file(&self, i: usize) -> PathBuf {
        self.dir.join(if i == 0 { "deck.log".to_string() } else { format!("deck.{i}.log") })
    }

    #[cfg(test)]
    pub fn write(&self, level: &str, msg: &str) {
        let line = line(level, &thread_name(), msg);
        self.write_lines(&[line]);
    }

    /// Appends finished lines (each ends in CRLF), rotating first when they wouldn't fit.
    /// False when the file couldn't be written (the folder gone, the disk full).
    fn write_lines(&self, lines: &[String]) -> bool {
        if lines.is_empty() {
            return true;
        }
        let bytes: usize = lines.iter().map(String::len).sum();
        let _g = self.lock.lock();
        let _ = std::fs::create_dir_all(&self.dir);
        let now = std::fs::metadata(self.file(0)).map(|m| m.len()).unwrap_or(0);
        if now > 0 && now + bytes as u64 > self.max {
            let _ = std::fs::remove_file(self.file(self.keep));
            for i in (0..self.keep).rev() {
                let _ = std::fs::rename(self.file(i), self.file(i + 1));
            }
        }
        match std::fs::OpenOptions::new().create(true).append(true).open(self.file(0)) {
            Ok(mut f) => f.write_all(lines.concat().as_bytes()).is_ok(),
            Err(_) => false,
        }
    }

    /// The last `lines` lines (older files too, when the current one is shorter).
    pub fn tail(&self, lines: usize) -> String {
        let _g = self.lock.lock();
        let mut out: Vec<String> = Vec::new();
        for i in 0..=self.keep {
            let Ok(t) = std::fs::read_to_string(self.file(i)) else { break };
            let mut older: Vec<String> = t.lines().map(String::from).collect();
            older.append(&mut out);
            out = older;
            if out.len() >= lines {
                break;
            }
        }
        let skip = out.len().saturating_sub(lines);
        out[skip..].join("\r\n")
    }
}

/// One finished line: stamp, level, thread, the message on one line, secrets masked.
fn line(level: &str, thread: &str, msg: &str) -> String {
    let (s, ms) = {
        let t = crate::util::now_ms();
        (t / 1000, t % 1000)
    };
    format!("{}.{ms:03}Z [{level}] [{thread}] {}\r\n", stamp(s).trim_end_matches('Z'), redact(&msg.replace(['\r', '\n'], " | ")))
}

fn thread_name() -> String {
    let t = std::thread::current();
    let name = t.name().map(str::to_string).unwrap_or_else(|| format!("{:?}", t.id()).replace("ThreadId(", "t").replace(')', ""));
    match ROLE.get() {
        Some(role) => format!("{role}/{name}"),
        None => name,
    }
}

/// What this process is when it isn't Deck itself (the watchdog, the update helper): its lines
/// say so, since they share the log with the running Deck.
static ROLE: OnceLock<&'static str> = OnceLock::new();

pub fn set_role(role: &'static str) {
    let _ = ROLE.set(role);
}

/// "2026-10-17 14:03:09Z" (UTC) for unix seconds.
fn stamp(s: u64) -> String {
    let days = (s / 86400) as i64;
    // civil date from days since 1970 (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let t = s % 86400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

// --------------------------------------------------------------------- privacy

/// Words after which a value is a secret: `token=abc`, `"password": "x"`, `Bearer abc`.
const SECRET_KEYS: &[&str] = &["token", "password", "passwd", "secret", "apikey", "api_key", "api-key", "authorization", "bearer", "pairing code", "sync key", "phone key"];

/// `msg` with secrets masked and the user's name taken out of paths. Used on every log line and
/// by the support report.
pub fn redact(msg: &str) -> String {
    private(&mask_secrets(msg))
}

/// Masks the value after each secret word: the next run of characters that a token is made of,
/// when it is at least 4 long (so "token expired" or "password: none" read as they are).
fn mask_secrets(msg: &str) -> String {
    let lower = msg.to_ascii_lowercase();
    let b = msg.as_bytes();
    let mut cuts: Vec<(usize, usize)> = vec![];
    for key in SECRET_KEYS {
        let mut from = 0;
        while let Some(p) = lower[from..].find(key) {
            let mut i = from + p + key.len();
            from = i;
            // `token=`, `token: `, `"token": "`, `Bearer `
            while i < b.len() && matches!(b[i], b'"' | b'\'' | b' ' | b'\t') {
                i += 1;
            }
            let sep = i < b.len() && matches!(b[i], b'=' | b':');
            if sep {
                i += 1;
            } else if *key != "bearer" && *key != "authorization" {
                continue;
            }
            while i < b.len() && matches!(b[i], b'"' | b'\'' | b' ' | b'\t') {
                i += 1;
            }
            // `Authorization: Bearer x`: the scheme stays readable, "bearer" masks what follows
            if *key == "authorization" && ["bearer ", "basic "].iter().any(|s| lower[i..].starts_with(s)) {
                continue;
            }
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b'_' | b'.' | b'+' | b'/' | b'=' | b'~')) {
                i += 1;
            }
            if i - start >= 4 && !msg[start..i].eq_ignore_ascii_case("none") {
                cuts.push((start, i));
            }
        }
    }
    if cuts.is_empty() {
        return msg.to_string();
    }
    cuts.sort();
    let mut out = String::with_capacity(msg.len());
    let mut at = 0;
    for (s, e) in cuts {
        if s < at {
            continue;
        }
        out.push_str(&msg[at..s]);
        out.push_str("***");
        at = e;
    }
    out.push_str(&msg[at..]);
    out
}

/// The user's name out of paths (C:\Users\<name> → %USERPROFILE%), and the PC's name.
pub fn private(s: &str) -> String {
    let mut out = s.to_string();
    if let Ok(home) = std::env::var("USERPROFILE") {
        if !home.is_empty() {
            out = replace_ci(&out, &home, "%USERPROFILE%");
        }
    }
    if let Ok(user) = std::env::var("USERNAME") {
        if user.len() > 2 {
            out = replace_ci(&out, &format!("\\{user}\\"), "\\<user>\\");
        }
    }
    // the PC's name too (Transfer names the PC it runs on)
    if let Ok(pc) = std::env::var("COMPUTERNAME") {
        if pc.len() > 2 {
            out = replace_word_ci(&out, &pc, "<pc>");
        }
    }
    out
}

/// `replace_ci`, only where `needle` is a whole word (letters, digits, '-' and '_'): a PC named
/// "Deck" leaves "Decks" and "deck-deaths" alone.
fn replace_word_ci(hay: &str, needle: &str, with: &str) -> String {
    let (lh, ln) = (hay.to_lowercase(), needle.to_lowercase());
    if ln.is_empty() || lh.len() != hay.len() {
        return hay.to_string();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = lh[i..].find(&ln) {
        let (a, b) = (i + p, i + p + ln.len());
        out.push_str(&hay[i..a]);
        out.push_str(if word(hay[..a].chars().next_back()) || word(hay[b..].chars().next()) { &hay[a..b] } else { with });
        i = b;
    }
    out.push_str(&hay[i..]);
    out
}

fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    let (lh, ln) = (hay.to_lowercase(), needle.to_lowercase());
    if ln.is_empty() || lh.len() != hay.len() {
        return hay.replace(needle, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = lh[i..].find(&ln) {
        out.push_str(&hay[i..i + p]);
        out.push_str(with);
        i += p + ln.len();
    }
    out.push_str(&hay[i..]);
    out
}

// --------------------------------------------------------------------- the one door

/// How long the same line is counted instead of written again.
const REPEAT_WINDOW: Duration = Duration::from_secs(60);
/// Lines waiting for the writer; past this they are dropped (and counted).
const QUEUE_MAX: usize = 4000;

struct Repeat {
    first: Instant,
    level: &'static str,
    count: u32,
}

#[derive(Default)]
struct Queue {
    lines: VecDeque<String>,
    dropped: u64,
    /// message → when it was last written and how often it came again since
    repeats: HashMap<String, Repeat>,
    writer: bool,
}

struct Global {
    log: Log,
    queue: Mutex<Queue>,
    cv: Condvar,
    /// held from taking lines off the queue until they are written, so lines land in order
    order: Mutex<()>,
    /// writes that failed in a row (the folder unwritable): said once it works again
    failed: AtomicU64,
}

static DEBUG: AtomicBool = AtomicBool::new(false);

static G: OnceLock<Global> = OnceLock::new();

fn global() -> &'static Global {
    G.get_or_init(|| {
        let dir = dir();
        DEBUG.store(std::env::var("DECK_LOG").map(|v| v.eq_ignore_ascii_case("debug")).unwrap_or(false) || dir.join("debug").exists(), Ordering::Relaxed);
        Global { log: Log::new(&dir, 2 << 20, 4), queue: Mutex::new(Queue::default()), cv: Condvar::new(), order: Mutex::new(()), failed: AtomicU64::new(0) }
    })
}

/// Detail is being logged (`debug` lines): callers can skip building costly messages.
pub fn debug_on() -> bool {
    let _ = global();
    DEBUG.load(Ordering::Relaxed)
}

fn put(level: &'static str, msg: &str) {
    let g = global();
    let text = line(level, &thread_name(), msg);
    // never wait long: a crash handler can log while the thread that crashed held the queue
    let Some(mut guard) = g.queue.try_lock_for(Duration::from_millis(500)) else { return };
    // the queue itself, so its fields can be borrowed apart
    let q: &mut Queue = &mut guard;
    // the same message (whatever the time and thread) within the window: counted, not written
    let key = format!("{level} {msg}");
    let now = Instant::now();
    if let Some(r) = q.repeats.get_mut(&key) {
        if now.duration_since(r.first) < REPEAT_WINDOW {
            r.count += 1;
            return;
        }
        // its window is over: what it counted is said before the line starts a new one
        if r.count > 0 {
            let said = repeat_line(&key, r, now);
            q.lines.push_back(said);
        }
    }
    if q.repeats.len() > 512 {
        q.repeats.retain(|_, r| now.duration_since(r.first) < REPEAT_WINDOW || r.count > 0);
        if q.repeats.len() > 2048 {
            // a flood of different lines: their counts are lost rather than the map growing on
            q.repeats.retain(|_, r| r.count > 0);
        }
    }
    q.repeats.insert(key, Repeat { first: now, level, count: 0 });
    if q.lines.len() >= QUEUE_MAX {
        q.dropped += 1;
        return;
    }
    q.lines.push_back(text);
    if cfg!(test) {
        // tests read the log right after writing it
        let lines: Vec<String> = q.lines.drain(..).collect();
        drop(guard);
        g.log.write_lines(&lines);
        return;
    }
    if !q.writer {
        q.writer = true;
        if std::thread::Builder::new().name("log".into()).spawn(writer).is_err() {
            // no thread: write here, as before
            q.writer = false;
            let lines: Vec<String> = q.lines.drain(..).collect();
            drop(guard);
            g.log.write_lines(&lines);
            return;
        }
    }
    g.cv.notify_one();
}

/// Lines for the repeats whose window is over (or all of them when `all`), their counts cleared.
fn repeat_lines(q: &mut Queue, all: bool) -> Vec<String> {
    let now = Instant::now();
    let mut out = vec![];
    for (key, r) in q.repeats.iter_mut() {
        if r.count > 0 && (all || now.duration_since(r.first) >= REPEAT_WINDOW) {
            out.push(repeat_line(key, r, now));
            r.count = 0;
        }
    }
    q.repeats.retain(|_, r| now.duration_since(r.first) < REPEAT_WINDOW || r.count > 0);
    out
}

/// "(the line "…" came 41 more times in 60 s)", at the level of the line it counts.
fn repeat_line(key: &str, r: &Repeat, now: Instant) -> String {
    let msg = key.split_once(' ').map(|x| x.1).unwrap_or(key);
    let short: String = msg.chars().take(160).collect();
    line(r.level, &thread_name(), &format!("(the line \"{short}\" came {} more time{} in {} s)", r.count, if r.count == 1 { "" } else { "s" }, now.duration_since(r.first).as_secs()))
}

fn take(q: &mut Queue, all_repeats: bool) -> Vec<String> {
    let mut lines: Vec<String> = q.lines.drain(..).collect();
    lines.extend(repeat_lines(q, all_repeats));
    if q.dropped > 0 {
        lines.push(line("warn", "log", &format!("log: {} lines were dropped (written faster than the disk took them)", q.dropped)));
        q.dropped = 0;
    }
    lines
}

fn writer() {
    let g = global();
    loop {
        {
            let mut q = g.queue.lock();
            if q.lines.is_empty() {
                g.cv.wait_for(&mut q, Duration::from_secs(20));
            }
        }
        let _o = g.order.lock();
        let lines = take(&mut g.queue.lock(), false);
        write_out(g, &lines);
    }
}

fn write_out(g: &Global, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    if g.log.write_lines(lines) {
        let n = g.failed.swap(0, Ordering::Relaxed);
        if n > 0 {
            g.log.write_lines(&[line("warn", "log", &format!("log: {n} earlier writes failed (the log folder couldn't be written); their lines are lost"))]);
        }
    } else {
        g.failed.fetch_add(1, Ordering::Relaxed);
    }
}

/// Writes everything queued now, on this thread (a crash, a quit, the support report). Waits at
/// most a moment for the queue: a crash inside the logger must not hang the crash handler.
pub fn flush() {
    // nothing logged yet: nothing to set up (Claude Code's hook process exits through here)
    let Some(g) = G.get() else { return };
    let Some(_o) = g.order.try_lock_for(Duration::from_millis(300)) else { return };
    let Some(mut q) = g.queue.try_lock_for(Duration::from_millis(300)) else { return };
    let lines = take(&mut q, true);
    drop(q);
    write_out(g, &lines);
}

/// A failure Deck couldn't carry on from (a crash, a worker that died), or one that leaves
/// something important not working. Written at once, so it survives the process ending.
pub fn error(msg: impl AsRef<str>) {
    put("error", msg.as_ref());
    flush();
}

/// Something went wrong that Deck carried on from.
pub fn warn(msg: impl AsRef<str>) {
    put("warn", msg.as_ref());
}

/// Something worth knowing later (a system change Deck made, an operation's outcome).
pub fn info(msg: impl AsRef<str>) {
    put("info", msg.as_ref());
}

/// Detail for tracking a problem down: written only while debug logging is on.
pub fn debug(msg: impl AsRef<str>) {
    if debug_on() {
        put("debug", msg.as_ref());
    }
}

/// `debug` with the message built only when it will be written.
pub fn debug_with(msg: impl FnOnce() -> String) {
    if debug_on() {
        put("debug", &msg());
    }
}

/// True once per `every` for `key`: for logging a state that keeps coming back (a sensor that
/// keeps failing) once in a while, with lines that differ each time (and so aren't counted).
pub fn every(key: &str, every: Duration) -> bool {
    static LAST: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let mut g = LAST.lock();
    let m = g.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    if m.get(key).is_some_and(|t| now.duration_since(*t) < every) {
        return false;
    }
    if m.len() > 512 {
        m.retain(|_, t| now.duration_since(*t) < every);
    }
    m.insert(key.to_string(), now);
    true
}

/// The log's last `lines` lines.
pub fn tail(lines: usize) -> String {
    let g = global();
    flush();
    g.log.tail(lines)
}

/// Where the log lives: `%LOCALAPPDATA%\Deck\logs` (`Deck-<profile>` for a test profile).
pub fn dir() -> PathBuf {
    // the tests' warnings stay out of the real log
    if cfg!(test) {
        return std::env::temp_dir().join("deck-test-logs");
    }
    let name = crate::settings::profile().map(|p| format!("Deck-{p}")).unwrap_or_else(|| "Deck".into());
    crate::util::local_appdata().join(name).join("logs")
}

/// An error with what caused it, each cause after a colon ("couldn't connect: timed out").
pub fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let t = c.to_string();
        if !s.contains(&t) {
            s.push_str(": ");
            s.push_str(&t);
        }
        cur = c.source();
    }
    s
}

// --------------------------------------------------------------------- operations

static NEXT_OP: AtomicU64 = AtomicU64::new(1);

/// One operation from start to outcome (an update, a profile switch, a download), under an id
/// that its lines share: `area: what started (#12)` (debug), then one of `done in 340 ms (#12)`,
/// `failed after 2.1 s (#12): why`, or, when it was dropped without either (the thread failed, a
/// `?` returned early), `ended without an outcome`.
pub struct Op {
    area: &'static str,
    what: String,
    id: u64,
    start: Instant,
    ended: bool,
}

impl Op {
    pub fn start(area: &'static str, what: impl Into<String>) -> Op {
        let op = Op { area, what: what.into(), id: NEXT_OP.fetch_add(1, Ordering::Relaxed), start: Instant::now(), ended: false };
        debug_with(|| format!("{}: {} started (#{})", op.area, op.what, op.id));
        op
    }

    #[cfg(test)]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Finished well; `detail` says with what ("" for nothing).
    pub fn ok(mut self, detail: impl AsRef<str>) {
        self.ended = true;
        let d = detail.as_ref();
        info(format!("{}: {} done in {} (#{}){}{}", self.area, self.what, took(self.start.elapsed()), self.id, if d.is_empty() { "" } else { ": " }, d));
    }

    /// Finished well, routine enough for debug only (it runs often).
    pub fn ok_quiet(mut self, detail: impl AsRef<str>) {
        self.ended = true;
        debug_with(|| format!("{}: {} done in {} (#{}): {}", self.area, self.what, took(self.start.elapsed()), self.id, detail.as_ref()));
    }

    /// Failed; Deck carries on.
    pub fn fail(mut self, why: impl std::fmt::Display) {
        self.ended = true;
        warn(format!("{}: {} failed after {} (#{}): {why}", self.area, self.what, took(self.start.elapsed()), self.id));
    }

    /// Didn't happen on purpose (the user cancelled, nothing to do): not a failure.
    pub fn skipped(mut self, why: impl std::fmt::Display) {
        self.ended = true;
        info(format!("{}: {} not done (#{}, {}): {why}", self.area, self.what, self.id, took(self.start.elapsed())));
    }

}

impl Drop for Op {
    fn drop(&mut self) {
        if !self.ended {
            let why = if std::thread::panicking() { "its thread panicked" } else { "returned early" };
            warn(format!("{}: {} ended without an outcome after {} (#{}): {why}", self.area, self.what, took(self.start.elapsed()), self.id));
        }
    }
}

/// "340 ms", "2.1 s", "3 min 05 s".
pub fn took(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {:02} s", ms / 60_000, ms / 1000 % 60)
    }
}

// --------------------------------------------------------------------- web requests

/// `b` with every request logged: failures (no answer: warn; an error status: info) and, with
/// debug logging, every answer. Only the method, host and path go in the log.
pub fn http(b: ureq::AgentBuilder) -> ureq::AgentBuilder {
    b.middleware(http_logger)
}

/// A shared agent with ureq's defaults and the request log, for one-off requests
/// (`ureq::get` uses the same defaults without the log).
pub fn http_agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| http(ureq::AgentBuilder::new()).build())
}

// the signature is ureq's middleware's (its `Error` is large, not ours to box)
#[allow(clippy::result_large_err)]
fn http_logger(req: ureq::Request, next: ureq::MiddlewareNext) -> Result<ureq::Response, ureq::Error> {
    let (method, url) = (req.method().to_string(), safe_url(req.url()));
    let t = Instant::now();
    let r = next.handle(req);
    let ms = took(t.elapsed());
    match &r {
        Ok(resp) => debug_with(|| format!("http: {method} {url} answered {} in {ms}", resp.status())),
        Err(ureq::Error::Status(code, _)) => info(format!("http: {method} {url} answered {code} in {ms}")),
        Err(e) => warn(format!("http: {method} {url} failed after {ms}: {}", chain(e))),
    }
    r
}

/// `scheme://host/path` of `url`: no user, query or fragment (they can carry keys).
pub fn safe_url(url: &str) -> String {
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    let u = &url[..cut];
    match u.split_once("://") {
        Some((scheme, rest)) => {
            let rest = rest.rsplit_once('@').filter(|(user, _)| !user.contains('/')).map(|x| x.1).unwrap_or(rest);
            format!("{scheme}://{rest}")
        }
        None => u.to_string(),
    }
}

// --------------------------------------------------------------------- the start

/// The first line of a run: which Deck, on what, started how. Arguments are named, not copied
/// (one carries an update's file path).
pub fn session_start() {
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a.starts_with("--")).collect();
    info(format!(
        "Deck {} started (process {}, admin: {}, profile: {}, arguments: {}, debug log: {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        crate::elevation::is_admin(),
        crate::settings::profile().unwrap_or_else(|| "normal".into()),
        if args.is_empty() { "none".to_string() } else { args.join(" ") },
        if debug_on() { "on" } else { "off" }
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_kept_in_order_and_old_files_rotate_out() {
        let dir = std::env::temp_dir().join(format!("deck-log-{}", crate::util::now_ms()));
        let log = Log::new(&dir, 600, 2);
        for i in 0..100 {
            log.write("warn", &format!("line {i:03}"));
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["deck.1.log", "deck.2.log", "deck.log"], "the current file and two older ones");
        let size: u64 = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
        assert!(size < 3 * 700, "small: {size} bytes");
        let t = log.tail(10);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 10);
        assert!(lines.last().unwrap().ends_with("line 099"), "{t}");
        assert!(lines.last().unwrap().contains("[warn] ["), "level and thread: {t}");
        assert!(lines[0].ends_with("line 090"), "across the rotation, in order: {t}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unwritable_log_folder_is_carried_on_from() {
        // a file where the folder should be: nothing can be written, nothing panics
        let f = std::env::temp_dir().join(format!("deck-log-file-{}", crate::util::now_ms()));
        std::fs::write(&f, b"x").unwrap();
        let log = Log::new(&f, 600, 2);
        log.write("warn", "lost");
        assert!(!log.write_lines(&["x\r\n".into()]));
        assert_eq!(log.tail(5), "");
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(super::stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(super::stamp(1_792_259_589), "2026-10-17 17:53:09Z");
        let l = line("info", "main", "a\nb");
        assert!(l.contains("Z [info] [main] a | b\r\n"), "one line, milliseconds, level, thread: {l}");
    }

    #[test]
    fn secrets_are_masked_whatever_the_caller_wrote() {
        assert_eq!(mask_secrets("remote token=Ab3dEf9h12 ok"), "remote token=*** ok");
        assert_eq!(mask_secrets(r#"{"password": "hunter22", "name": "x"}"#), r#"{"password": "***", "name": "x"}"#);
        assert_eq!(mask_secrets("Authorization: Bearer eyJhbGciOi.x-y_z"), "Authorization: Bearer ***");
        assert_eq!(mask_secrets("https://x.io/a?api_key=12345678&b=2"), "https://x.io/a?api_key=***&b=2");
        assert_eq!(mask_secrets("Token: none"), "Token: none");
        assert_eq!(mask_secrets("the phone key changed"), "the phone key changed", "no value: nothing to mask");
        assert_eq!(mask_secrets("pairing code: 4821-9931"), "pairing code: ***");
    }

    #[test]
    fn the_user_name_is_taken_out_of_paths() {
        let home = std::env::var("USERPROFILE").unwrap();
        assert_eq!(redact(&format!("{}\\AppData\\x token=abcdefgh", home.to_uppercase())), "%USERPROFILE%\\AppData\\x token=***");
    }

    #[test]
    fn the_pc_name_is_taken_out_as_a_whole_word() {
        assert_eq!(replace_word_ci("PC to PC started on DESK-1 (desk-1.local)", "desk-1", "<pc>"), "PC to PC started on <pc> (<pc>.local)");
        assert_eq!(replace_word_ci("Decks and deck-deaths, deck.exe", "deck", "<pc>"), "Decks and deck-deaths, <pc>.exe");
        if let Ok(pc) = std::env::var("COMPUTERNAME") {
            assert_eq!(redact(&format!("transfer: started on {pc}")), "transfer: started on <pc>");
        }
    }

    #[test]
    fn urls_lose_their_query_and_user() {
        assert_eq!(safe_url("https://api.github.com/repos/a/b/releases?per_page=5#x"), "https://api.github.com/repos/a/b/releases");
        assert_eq!(safe_url("https://me:pw@host.io/p"), "https://host.io/p");
        assert_eq!(safe_url("http://127.0.0.1:7878/api/add"), "http://127.0.0.1:7878/api/add");
    }

    #[test]
    fn the_same_line_again_is_counted_not_written() {
        let mut q = Queue::default();
        let now = Instant::now();
        q.repeats.insert("warn sensor failed".into(), Repeat { first: now - REPEAT_WINDOW - Duration::from_secs(1), level: "warn", count: 41 });
        q.repeats.insert("warn other".into(), Repeat { first: now, level: "warn", count: 2 });
        let out = repeat_lines(&mut q, false);
        assert_eq!(out.len(), 1, "only the window that is over: {out:?}");
        assert!(out[0].contains("\"sensor failed\" came 41 more times"), "{out:?}");
        assert_eq!(repeat_lines(&mut q, true).len(), 1, "a flush writes the rest");
        assert!(repeat_lines(&mut q, true).is_empty(), "each count once");
    }

    #[test]
    fn a_full_queue_says_how_many_lines_it_dropped() {
        let mut q = Queue { dropped: 7, ..Default::default() };
        q.lines.push_back("a\r\n".into());
        let out = take(&mut q, false);
        assert_eq!(out.len(), 2);
        assert!(out[1].contains("7 lines were dropped"), "{out:?}");
        assert_eq!(q.dropped, 0);
    }

    #[test]
    fn every_lets_one_through_per_window() {
        let k = format!("k{}", crate::util::now_ms());
        assert!(every(&k, Duration::from_secs(60)));
        assert!(!every(&k, Duration::from_secs(60)));
        assert!(every(&format!("{k}b"), Duration::from_secs(60)));
    }

    #[test]
    fn an_operation_logs_its_outcome_with_its_id() {
        let op = Op::start("test", "thing");
        let id = op.id();
        op.fail("it broke: disk full");
        // other tests write to the same test log at the same time
        let t = tail(2000);
        assert!(t.lines().any(|l| l.contains("test: thing failed after ") && l.contains(&format!("(#{id}): it broke: disk full"))), "{t}");
        let id = {
            let op = Op::start("test", "early return");
            op.id()
        };
        let t = tail(2000);
        assert!(t.lines().any(|l| l.contains("test: early return ended without an outcome") && l.contains(&format!("(#{id}): returned early"))), "a dropped operation is visible: {t}");
    }

    #[test]
    fn error_chains_keep_their_causes() {
        let e = std::io::Error::other(std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"));
        let s = chain(&e);
        assert!(s.contains("timed out"), "{s}");
        assert_eq!(took(Duration::from_millis(340)), "340 ms");
        assert_eq!(took(Duration::from_millis(2100)), "2.1 s");
        assert_eq!(took(Duration::from_secs(185)), "3 min 05 s");
    }
}
