//! Tweaks (Maintenance → Tweaks): Windows settings with their live state and an exact undo.
//!
//! The recipes are `tweaks/tweaks.json`, built by `tools/tweaks/build.py` from Win11Debloat and
//! WinUtil (MIT) plus Deck's own (design: `docs/design/specs/2026-10-20-tweaks-design.md`).
//! Every tweak is a feature switch or a choice; its state is read from the PC each time. Before a
//! step changes anything, what was there goes into the journal (`%APPDATA%\Deck\tweaks.json`), so
//! undo puts back the PC's own old value, even months later. A tweak Windows changed back (a
//! feature update) is noticed and listed.

pub mod apps;
pub mod steps;
pub mod storedl;

use crate::app::App;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use steps::{Before, Step};

const DATA: &str = include_str!("../../tweaks/tweaks.json");

#[derive(Deserialize, Clone)]
pub struct Opt {
    pub id: String,
    pub label: String,
    pub steps: Vec<Step>,
}

#[derive(Deserialize, Clone)]
pub struct Tweak {
    pub id: String,
    pub cat: String,
    #[serde(default)]
    pub group: Option<String>,
    pub name: String,
    pub what: String,
    #[serde(default)]
    pub more: Option<String>,
    pub risk: String,
    pub after: String,
    pub src: String,
    #[serde(default)]
    pub min: Option<u32>,
    #[serde(default)]
    pub max: Option<u32>,
    #[serde(default)]
    pub needs: Vec<String>,
    pub kind: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub rec: Option<String>,
    #[serde(default)]
    pub options: Vec<Opt>,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub page: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, rename = "do")]
    pub action: Option<String>,
    /// a switch, worded as what Deck does when it's ticked (ticked = away from Windows' default)
    #[serde(default)]
    pub tick: Option<String>,
    /// plain words: what you gain with Deck's change, and what you give up
    #[serde(default)]
    pub good: Option<String>,
    #[serde(default)]
    pub lose: Option<String>,
}

#[derive(Deserialize)]
struct Data {
    cats: Vec<Value>,
    presets: Vec<Value>,
    tweaks: Vec<Tweak>,
}

fn data() -> &'static Data {
    static D: std::sync::OnceLock<Data> = std::sync::OnceLock::new();
    D.get_or_init(|| serde_json::from_str(DATA).expect("tweaks.json"))
}

pub fn find(id: &str) -> Option<&'static Tweak> {
    data().tweaks.iter().find(|t| t.id == id)
}

/// Runs a console program hidden, ending it after `secs` (DISM and PowerShell can hang; nothing
/// here may wait forever).
pub(crate) fn run(secs: u64, exe: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    crate::util::run_for(secs, exe, args)
}

// ------------------------------------------------------------------------------------- journal

/// One tweak Deck changed: what it set and what was there before (the first time it changed it).
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Entry {
    pub id: String,
    /// the option Deck set last
    pub applied: String,
    /// the option the PC was at before Deck's first change ("" when it matched none)
    pub was: String,
    pub at: u64,
    pub before: Vec<Before>,
    /// an action's own undo data (e.g. the Start layout file it replaced)
    #[serde(default)]
    pub backup: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Journal {
    pub entries: Vec<Entry>,
    /// built-in apps Deck removed (for reinstall)
    #[serde(default)]
    pub removed_apps: Vec<apps::Removed>,
}

static JOURNAL: Mutex<Option<Journal>> = parking_lot::const_mutex(None);

fn journal_path() -> std::path::PathBuf {
    crate::settings::Settings::dir().join("tweaks.json")
}

/// The journal's name in `seal` (its fingerprint is kept where only administrators can write).
const SEAL: &str = "tweaks-journal";
/// The journal was changed outside Deck and set aside (`tweaks.changed-outside.json`).
static SET_ASIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The journal as Deck last saved it. Undo writes its before-values back with Deck's admin rights,
/// and the file is in the user's folder, where any of their programs can edit it: one whose
/// fingerprint isn't the one Deck recorded is set aside and never acted on.
fn load_journal() -> Journal {
    let Ok(bytes) = std::fs::read(journal_path()) else { return Journal::default() };
    if crate::seal::check(SEAL, &bytes) == crate::seal::Seal::Changed {
        let aside = journal_path().with_file_name("tweaks.changed-outside.json");
        let _ = std::fs::remove_file(&aside);
        let _ = std::fs::rename(journal_path(), &aside);
        SET_ASIDE.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::log::warn("tweaks: the undo list was changed outside Deck; it was set aside and isn't used");
        return Journal::default();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn with_journal<R>(f: impl FnOnce(&mut Journal) -> R) -> R {
    let mut j = JOURNAL.lock();
    if j.is_none() {
        *j = Some(load_journal());
    }
    f(j.as_mut().unwrap())
}

pub(crate) fn save_journal() {
    with_journal(|j| {
        if let Ok(t) = serde_json::to_vec_pretty(j) {
            match crate::util::write_atomic(&journal_path(), &t) {
                Ok(()) => crate::seal::record(SEAL, &t),
                Err(e) => crate::log::warn(format!("tweaks: the journal couldn't be saved: {e}")),
            }
        }
    });
}

/// Defence in depth for undo: a before-value may only touch what this tweak's own recipe touches
/// (so even a forged journal entry can't reach other registry keys, services or features).
fn before_allowed(t: &Tweak, b: &Before) -> bool {
    let steps = || t.options.iter().flat_map(|o| o.steps.iter());
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    match b {
        Before::Value { path, name, .. } => steps().any(|s| {
            s.reg.as_deref().is_some_and(|r| eq(r, path)) && !s.key_delete && !s.key_create && eq(s.name.as_deref().unwrap_or(""), name)
                || s.dx.is_some() && eq(path, steps::DX_KEY) && eq(name, steps::DX_NAME)
        }),
        Before::Key { path, .. } => steps().any(|s| (s.key_delete || s.key_create) && s.reg.as_deref().is_some_and(|r| eq(r, path))),
        // the first key on a step's path that wasn't there
        Before::Created { path } => steps().any(|s| s.reg.as_deref().is_some_and(|r| eq(r, path) || r.to_ascii_lowercase().starts_with(&format!("{}\\", path.to_ascii_lowercase())))),
        Before::Service { name, .. } => steps().any(|s| s.service.as_deref().is_some_and(|n| eq(n, name))),
        Before::Feature { name, .. } => steps().any(|s| s.feature.as_deref().is_some_and(|n| eq(n, name))),
        Before::Cmd { name, .. } => steps().any(|s| s.cmd.as_deref().is_some_and(|n| eq(n, name))),
        Before::Dns { .. } => steps().any(|s| s.dns.is_some()),
        Before::Dx { key, .. } => steps().any(|s| s.dx.as_deref().is_some_and(|k| eq(k, key))),
    }
}

// ------------------------------------------------------------------------------------- slow reads

/// DISM and PowerShell answers take seconds: kept for 5 minutes, read in the background.
static SLOW: Mutex<Option<HashMap<String, (Instant, Option<bool>)>>> = parking_lot::const_mutex(None);
static SLOW_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn slow(key: &str) -> Option<bool> {
    let fresh = SLOW.lock().as_ref().and_then(|m| m.get(key).filter(|(t, _)| t.elapsed() < Duration::from_secs(300)).map(|(_, v)| *v));
    match fresh {
        Some(v) => v,
        None => {
            refresh_slow();
            None
        }
    }
}

pub(crate) fn forget_slow(key: &str) {
    if let Some(m) = SLOW.lock().as_mut() {
        m.remove(key);
    }
}

fn read_slow(key: &str) -> Option<bool> {
    let (kind, name) = key.split_once(':')?;
    // DISM needs admin rights (without them it can sit and wait)
    if !crate::elevation::is_admin() && !crate::settings::is_test_profile() && (kind == "feature" || name == "reserved_storage") {
        return None;
    }
    match kind {
        "feature" => steps::feature_on(name),
        "cmd" => steps::cmd_on(name),
        _ => None,
    }
}

fn refresh_slow() {
    if SLOW_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let mut keys: Vec<String> = Vec::new();
    for t in &data().tweaks {
        for o in &t.options {
            for s in &o.steps {
                if let Some(f) = &s.feature {
                    keys.push(format!("feature:{f}"));
                }
                if let Some(c) = &s.cmd {
                    keys.push(format!("cmd:{c}"));
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    std::thread::spawn(move || {
        for k in keys {
            let stale = SLOW.lock().as_ref().and_then(|m| m.get(&k)).is_none_or(|(t, _)| t.elapsed() >= Duration::from_secs(300));
            if stale {
                let v = read_slow(&k);
                SLOW.lock().get_or_insert_with(HashMap::new).insert(k, (Instant::now(), v));
            }
        }
        SLOW_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

static IFACE: Mutex<Option<(Instant, Option<(String, u32)>)>> = parking_lot::const_mutex(None);

/// The connection in use, asked at most once a minute.
pub(crate) fn iface() -> Option<(String, u32)> {
    let mut c = IFACE.lock();
    if let Some((t, v)) = c.as_ref() {
        if t.elapsed() < Duration::from_secs(60) {
            return v.clone();
        }
    }
    let v = steps::dns_iface();
    *c = Some((Instant::now(), v.clone()));
    v
}

// ------------------------------------------------------------------------------------- the PC

pub fn build() -> u32 {
    steps::read_value(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber")
        .and_then(|d| {
            let b = crate::util::unbase64(&d.data)?;
            let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|c| *c != 0).collect();
            String::from_utf16_lossy(&u).trim().parse().ok()
        })
        .unwrap_or_else(|| if crate::settings::is_test_profile() { 26100 } else { 0 })
}

fn has_battery() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).is_ok() && s.BatteryFlag != 128 && s.BatteryFlag != 255 }
}

fn app_installed(name: &str) -> bool {
    let pf = crate::util::program_files();
    let pf86 = crate::util::program_files_x86();
    let la = crate::util::local_appdata();
    let paths: &[std::path::PathBuf] = &match name {
        "chrome" => [pf.join(r"Google\Chrome\Application\chrome.exe"), pf86.join(r"Google\Chrome\Application\chrome.exe"), la.join(r"Google\Chrome\Application\chrome.exe")],
        "brave" => [pf.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"), pf86.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"), la.join(r"BraveSoftware\Brave-Browser\Application\brave.exe")],
        _ => return false,
    };
    paths.iter().any(|p| p.exists())
}

fn edition_pro() -> bool {
    let id = steps::read_value(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "EditionID")
        .and_then(|d| crate::util::unbase64(&d.data))
        .map(|b| String::from_utf16_lossy(&b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|c| *c != 0).collect::<Vec<u16>>()))
        .unwrap_or_default();
    !id.is_empty() && !id.to_lowercase().contains("core")
}

/// The system drive is a hard disk (SysMain helps those).
fn system_hdd() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::Ioctl::{PropertyStandardQuery, StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_PROPERTY_QUERY};
    use windows::Win32::System::IO::DeviceIoControl;
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    unsafe {
        let Ok(h) = CreateFileW(&HSTRING::from(format!(r"\\.\{drive}")), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, Default::default(), None) else { return false };
        let q = STORAGE_PROPERTY_QUERY { PropertyId: StorageDeviceSeekPenaltyProperty, QueryType: PropertyStandardQuery, ..Default::default() };
        let mut d = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
        let mut got = 0u32;
        let ok = DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, Some(&q as *const _ as *const _), std::mem::size_of_val(&q) as u32, Some(&mut d as *mut _ as *mut _), std::mem::size_of_val(&d) as u32, Some(&mut got), None).is_ok();
        let _ = CloseHandle(h);
        ok && d.IncursSeekPenalty
    }
}

/// Why a tweak isn't offered on this PC (None: it is).
fn hidden_because(t: &Tweak, build: u32) -> Option<&'static str> {
    if build > 0 && t.min.is_some_and(|m| build < m) {
        return Some("needs a newer Windows");
    }
    if build > 0 && t.max.is_some_and(|m| build > m) {
        return Some("not in this Windows version");
    }
    if crate::settings::is_test_profile() {
        return None;
    }
    for n in &t.needs {
        let ok = match n.split_once(':') {
            Some(("app", a)) => app_installed(a),
            Some(("service", s)) => steps::key_exists(&format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{s}")),
            Some(("edition", _)) => edition_pro(),
            _ => match n.as_str() {
                "battery" => has_battery(),
                "system_hdd" => system_hdd(),
                _ => true,
            },
        };
        if !ok {
            return Some("not on this PC");
        }
    }
    None
}

// ------------------------------------------------------------------------------------- state

/// The option the PC is at: the first whose steps all match (Windows' default also when values
/// aren't there), "mixed" when none does, None when it can't be told yet.
pub fn state_of(t: &Tweak) -> Option<String> {
    if t.kind == "action" {
        return Some(if with_journal(|j| j.entries.iter().any(|e| e.id == t.id)) { "done".into() } else { "".into() });
    }
    if t.options.is_empty() {
        return None;
    }
    let def = t.default.as_deref().unwrap_or("");
    let mut order: Vec<&Opt> = t.options.iter().filter(|o| o.id != def).collect();
    order.extend(t.options.iter().filter(|o| o.id == def));
    let mut unknown = false;
    for o in order {
        let absent_ok = o.id == def;
        let mut all = true;
        for s in &o.steps {
            match steps::matches(s, absent_ok) {
                Some(true) => {}
                Some(false) => {
                    all = false;
                    break;
                }
                None => {
                    unknown = true;
                    all = false;
                    break;
                }
            }
        }
        if all {
            return Some(o.id.clone());
        }
    }
    if unknown {
        None
    } else {
        Some("mixed".into())
    }
}

/// For a tweak that matches no option fully: the option most of its steps match, and how many.
pub fn lean_of(t: &Tweak) -> Option<(String, usize, usize)> {
    let def = t.default.as_deref().unwrap_or("");
    t.options
        .iter()
        .map(|o| {
            let n = o.steps.iter().filter(|s| steps::matches(s, o.id == def) == Some(true)).count();
            (o.id.clone(), n, o.steps.len())
        })
        .filter(|(_, n, _)| *n > 0)
        .max_by(|a, b| (a.1 as f64 / a.2.max(1) as f64).total_cmp(&(b.1 as f64 / b.2.max(1) as f64)))
}

fn who_runs() -> (bool, bool) {
    let admin = crate::elevation::is_admin() || crate::settings::is_test_profile();
    // an elevated Deck under another account (an admin password typed for a standard user) would
    // change that account's settings, not yours
    let same_user = crate::settings::is_test_profile() || crate::elevation::runs_as_desktop_user() != Some(false);
    (admin, same_user)
}

/// Why this tweak can't be changed now (None: it can).
fn blocked(t: &Tweak, admin: bool, same_user: bool) -> Option<String> {
    let steps: Vec<&Step> = t.options.iter().flat_map(|o| o.steps.iter()).collect();
    if (t.admin || steps.iter().any(|s| steps::needs_admin(s))) && !admin {
        return Some("Needs Deck's admin rights (Settings → About → Set up)".into());
    }
    if steps.iter().any(|s| steps::per_user(s)) && !same_user {
        return Some("Deck runs as another account here, so it can't change your own settings".into());
    }
    None
}

fn tweak_json(t: &Tweak, build: u32, admin: bool, same_user: bool, entries: &[Entry]) -> Value {
    let hidden = hidden_because(t, build);
    let state = if hidden.is_some() || t.kind == "link" { None } else { state_of(t) };
    let e = entries.iter().find(|e| e.id == t.id);
    let changed_back = e.is_some_and(|e| t.kind != "action" && state.as_deref().is_some_and(|s| s != e.applied));
    // partly set (another tool set some of it): which way it leans, and how far
    let lean = (state.as_deref() == Some("mixed")).then(|| lean_of(t)).flatten();
    json!({
        "id": t.id, "cat": t.cat, "group": t.group, "name": t.name, "what": t.what, "more": t.more, "risk": t.risk, "after": t.after, "src": t.src,
        "kind": t.kind, "default": t.default, "rec": t.rec, "page": t.page, "label": t.label, "tick": t.tick, "good": t.good, "lose": t.lose,
        "options": t.options.iter().map(|o| json!({ "id": o.id, "label": o.label })).collect::<Vec<_>>(),
        "state": state, "hidden": hidden, "blocked": blocked(t, admin, same_user),
        "changed": e.is_some(), "applied": e.map(|e| e.applied.clone()), "was": e.map(|e| e.was.clone()), "at": e.map(|e| e.at),
        "changed_back": changed_back,
        "lean": lean.as_ref().map(|l| l.0.clone()), "partial": lean.as_ref().map(|l| format!("{} of {}", l.1, l.2)),
        "details": details(t),
    })
}

/// What each option changes, in words for the Details fold.
fn details(t: &Tweak) -> Vec<Value> {
    t.options
        .iter()
        .map(|o| {
            let lines: Vec<String> = o
                .steps
                .iter()
                .map(|s| {
                    if let Some(p) = &s.reg {
                        let n = s.name.as_deref().map(|n| if n.is_empty() { "(Default)" } else { n }).unwrap_or("");
                        if s.key_delete {
                            format!("{p}  — key removed")
                        } else if s.key_create {
                            format!("{p}  — key added")
                        } else if s.delete {
                            format!("{p}  {n}  — removed")
                        } else if let Some(b) = s.bits {
                            format!("{p}  {n}  bit 0x{b:x} {}", if s.set.unwrap_or(true) { "set" } else { "cleared" })
                        } else {
                            format!("{p}  {n} = {}", s.value.as_ref().map(|v| v.to_string()).unwrap_or_default())
                        }
                    } else if let Some(k) = &s.dx {
                        format!(r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences  {k}={}", s.value.as_ref().and_then(|v| v.as_str()).unwrap_or("(not set)"))
                    } else if let Some(n) = &s.service {
                        format!("Service {n}: start {}", s.start.as_deref().unwrap_or(""))
                    } else if let Some(n) = &s.feature {
                        format!("Windows feature {n}: {}", if s.on.unwrap_or(true) { "on" } else { "off" })
                    } else if let Some(n) = &s.cmd {
                        match n.as_str() {
                            "hibernate" => format!("powercfg /hibernate {}", if s.on.unwrap_or(true) { "on" } else { "off" }),
                            _ => format!("DISM reserved storage {}", if s.on.unwrap_or(true) { "on" } else { "off" }),
                        }
                    } else if let Some(d) = &s.dns {
                        if d.is_empty() {
                            "DNS servers: automatic (from your router)".into()
                        } else {
                            format!("DNS servers: {}", d.join(", "))
                        }
                    } else {
                        String::new()
                    }
                })
                .collect();
            json!({ "id": o.id, "label": o.label, "lines": lines })
        })
        .collect()
}

pub fn state() -> Value {
    let build = build();
    let (admin, same_user) = who_runs();
    let entries = with_journal(|j| j.entries.clone());
    let tweaks: Vec<Value> = data().tweaks.iter().map(|t| tweak_json(t, build, admin, same_user, &entries)).collect();
    json!({
        "cats": data().cats, "presets": data().presets, "tweaks": tweaks, "build": build, "admin": admin,
        "pending": *PENDING.lock(), "checking": SLOW_BUSY.load(std::sync::atomic::Ordering::SeqCst),
        "restore_point": *RESTORE.lock(),
        "journal_set_aside": SET_ASIDE.load(std::sync::atomic::Ordering::SeqCst),
    })
}


// ------------------------------------------------------------------------------------- after-effects

#[derive(Serialize, Default, Clone)]
pub struct Pending {
    pub explorer: Vec<String>,
    pub signout: Vec<String>,
    pub restart: Vec<String>,
}

static PENDING: Mutex<Pending> = parking_lot::const_mutex(Pending { explorer: Vec::new(), signout: Vec::new(), restart: Vec::new() });

fn note_after(t: &Tweak) {
    let mut p = PENDING.lock();
    let list = match t.after.as_str() {
        "explorer" => &mut p.explorer,
        "signout" => &mut p.signout,
        "restart" => &mut p.restart,
        _ => return,
    };
    if !list.contains(&t.name) {
        list.push(t.name.clone());
    }
}

/// Restarts File Explorer, signs out or restarts the PC for the changes waiting for it.
pub fn after(app: &App, what: &str) -> Result<(), String> {
    if let Some(g) = crate::boost::game_in_front(app) {
        return Err(format!("{g} is in front: Deck waits until you leave the game"));
    }
    match what {
        "explorer" => {
            restart_explorer();
            PENDING.lock().explorer.clear();
            Ok(())
        }
        "signout" => crate::power::shutdown_exe(&["/l"]),
        "restart" => crate::power::shutdown_exe(&["/r", "/t", "0"]),
        "later" => {
            PENDING.lock().explorer.clear();
            Ok(())
        }
        _ => Err("unknown".into()),
    }
}

/// Ends Explorer and lets Windows start it again as you (an Explorer Deck started would run with
/// Deck's admin rights). Starts it itself only when Windows didn't within 5 s (AutoRestartShell
/// off, as some debloat scripts leave it): through a one-off task without admin rights.
pub(crate) fn restart_explorer() {
    let running = || {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case("explorer.exe"))
    };
    let _ = crate::tweaks::run(60, "taskkill.exe", &["/F", "/IM", "explorer.exe"]);
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if running() {
            return;
        }
    }
    crate::log::warn("tweaks: Windows didn't restart Explorer by itself; Deck started it");
    let exe = crate::util::windows_dir().join("explorer.exe").display().to_string();
    if crate::elevation::is_admin() {
        const TASK: &str = "\\Deck\\Explorer";
        let xml = crate::elevation::task_xml(&exe, "", false, "Starts File Explorer without admin rights (removed right after).").replace("<RunLevel>HighestAvailable</RunLevel>", "<RunLevel>LeastPrivilege</RunLevel>");
        let started = crate::elevation::create_task(TASK, &xml).is_ok() && crate::elevation::run_task(TASK);
        crate::elevation::delete_task(TASK);
        if started {
            return;
        }
    }
    // no desktop at all is worse than an Explorer with Deck's rights
    let _ = std::process::Command::new(exe).spawn();
}

// ------------------------------------------------------------------------------------- restore point

#[derive(Serialize, Clone, Copy)]
pub struct RestoreState {
    /// made this session (Advanced tweaks don't ask again)
    pub made: bool,
}
static RESTORE: Mutex<RestoreState> = parking_lot::const_mutex(RestoreState { made: false });

/// A System Restore point before Advanced changes. Ok(false): Windows made none (it allows one a
/// day, or System Restore is off).
pub fn restore_point() -> Result<bool, String> {
    if crate::settings::is_test_profile() {
        RESTORE.lock().made = true;
        return Ok(true);
    }
    let count = || -> i64 {
        crate::tweaks::run(60, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", "@(Get-ComputerRestorePoint).Count"])
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
            .unwrap_or(-1)
    };
    let before = count();
    let o = crate::tweaks::run(600, "powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", "Checkpoint-Computer -Description 'Deck: before tweaks' -RestorePointType MODIFY_SETTINGS"]).map_err(|e| e.to_string())?;
    let made = count() > before;
    RESTORE.lock().made = true;
    if !o.status.success() && !made {
        let e = String::from_utf8_lossy(&o.stderr);
        return Err(if e.to_lowercase().contains("disabled") || e.contains("0x80042306") {
            "System Restore is off on this PC (turn it on in System → System protection)".into()
        } else {
            format!("Windows didn't make a restore point: {}", e.lines().next().unwrap_or("").trim())
        });
    }
    Ok(made)
}

// ------------------------------------------------------------------------------------- change

/// Sets tweak `id` to option `opt`. Before each step, what is there is kept (once) for undo; when
/// a step fails, what this call already changed is put back.
pub fn set(app: &App, id: &str, opt: &str) -> Result<(), String> {
    let t = find(id).ok_or("unknown tweak")?;
    if hidden_because(t, build()).is_some() {
        return Err("This tweak isn't for this PC".into());
    }
    let (admin, same_user) = who_runs();
    if let Some(why) = blocked(t, admin, same_user) {
        return Err(why);
    }
    if let Some(g) = crate::boost::game_in_front(app) {
        return Err(format!("{g} is in front: Deck changes Windows settings only outside games"));
    }
    set_inner(id, opt)
}

fn set_inner(id: &str, opt: &str) -> Result<(), String> {
    let t = find(id).ok_or("unknown tweak")?;
    let o = t.options.iter().find(|o| o.id == opt).ok_or("unknown option")?;
    // back to what it was before Deck: an exact undo rather than Windows' textbook value
    if with_journal(|j| j.entries.iter().any(|e| e.id == id && e.was == opt)) {
        return undo(id);
    }
    let was = state_of(t).unwrap_or_default();
    let mut befores: Vec<Before> = Vec::new();
    for s in &o.steps {
        let b = steps::before(s);
        if let Err(e) = steps::apply(s) {
            // undone step by step; one that can't be undone is said, since the PC is then half-changed
            let undo_failed: Vec<String> = befores.iter().rev().filter_map(|x| steps::restore(x).err()).collect();
            if undo_failed.is_empty() {
                crate::log::warn(format!("tweaks: {id} → {opt} failed and was rolled back: {e}"));
            } else {
                crate::log::error(format!("tweaks: {id} → {opt} failed ({e}) and {} step(s) couldn't be rolled back: {}", undo_failed.len(), undo_failed.join(" · ")));
            }
            return Err(e);
        }
        befores.extend(b);
    }
    with_journal(|j| match j.entries.iter_mut().find(|e| e.id == id) {
        Some(e) => {
            // keep the first value Deck saw for each target; add targets this option touches newly
            for b in befores {
                if !e.before.iter().any(|x| x.same_target(&b)) {
                    e.before.push(b);
                }
            }
            e.applied = opt.to_string();
            e.at = crate::util::now_s();
        }
        None => j.entries.push(Entry { id: id.into(), applied: opt.into(), was, at: crate::util::now_s(), before: befores, backup: None }),
    });
    save_journal();
    note_after(t);
    crate::log::info(format!("tweaks: {id} set to {opt}"));
    Ok(())
}

/// The tweaks Deck applied here, as (id, option), for a PC to PC setup.
pub fn applied() -> Vec<(String, String)> {
    with_journal(|j| j.entries.iter().filter(|e| find(&e.id).is_some_and(|t| t.kind == "switch" || t.kind == "choice")).map(|e| (e.id.clone(), e.applied.clone())).collect())
}

pub fn is_changed(id: &str) -> bool {
    with_journal(|j| j.entries.iter().any(|e| e.id == id))
}

/// A tweak from a PC to PC setup the user chose to apply (skipped when it isn't for this PC).
pub fn set_from_setup(id: &str, opt: &str) -> Result<(), String> {
    let t = find(id).ok_or("not in this Deck")?;
    if hidden_because(t, build()).is_some() {
        return Err("not for this PC".into());
    }
    if state_of(t).as_deref() == Some(opt) {
        return Ok(());
    }
    let (admin, same_user) = who_runs();
    if let Some(why) = blocked(t, admin, same_user) {
        return Err(why);
    }
    set_inner(id, opt)
}

/// Puts everything tweak `id` changed back as it was before Deck.
pub fn undo(id: &str) -> Result<(), String> {
    let e = with_journal(|j| j.entries.iter().find(|e| e.id == id).cloned()).ok_or("Deck didn't change this")?;
    // only what this Deck's recipe for it touches (an entry for a tweak it doesn't know isn't acted on)
    let t = find(id).ok_or("This tweak isn't in this version of Deck: Forget it instead")?;
    if let Some(b) = e.before.iter().find(|b| !before_allowed(t, b)) {
        crate::log::warn(format!("tweaks: undo of {id} refused: its undo list names something its recipe doesn't touch ({b:?})"));
        return Err("Its undo list names something this tweak never changes, so Deck won't use it".into());
    }
    let (admin, same_user) = who_runs();
    if let Some(why) = blocked(t, admin, same_user) {
        return Err(why);
    }
    if t.kind == "action" {
        actions::undo(t, &e)?;
    }
    let mut first_err = None;
    for b in e.before.iter().rev() {
        if let Err(x) = steps::restore(b) {
            first_err.get_or_insert(x);
        }
    }
    if let Some(x) = first_err {
        return Err(x);
    }
    with_journal(|j| j.entries.retain(|x| x.id != id));
    save_journal();
    if let Some(t) = find(id) {
        note_after(t);
    }
    crate::log::info(format!("tweaks: {id} undone"));
    Ok(())
}

/// Undoes every tweak, newest first. Returns how many, and the ones that failed.
pub fn undo_all() -> (usize, Vec<String>) {
    let mut ids: Vec<(u64, String)> = with_journal(|j| j.entries.iter().map(|e| (e.at, e.id.clone())).collect());
    ids.sort_by_key(|x| std::cmp::Reverse(x.0));
    let (mut n, mut failed) = (0, Vec::new());
    for (_, id) in ids {
        match undo(&id) {
            Ok(()) => n += 1,
            Err(e) => failed.push(format!("{}: {e}", find(&id).map(|t| t.name.as_str()).unwrap_or(&id))),
        }
    }
    (n, failed)
}

/// "Windows changed this back": stop listing it (the journal entry goes, the PC stays as it is).
pub fn forget(id: &str) {
    with_journal(|j| j.entries.retain(|x| x.id != id));
    save_journal();
}

/// Runs a one-off action (`do` in the data).
pub fn run_action(app: &App, id: &str) -> Result<(), String> {
    let t = find(id).filter(|t| t.kind == "action").ok_or("unknown action")?;
    if let Some(g) = crate::boost::game_in_front(app) {
        return Err(format!("{g} is in front: Deck changes Windows settings only outside games"));
    }
    if with_journal(|j| j.entries.iter().any(|e| e.id == id)) {
        return Err("Already done: undo it first".into());
    }
    let backup = actions::run(t)?;
    with_journal(|j| j.entries.push(Entry { id: id.into(), applied: "done".into(), was: "".into(), at: crate::util::now_s(), before: Vec::new(), backup }));
    save_journal();
    note_after(t);
    Ok(())
}

mod actions {
    use super::{Entry, Tweak};

    fn start_dir() -> std::path::PathBuf {
        crate::util::local_appdata().join(r"Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState")
    }

    /// Win11Debloat's empty Start layout (MIT): Start opens with nothing pinned.
    const EMPTY_START: &[u8] = include_bytes!("../../tweaks/start2.bin");

    /// Where "Unpin everything" keeps your Start layout for its undo.
    fn keep_path() -> std::path::PathBuf {
        crate::settings::Settings::dir().join("tweaks-start2.bin")
    }

    fn restart_start() {
        let _ = crate::tweaks::run(60, "taskkill.exe", &["/F", "/IM", "StartMenuExperienceHost.exe"]);
    }

    pub fn run(t: &Tweak) -> Result<Option<String>, String> {
        match t.action.as_deref() {
            Some("start_clear") => {
                let file = start_dir().join("start2.bin");
                let keep = keep_path();
                if file.exists() {
                    // read with your rights: the file is in your folder (it could be swapped for a link)
                    let mut b = Vec::new();
                    std::io::Read::read_to_end(&mut crate::fsguard::open_read_for_user(&file).map_err(|e| format!("Couldn't keep your Start layout: {e}"))?, &mut b).map_err(|e| format!("Couldn't keep your Start layout: {e}"))?;
                    crate::util::write_atomic(&keep, &b).map_err(|e| format!("Couldn't keep your Start layout: {e}"))?;
                } else {
                    // a copy from an earlier time isn't this layout: undo leaves Windows' default
                    let _ = std::fs::remove_file(&keep);
                }
                std::fs::create_dir_all(start_dir()).map_err(|e| e.to_string())?;
                crate::util::write_atomic(&file, EMPTY_START).map_err(|e| e.to_string())?;
                restart_start();
                Ok(Some(keep.display().to_string()))
            }
            _ => Err("unknown action".into()),
        }
    }

    pub fn undo(t: &Tweak, e: &Entry) -> Result<(), String> {
        match t.action.as_deref() {
            Some("start_clear") => {
                let file = start_dir().join("start2.bin");
                // Deck's own copy only (never a path from the journal), read with your rights
                let keep = keep_path();
                let _ = e;
                match keep.exists().then_some(&keep) {
                    Some(keep) => {
                        let mut b = Vec::new();
                        std::io::Read::read_to_end(&mut crate::fsguard::open_read_for_user(keep).map_err(|e| e.to_string())?, &mut b).map_err(|e| e.to_string())?;
                        crate::util::write_atomic(&file, &b).map_err(|e| e.to_string())?;
                        let _ = std::fs::remove_file(keep);
                    }
                    // there was no layout file: Windows makes its default one again
                    None => {
                        let _ = std::fs::remove_file(&file);
                    }
                }
                restart_start();
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

// ------------------------------------------------------------------------------------- watch

/// Every 6 hours: tweaks Windows changed back since the last look (a toast once for each).
pub fn start_watch(app: Arc<App>) {
    std::thread::Builder::new()
        .name("tweaks-watch".into())
        .spawn(move || {
            let mut told: Vec<String> = Vec::new();
            std::thread::sleep(Duration::from_secs(120));
            loop {
                let entries = with_journal(|j| j.entries.clone());
                let back: Vec<&'static Tweak> = entries
                    .iter()
                    .filter_map(|e| {
                        let t = find(&e.id)?;
                        (t.kind != "action" && state_of(t).is_some_and(|s| s != e.applied)).then_some(t)
                    })
                    .filter(|t| !told.contains(&t.id))
                    .collect();
                if !back.is_empty() {
                    let names: Vec<&str> = back.iter().map(|t| t.name.as_str()).collect();
                    app.toast(crate::app::Toast::simple(
                        "tweaks-back",
                        &format!("Windows changed {} tweak{} back", back.len(), if back.len() > 1 { "s" } else { "" }),
                        &format!("{} (an update, usually). Deck → Tweaks can apply them again.", names.join(", ")),
                    ));
                    told.extend(back.iter().map(|t| t.id.clone()));
                    app.changed("tweaks");
                }
                std::thread::sleep(Duration::from_secs(6 * 3600));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch place in HKCU for this test only (every path is mapped under it).
    fn scratch(name: &str) -> String {
        let base = format!(r"Software\Deck-test\tweaks-unit-{name}-{}", std::process::id());
        steps::use_scratch(&base);
        let _ = steps::delete_key(r"HKCU");
        base
    }

    fn cleanup(base: &str) {
        let _ = steps::delete_key("HKCU");
        unsafe {
            use windows_sys::Win32::System::Registry::*;
            RegDeleteTreeW(HKEY_CURRENT_USER, crate::util::wide(base).as_ptr());
        }
    }

    fn step(v: Value) -> Step {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn every_recipe_reads_and_names_its_options() {
        let d = data();
        assert!(d.tweaks.len() > 100);
        for t in &d.tweaks {
            if t.kind == "switch" || t.kind == "choice" {
                assert!(t.options.len() >= 2, "{}", t.id);
                assert!(t.options.iter().any(|o| Some(&o.id) == t.default.as_ref()), "{} default", t.id);
                assert!(t.options.iter().all(|o| !o.steps.is_empty()), "{} empty option", t.id);
            }
        }
    }

    #[test]
    fn a_value_is_put_back_exactly_also_when_it_was_missing() {
        let base = scratch("value");
        let path = r"HKCU\Software\Test\Explorer";
        let on = step(json!({ "reg": path, "name": "HideFileExt", "type": "dword", "value": 0 }));
        // missing before: undo removes it again, and the key Deck created
        let b = steps::before(&on);
        steps::apply(&on).unwrap();
        assert_eq!(steps::matches(&on, false), Some(true));
        for x in b.iter().rev() {
            steps::restore(x).unwrap();
        }
        assert!(steps::read_value(path, "HideFileExt").is_none());
        assert!(!steps::key_exists(r"HKCU\Software\Test"), "the key Deck made is gone");
        // there before as a string "7": comes back as that string, not a dword
        let odd = step(json!({ "reg": path, "name": "HideFileExt", "type": "string", "value": "7" }));
        steps::apply(&odd).unwrap();
        let b = steps::before(&on);
        steps::apply(&on).unwrap();
        for x in b.iter().rev() {
            steps::restore(x).unwrap();
        }
        assert_eq!(steps::matches(&odd, false), Some(true));
        cleanup(&base);
    }

    #[test]
    fn a_deleted_key_comes_back_with_everything_under_it() {
        let base = scratch("tree");
        let k = r"HKLM\SOFTWARE\Classes\Folder\ShellEx\Library Location";
        steps::apply(&step(json!({ "reg": k, "name": "", "type": "string", "value": "{3dad6c5d}" }))).unwrap();
        steps::apply(&step(json!({ "reg": format!(r"{k}\Sub"), "name": "N", "type": "dword", "value": 5 }))).unwrap();
        let del = step(json!({ "reg": k, "key_delete": true }));
        let b = steps::before(&del);
        steps::apply(&del).unwrap();
        assert_eq!(steps::matches(&del, false), Some(true));
        for x in b.iter().rev() {
            steps::restore(x).unwrap();
        }
        assert!(steps::matches(&step(json!({ "reg": format!(r"{k}\Sub"), "name": "N", "type": "dword", "value": 5 })), false) == Some(true));
        cleanup(&base);
    }

    #[test]
    fn bits_change_only_their_own_bits() {
        let base = scratch("bits");
        let p = r"HKLM\SYSTEM\Tcpip6\Parameters";
        steps::apply(&step(json!({ "reg": p, "name": "DisabledComponents", "type": "dword", "value": 1 }))).unwrap();
        let set = step(json!({ "reg": p, "name": "DisabledComponents", "bits": 32, "set": true }));
        steps::apply(&set).unwrap();
        assert_eq!(steps::matches(&set, false), Some(true));
        assert!(steps::matches(&step(json!({ "reg": p, "name": "DisabledComponents", "type": "dword", "value": 33 })), false) == Some(true));
        steps::apply(&step(json!({ "reg": p, "name": "DisabledComponents", "bits": 32, "set": false }))).unwrap();
        assert!(steps::matches(&step(json!({ "reg": p, "name": "DisabledComponents", "type": "dword", "value": 1 })), false) == Some(true));
        cleanup(&base);
    }

    #[test]
    fn undo_only_touches_what_the_tweaks_recipe_touches() {
        let t = find("gaming.gamebar").unwrap();
        let p = |s: &str| s.to_string();
        // its own key (any case) and the key it made: fine
        assert!(before_allowed(t, &Before::Key { path: p(r"hkcr\ms-gamebar\shell\open\command"), tree: None }));
        // a forged journal: another key, a value it never sets, a service it never touches
        assert!(!before_allowed(t, &Before::Key { path: p(r"HKCR\exefile\shell\open\command"), tree: None }));
        assert!(!before_allowed(t, &Before::Value { path: p(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"), name: p("x"), data: None }));
        assert!(!before_allowed(t, &Before::Service { name: p("WinDefend"), start: 2, delayed: None }));
        let ext = find("explorer.ext").unwrap();
        let step = ext.options.iter().flat_map(|o| o.steps.iter()).find(|s| s.reg.is_some()).unwrap();
        let (path, name) = (step.reg.clone().unwrap(), step.name.clone().unwrap_or_default());
        assert!(before_allowed(ext, &Before::Value { path: path.clone(), name: name.clone(), data: None }));
        // the first missing key on its path, not a sibling that only starts the same
        let parent = path.rsplit_once('\\').unwrap().0.to_string();
        assert!(before_allowed(ext, &Before::Created { path: parent.clone() }));
        assert!(!before_allowed(ext, &Before::Created { path: format!("{parent}X") }));
        assert!(!before_allowed(ext, &Before::Value { path, name: format!("{name}2"), data: None }));
    }

    #[test]
    fn directx_settings_keep_the_other_keys() {
        let base = scratch("dx");
        let path = r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences";
        steps::apply(&step(json!({ "reg": path, "name": "DirectXUserGlobalSettings", "type": "string", "value": "VRROptimizeEnable=1;" }))).unwrap();
        let on = step(json!({ "dx": "SwapEffectUpgradeEnable", "value": "1" }));
        steps::apply(&on).unwrap();
        assert_eq!(steps::matches(&on, false), Some(true));
        assert_eq!(steps::matches(&step(json!({ "dx": "VRROptimizeEnable", "value": "1" })), false), Some(true));
        let unset = step(json!({ "dx": "SwapEffectUpgradeEnable", "value": null }));
        steps::apply(&unset).unwrap();
        assert_eq!(steps::matches(&unset, false), Some(true));
        assert_eq!(steps::matches(&step(json!({ "dx": "VRROptimizeEnable", "value": "1" })), false), Some(true));
        // two tweaks in the one string: undoing the first keeps what the second set since
        let vrr_off = step(json!({ "dx": "VRROptimizeEnable", "value": "0" }));
        let b1 = steps::before(&vrr_off);
        steps::apply(&vrr_off).unwrap();
        steps::apply(&on).unwrap();
        for b in &b1 {
            steps::restore(b).unwrap();
        }
        assert_eq!(steps::matches(&step(json!({ "dx": "VRROptimizeEnable", "value": "1" })), false), Some(true));
        assert_eq!(steps::matches(&on, false), Some(true), "the other tweak's key stays");
        cleanup(&base);
    }

    #[test]
    fn a_tweaks_state_follows_its_options_and_windows_default() {
        let base = scratch("state");
        let t = find("explorer.ext").unwrap().clone();
        // nothing set: Windows' default (extensions hidden: "off")
        assert_eq!(state_of(&t).as_deref(), Some("off"));
        for s in &t.options.iter().find(|o| o.id == "on").unwrap().steps {
            steps::apply(s).unwrap();
        }
        assert_eq!(state_of(&t).as_deref(), Some("on"));
        cleanup(&base);
    }
}

#[cfg(test)]
mod live {
    /// Live, read-only: the page's data on this PC, for the UI preview's fake backend
    /// (DECK_TWEAKS_DUMP=<file>).
    #[test]
    #[ignore]
    fn dump_state_for_the_preview() {
        let Ok(p) = std::env::var("DECK_TWEAKS_DUMP") else { return };
        std::fs::write(p, serde_json::to_vec(&super::state()).unwrap()).unwrap();
    }

    /// Live, read-only: every tweak's state on this PC (nothing is changed).
    #[test]
    #[ignore]
    fn read_every_state_on_this_pc() {
        let b = super::build();
        for t in &super::data().tweaks {
            let hidden = super::hidden_because(t, b);
            let st = if hidden.is_some() || t.kind == "link" { None } else { super::state_of(t) };
            println!("{:32} {:10} {}", t.id, st.as_deref().unwrap_or("-"), hidden.unwrap_or(""));
        }
    }
}

#[cfg(test)]
mod live_undo {
    /// Live on this PC: three harmless per-user tweaks applied and undone the way Undo does it;
    /// a `reg export` of their keys before and after must be identical.
    #[test]
    #[ignore]
    fn apply_and_undo_leave_the_registry_exactly_as_it_was() {
        let dir = std::env::temp_dir().join(format!("deck-undo-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let export = |key: &str, name: &str| {
            let f = dir.join(name);
            let _ = std::fs::remove_file(&f);
            let o = std::process::Command::new("reg.exe").args(["export", key, &f.display().to_string(), "/y"]).output().unwrap();
            if o.status.success() { std::fs::read(&f).unwrap() } else { Vec::new() }
        };
        for (id, key) in [("explorer.checkboxes", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
                          ("explorer.fullpath", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState"),
                          ("taskbar.seconds", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced")] {
            let t = super::find(id).unwrap();
            let now = super::state_of(t).unwrap();
            let other = t.options.iter().find(|o| o.id != now).unwrap();
            let before = export(key, "before.reg");
            let mut saved = Vec::new();
            for s in &other.steps {
                saved.extend(super::steps::before(s));
                super::steps::apply(s).unwrap();
            }
            assert_eq!(super::state_of(t).as_deref(), Some(other.id.as_str()), "{id} reads as set");
            for b in saved.iter().rev() {
                super::steps::restore(b).unwrap();
            }
            let after = export(key, "after.reg");
            assert_eq!(super::state_of(t).as_deref(), Some(now.as_str()));
            assert!(before == after, "{id}: the key differs after undo");
            println!("{id}: {now} → {} → {now}, {key} identical ({} bytes)", other.id, before.len());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
