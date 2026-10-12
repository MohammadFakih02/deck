//! "What's eating my disk": a fast parallel scan of a drive or folder into a
//! tree of folder sizes (like WizTree/TreeSize, without admin), plus the
//! largest files.

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BinaryHeap, HashMap};
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
struct Node {
    name: String,
    parent: u32,
    size: u64,
    files: u64,
    children: Vec<u32>,
}

#[derive(Default)]
struct Scan {
    root: PathBuf,
    nodes: Vec<Node>,
    big_files: Vec<(u64, String)>,
    errors: u64,
    took: f32,
}

#[derive(Serialize, Clone)]
pub struct Entry {
    pub id: Option<u32>,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub files: u64,
    pub dir: bool,
    /// a system folder: the page offers no Recycle Bin button
    pub keep: bool,
}

pub struct Disk {
    scan: Mutex<Option<Scan>>,
    pub running: AtomicBool,
    cancel: AtomicBool,
    pub progress_files: AtomicU64,
    pub progress_bytes: AtomicU64,
    started: Mutex<Option<(Instant, String)>>,
}

impl Disk {
    pub fn new() -> Self {
        Self {
            scan: Mutex::new(None),
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            progress_files: AtomicU64::new(0),
            progress_bytes: AtomicU64::new(0),
            started: Mutex::new(None),
        }
    }

    pub fn status(&self) -> Value {
        let s = self.scan.lock();
        json!({
            "running": self.running.load(Ordering::Relaxed),
            "files": self.progress_files.load(Ordering::Relaxed),
            "bytes": self.progress_bytes.load(Ordering::Relaxed),
            "target": self.started.lock().as_ref().map(|x| x.1.clone()),
            "elapsed": self.started.lock().as_ref().map(|x| x.0.elapsed().as_secs_f32()),
            "root": s.as_ref().map(|s| s.root.to_string_lossy().to_string()),
            "total": s.as_ref().and_then(|s| s.nodes.first().map(|n| n.size)),
            "total_files": s.as_ref().and_then(|s| s.nodes.first().map(|n| n.files)),
            "errors": s.as_ref().map(|s| s.errors),
            "took": s.as_ref().map(|s| s.took),
        })
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Scans `root` in the background; `done` runs when finished.
    pub fn start(self: &Arc<Self>, root: String, done: impl FnOnce() + Send + 'static) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.progress_files.store(0, Ordering::Relaxed);
        self.progress_bytes.store(0, Ordering::Relaxed);
        *self.started.lock() = Some((Instant::now(), root.clone()));
        let me = self.clone();
        std::thread::Builder::new()
            .name("disk-scan".into())
            .spawn(move || {
                let t = Instant::now();
                let result = me.walk(Path::new(&root));
                if let Some(mut s) = result {
                    s.took = t.elapsed().as_secs_f32();
                    *me.scan.lock() = Some(s);
                }
                me.running.store(false, Ordering::SeqCst);
                done();
            })
            .ok();
    }

    fn walk(&self, root: &Path) -> Option<Scan> {
        let mut nodes = vec![Node { name: root.to_string_lossy().to_string(), parent: u32::MAX, size: 0, files: 0, children: Vec::new() }];
        let mut index: HashMap<PathBuf, u32> = HashMap::new();
        index.insert(root.to_path_buf(), 0);
        let mut heap: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
        let mut errors = 0u64;
        let walk = jwalk::WalkDir::new(root)
            .skip_hidden(false)
            .follow_links(false)
            .parallelism(jwalk::Parallelism::RayonNewPool(4));
        for e in walk {
            if self.cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Ok(e) = e else {
                errors += 1;
                continue;
            };
            if e.depth() == 0 {
                continue;
            }
            let parent = e.parent_path();
            let Some(&pid) = index.get::<std::path::Path>(parent.as_ref()) else { continue };
            if e.file_type().is_dir() {
                let id = nodes.len() as u32;
                nodes.push(Node { name: e.file_name().to_string_lossy().to_string(), parent: pid, size: 0, files: 0, children: Vec::new() });
                nodes[pid as usize].children.push(id);
                index.insert(e.path(), id);
            } else if e.file_type().is_file() {
                let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                nodes[pid as usize].size += len;
                nodes[pid as usize].files += 1;
                self.progress_files.fetch_add(1, Ordering::Relaxed);
                self.progress_bytes.fetch_add(len, Ordering::Relaxed);
                if len > 50_000_000 {
                    heap.push(Reverse((len, e.path().to_string_lossy().to_string())));
                    if heap.len() > 100 {
                        heap.pop();
                    }
                }
            }
        }
        // Children were created after their parents, so a reverse pass adds
        // every folder's total into its parent.
        for i in (1..nodes.len()).rev() {
            let (p, s, f) = (nodes[i].parent as usize, nodes[i].size, nodes[i].files);
            nodes[p].size += s;
            nodes[p].files += f;
        }
        let mut big: Vec<(u64, String)> = heap.into_iter().map(|Reverse(x)| x).collect();
        big.sort_by(|a, b| b.0.cmp(&a.0));
        Some(Scan { root: root.to_path_buf(), nodes, big_files: big, errors, took: 0.0 })
    }

    fn path_of(s: &Scan, mut id: u32) -> PathBuf {
        let mut parts = Vec::new();
        while id != 0 {
            let n = &s.nodes[id as usize];
            parts.push(n.name.clone());
            id = n.parent;
        }
        let mut p = s.root.clone();
        for part in parts.iter().rev() {
            p.push(part);
        }
        p
    }

    /// A folder's contents: sub-folders from the scan plus its own files,
    /// biggest first.
    pub fn children(&self, id: u32) -> Value {
        let g = self.scan.lock();
        let Some(s) = g.as_ref() else { return json!(null) };
        let Some(node) = s.nodes.get(id as usize) else { return json!(null) };
        let path = Self::path_of(s, id);
        let mut out: Vec<Entry> = node
            .children
            .iter()
            .map(|&c| {
                let n = &s.nodes[c as usize];
                Entry { id: Some(c), name: n.name.clone(), path: path.join(&n.name).to_string_lossy().to_string(), size: n.size, files: n.files, dir: true, keep: false }
            })
            .collect();
        if let Ok(rd) = std::fs::read_dir(&path) {
            for e in rd.flatten() {
                if let Ok(m) = e.metadata() {
                    if m.is_file() {
                        out.push(Entry { id: None, name: e.file_name().to_string_lossy().to_string(), path: e.path().to_string_lossy().to_string(), size: m.len(), files: 1, dir: false, keep: false });
                    }
                }
            }
        }
        out.sort_by(|a, b| b.size.cmp(&a.size));
        out.truncate(300);
        for e in &mut out {
            e.keep = protected(&e.path).is_some();
        }
        // breadcrumb
        let mut crumbs = Vec::new();
        let mut c = id;
        loop {
            let n = &s.nodes[c as usize];
            crumbs.push(json!({ "id": c, "name": if c == 0 { n.name.clone() } else { n.name.clone() } }));
            if c == 0 {
                break;
            }
            c = n.parent;
        }
        crumbs.reverse();
        json!({ "id": id, "path": path.to_string_lossy(), "size": node.size, "files": node.files, "items": out, "crumbs": crumbs })
    }

    pub fn big_files(&self) -> Vec<Value> {
        let g = self.scan.lock();
        g.as_ref()
            .map(|s| {
                s.big_files
                    .iter()
                    .filter(|(_, p)| Path::new(p).exists())
                    .map(|(size, p)| json!({ "path": p, "size": size, "name": Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()), "keep": protected(p).is_some() }))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Updates sizes after something was deleted, without rescanning.
    pub fn forget(&self, path: &str, size: u64) {
        let mut g = self.scan.lock();
        let Some(s) = g.as_mut() else { return };
        s.big_files.retain(|(_, p)| p != path);
        let target = Path::new(path);
        let Ok(rel) = target.strip_prefix(&s.root) else { return };
        let mut id = 0u32;
        let mut chain = vec![0u32];
        let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        for (i, comp) in comps.iter().enumerate() {
            let next = s.nodes[id as usize].children.iter().copied().find(|&c| s.nodes[c as usize].name.eq_ignore_ascii_case(comp));
            match next {
                Some(n) => {
                    id = n;
                    chain.push(n);
                }
                None if i == comps.len() - 1 => break, // a file inside `id`
                None => return,
            }
        }
        let last = *chain.last().unwrap();
        let removed_dir = last != 0 && s.nodes[last as usize].name.eq_ignore_ascii_case(comps.last().map(|s| s.as_str()).unwrap_or(""));
        let files = if removed_dir { s.nodes[last as usize].files } else { 1 };
        for &c in &chain {
            let n = &mut s.nodes[c as usize];
            n.size = n.size.saturating_sub(size);
            n.files = n.files.saturating_sub(files);
        }
        if removed_dir {
            let parent = s.nodes[last as usize].parent;
            s.nodes[parent as usize].children.retain(|&c| c != last);
        }
    }
}

/// Why a path must not be moved to the Recycle Bin from the Disk space page,
/// or None. Covers drive roots, Windows, the Program Files / ProgramData /
/// Users / profile / AppData folders themselves, and system files at a root.
pub fn protected(path: &str) -> Option<&'static str> {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let p = norm(path);
    let within = |base: &str| !base.is_empty() && (p == base || p.starts_with(&format!("{base}\\")));
    // "c:" (from "C:\") or nothing left
    if p.len() <= 2 {
        return Some("A whole drive can't be moved to the Recycle Bin");
    }
    let env = |k: &str| std::env::var(k).map(|v| norm(&v)).unwrap_or_default();
    if within(&env("SystemRoot")) {
        return Some("Windows' own files can't be moved to the Recycle Bin");
    }
    let drive = &p[..2];
    let mut exact = vec![env("ProgramFiles"), env("ProgramFiles(x86)"), env("ProgramData"), env("USERPROFILE"), env("APPDATA"), env("LOCALAPPDATA"), format!("{drive}\\users")];
    exact.retain(|b| !b.is_empty());
    if exact.iter().any(|b| p == *b) {
        return Some("This folder belongs to Windows; pick something inside it instead");
    }
    for name in ["system volume information", "$recycle.bin", "recovery", "boot", "pagefile.sys", "hiberfil.sys", "swapfile.sys"] {
        if within(&format!("{drive}\\{name}")) {
            return Some("Windows' own files can't be moved to the Recycle Bin");
        }
    }
    None
}

/// Moves files/folders to the Recycle Bin. If Windows can't keep something
/// there (too big for the bin, or a drive without one) it asks first instead
/// of deleting it for good.
pub fn recycle(paths: &[String]) -> Result<(), String> {
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW};
    if paths.is_empty() {
        return Ok(());
    }
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.encode_utf16());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        return Err(format!("Windows couldn't move it to the Recycle Bin (code {r}). It may be in use."));
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("Cancelled".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::protected;

    #[test]
    fn system_folders_are_never_recycled() {
        let win = std::env::var("SystemRoot").unwrap();
        let home = std::env::var("USERPROFILE").unwrap();
        let pf = std::env::var("ProgramFiles").unwrap();
        // forward slashes are fine too: Windows accepts both
        for p in ["C:/", "D:/", &win, &format!("{win}/System32"), &format!("{win}/system32/drivers/x.sys"), &home, &pf, "C:/Users", "c:/users/", "C:/System Volume Information", "C:/$Recycle.Bin/x", "C:/pagefile.sys"] {
            assert!(protected(p).is_some(), "{p} should be refused");
        }
        for p in [&format!("{home}/Downloads/big.iso"), &format!("{home}/Videos"), &format!("{pf}/SomeGame"), "D:/Games/Old", "C:/Windows.old.backup"] {
            assert!(protected(p).is_none(), "{p} should be allowed");
        }
    }
}
