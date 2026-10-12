//! Safe one-click cleanups: caches and leftovers that Windows and apps
//! rebuild on their own. Nothing here needs administrator rights.

use crate::util::{appdata, dir_size, downloads, home, local_appdata};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Clone, Serialize, Debug)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub desc: String,
    /// "safe" (rebuilt automatically) | "careful" (costs time later, e.g. rebuilds)
    pub level: String,
    /// None until measured (the page shows "…" rather than a misleading 0 B)
    pub size: Option<u64>,
    pub files: u64,
    /// Process that should be closed for a full clean.
    pub close_first: Option<String>,
    pub running: bool,
    #[serde(skip)]
    paths: Vec<PathBuf>,
    /// Only delete files older than this many days (0 = everything).
    #[serde(skip)]
    older_days: u64,
    /// Files with these extensions only (empty = all).
    #[serde(skip)]
    exts: Vec<&'static str>,
    /// Move to the Recycle Bin instead of deleting.
    #[serde(skip)]
    recycle: bool,
}

pub struct Cleanup {
    pub targets: Mutex<Vec<Target>>,
    pub busy: AtomicBool,
    pub last: Mutex<Option<(u64, u64)>>,
}

fn glob_children(dir: &Path, sub: &[&str]) -> Vec<PathBuf> {
    // every profile folder inside `dir` + sub path (e.g. Chrome profiles)
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let mut p = e.path();
                for s in sub {
                    p.push(s);
                }
                if p.exists() {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn browser_caches(user_data: PathBuf) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for sub in [&["Cache"][..], &["Code Cache"], &["GPUCache"], &["Service Worker", "CacheStorage"]] {
        v.extend(glob_children(&user_data, sub));
    }
    v
}

fn target(id: &str, name: &str, desc: &str, level: &str, paths: Vec<PathBuf>) -> Target {
    Target {
        id: id.into(),
        name: name.into(),
        desc: desc.into(),
        level: level.into(),
        size: None,
        files: 0,
        close_first: None,
        running: false,
        paths: paths.into_iter().filter(|p| p.exists()).collect(),
        older_days: 0,
        exts: Vec::new(),
        recycle: false,
    }
}

fn all_targets() -> Vec<Target> {
    let la = local_appdata();
    let ra = appdata();
    let mut v = Vec::new();
    let mut t = target("temp", "Temporary files", "Leftovers apps put in your Temp folder. Files from the last day are kept.", "safe", vec![std::env::temp_dir()]);
    t.older_days = 1;
    v.push(t);
    v.push(target(
        "shader",
        "Graphics shader caches",
        "NVIDIA/AMD/DirectX shader caches. Games rebuild them; the first launch after may stutter briefly.",
        "safe",
        vec![la.join("NVIDIA\\DXCache"), la.join("NVIDIA\\GLCache"), la.join("D3DSCache"), la.join("AMD\\DxCache"), la.join("AMD\\GLCache"), la.join("NVIDIA Corporation\\NV_Cache")],
    ));
    v.push(target(
        "crash",
        "Crash dumps and error reports",
        "Memory dumps and Windows Error Reporting files from crashed apps.",
        "safe",
        vec![la.join("CrashDumps"), la.join("Microsoft\\Windows\\WER\\ReportArchive"), la.join("Microsoft\\Windows\\WER\\ReportQueue")],
    ));
    let mut t = target("chrome", "Chrome cache", "Cached web pages and images. Sites load a bit slower the first time.", "safe", browser_caches(la.join("Google\\Chrome\\User Data")));
    t.close_first = Some("chrome.exe".into());
    v.push(t);
    let mut t = target("edge", "Edge cache", "Cached web pages and images (also used by apps built on Edge).", "safe", browser_caches(la.join("Microsoft\\Edge\\User Data")));
    t.close_first = Some("msedge.exe".into());
    v.push(t);
    let mut t = target("brave", "Brave cache", "Cached web pages and images.", "safe", browser_caches(la.join("BraveSoftware\\Brave-Browser\\User Data")));
    t.close_first = Some("brave.exe".into());
    v.push(t);
    let mut t = target("opera", "Opera GX cache", "Cached web pages and images.", "safe", vec![la.join("Opera Software\\Opera GX Stable\\Cache"), ra.join("Opera Software\\Opera GX Stable\\Code Cache")]);
    t.close_first = Some("opera.exe".into());
    v.push(t);
    let mut t = target("firefox", "Firefox cache", "Cached web pages and images.", "safe", glob_children(&la.join("Mozilla\\Firefox\\Profiles"), &["cache2"]));
    t.close_first = Some("firefox.exe".into());
    v.push(t);
    let mut t = target("discord", "Discord cache", "Cached images, GIFs and videos from chats.", "safe", vec![ra.join("discord\\Cache"), ra.join("discord\\Code Cache"), ra.join("discord\\GPUCache")]);
    t.close_first = Some("Discord.exe".into());
    v.push(t);
    let mut t = target("spotify", "Spotify cache", "Songs Spotify cached for streaming (not your downloads for offline).", "safe", vec![la.join("Spotify\\Storage"), la.join("Spotify\\Data")]);
    t.close_first = Some("Spotify.exe".into());
    v.push(t);
    let mut t = target("steamshader", "Steam shader caches", "Per-game shader caches. Steam rebuilds them; first launch may stutter.", "safe", crate::games::steam_libraries().into_iter().map(|l| l.join("steamapps\\shadercache")).collect());
    t.close_first = Some("steam.exe".into());
    v.push(t);
    let mut t = target("installers", "Old installers in Downloads", "Setup files (.exe, .msi) you downloaded over 2 weeks ago. Moved to the Recycle Bin.", "careful", vec![downloads()]);
    t.older_days = 14;
    t.exts = vec!["exe", "msi", "msix", "msixbundle", "appx"];
    t.recycle = true;
    v.push(t);
    v.push(target("npm", "npm cache", "Downloaded JavaScript packages; npm downloads them again when needed.", "careful", vec![la.join("npm-cache")]));
    v.push(target("pip", "pip cache", "Downloaded Python packages.", "careful", vec![la.join("pip\\Cache")]));
    v.push(target("gradle", "Gradle caches", "Android/Java build downloads. The next build is slower.", "careful", vec![home().join(".gradle\\caches")]));
    v.push(target("nuget", "NuGet packages", ".NET packages; restored on the next build.", "careful", vec![home().join(".nuget\\packages")]));
    let rust = std::env::var("CARGO_TARGET_DIR").map(PathBuf::from).ok().filter(|p| p.is_dir());
    // only where a shared build folder is set up (CARGO_TARGET_DIR)
    if let Some(rust) = rust {
        v.push(target("rust", "Rust build cache", "Compiled Rust builds. Everything rebuilds from scratch next time (minutes).", "careful", vec![rust]));
    }
    let mut t = target("thumbs", "Explorer thumbnails", "Picture/video previews Explorer caches. Rebuilt as you browse.", "safe", vec![la.join("Microsoft\\Windows\\Explorer")]);
    t.exts = vec!["db"];
    t.close_first = None;
    v.push(t);
    v.into_iter().filter(|t| !t.paths.is_empty()).collect()
}

fn old_enough(p: &Path, days: u64) -> bool {
    if days == 0 {
        return true;
    }
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() > Duration::from_secs(days * 86400))
        .unwrap_or(false)
}

fn matches(t: &Target, p: &Path) -> bool {
    if !t.exts.is_empty() {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !t.exts.contains(&ext.as_str()) {
            return false;
        }
        if t.id == "thumbs" && !p.file_name().map(|n| n.to_string_lossy().starts_with("thumbcache_")).unwrap_or(false) {
            return false;
        }
    }
    old_enough(p, t.older_days)
}

fn measure(t: &mut Target) {
    let (mut size, mut files) = (0, 0);
    for root in &t.paths {
        if t.exts.is_empty() && t.older_days == 0 {
            let (s, f) = dir_size(root);
            size += s;
            files += f;
        } else {
            let depth = if t.id == "installers" || t.id == "thumbs" { 1 } else { usize::MAX };
            for e in jwalk::WalkDir::new(root).max_depth(depth).skip_hidden(false).into_iter().flatten() {
                if e.file_type().is_file() && matches(t, &e.path()) {
                    size += e.metadata().map(|m| m.len()).unwrap_or(0);
                    files += 1;
                }
            }
        }
    }
    t.size = Some(size);
    t.files = files;
}

/// Deletes what a target covers, skipping files that are in use.
/// Returns (bytes freed, files removed).
fn clean(t: &Target) -> (u64, u64) {
    let (mut freed, mut n) = (0u64, 0u64);
    if t.recycle {
        let mut batch = Vec::new();
        for root in &t.paths {
            if let Ok(rd) = std::fs::read_dir(root) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_file() && matches(t, &p) {
                        freed += e.metadata().map(|m| m.len()).unwrap_or(0);
                        n += 1;
                        batch.push(p.to_string_lossy().to_string());
                    }
                }
            }
        }
        if crate::disk::recycle(&batch).is_err() {
            return (0, 0);
        }
        return (freed, n);
    }
    for root in &t.paths {
        let depth = if t.id == "thumbs" { 1 } else { usize::MAX };
        let entries: Vec<_> = jwalk::WalkDir::new(root).max_depth(depth).skip_hidden(false).into_iter().flatten().collect();
        // files first, then empty folders deepest-first
        for e in &entries {
            if e.file_type().is_file() && matches(t, &e.path()) {
                let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                if std::fs::remove_file(e.path()).is_ok() {
                    freed += len;
                    n += 1;
                }
            }
        }
        if t.exts.is_empty() {
            let mut dirs: Vec<_> = entries.iter().filter(|e| e.file_type().is_dir() && e.depth() > 0).map(|e| (e.depth(), e.path())).collect();
            dirs.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, d) in dirs {
                let _ = std::fs::remove_dir(d);
            }
        }
    }
    (freed, n)
}

/// Recycle Bin size (bytes, items) for all drives.
pub fn recycle_bin() -> (u64, u64) {
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe {
        if SHQueryRecycleBinW(None, &mut info).is_ok() {
            return (info.i64Size as u64, info.i64NumItems as u64);
        }
    }
    (0, 0)
}

pub fn empty_recycle_bin() -> Result<(), String> {
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};
    unsafe { SHEmptyRecycleBinW(None, None, SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) }.map_err(|e| e.to_string())
}

impl Cleanup {
    pub fn new() -> Self {
        Self { targets: Mutex::new(Vec::new()), busy: AtomicBool::new(false), last: Mutex::new(None) }
    }

    /// Measures every target in the background.
    pub fn scan(self: &Arc<Self>, app: Arc<crate::app::App>) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let mut ts = all_targets();
            *me.targets.lock() = ts.clone();
            app.changed("cleanup");
            for t in ts.iter_mut() {
                measure(t);
                if let Some(p) = &t.close_first {
                    t.running = app.sys.running(p);
                }
                if let Some(x) = me.targets.lock().iter_mut().find(|x| x.id == t.id) {
                    *x = t.clone();
                }
                app.changed("cleanup");
            }
            me.busy.store(false, Ordering::SeqCst);
            app.changed("cleanup");
        });
    }

    pub fn run(self: &Arc<Self>, ids: Vec<String>, app: Arc<crate::app::App>) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let (mut freed, mut n) = (0, 0);
            let ts = me.targets.lock().clone();
            for t in ts.iter().filter(|t| ids.contains(&t.id)) {
                let (f, c) = clean(t);
                freed += f;
                n += c;
            }
            if ids.iter().any(|i| i == "recycle") {
                let (s, c) = recycle_bin();
                if empty_recycle_bin().is_ok() {
                    freed += s;
                    n += c;
                }
            }
            *me.last.lock() = Some((freed, n));
            me.busy.store(false, Ordering::SeqCst);
            app.toast(crate::app::Toast::simple("cleanup", &format!("Freed {}", crate::util::fmt_bytes(freed)), &format!("{n} files removed. Files in use were skipped.")));
            me.scan(app);
        });
    }
}
