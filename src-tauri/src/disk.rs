//! "What's eating my disk", like WizTree: every folder and file of a drive or folder, their
//! sizes and file types, and a nested treemap laid out here (the page only draws it).
//! A whole NTFS drive is read from its Master File Table (`mft.rs`: seconds, needs admin);
//! anything else (exFAT, a network drive, one folder, no admin) is listed folder by folder
//! with FindFirstFileEx on several threads, taking the sizes from the listing itself.

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
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
    /// folders inside it, at any depth
    folders: u64,
    children: Vec<u32>,
    /// its own files: a range of `Scan::files`
    own: (u32, u32),
}

#[derive(Clone, Copy)]
struct FileRec {
    parent: u32,
    ext: u16,
    name_len: u16,
    name: u32,
    size: u64,
}

#[derive(Default)]
struct Scan {
    root: PathBuf,
    nodes: Vec<Node>,
    files: Vec<FileRec>,
    /// every file name, one after another (FileRec::name / name_len point in here)
    names: String,
    /// file types (".pak", "" for none) with their bytes and count
    exts: Vec<(String, u64, u64)>,
    /// each type's colour slot: 1..=15 for the biggest types, 0 for the rest
    ext_color: Vec<u8>,
    errors: u64,
    took: f32,
    /// free space, when a whole drive was scanned
    free: Option<u64>,
    /// "MFT" (read from the drive's file table) or "folders" (listed folder by folder)
    method: &'static str,
    /// why a whole drive was listed folder by folder instead of read from its file table
    why_slow: Option<String>,
}

impl Scan {
    fn file_name(&self, f: &FileRec) -> &str {
        &self.names[f.name as usize..f.name as usize + f.name_len as usize]
    }
    fn own_files(&self, n: &Node) -> &[FileRec] {
        &self.files[n.own.0 as usize..n.own.1 as usize]
    }
}

/// Builds a Scan the same way for both readers: folders (parents before children), then files.
struct Builder {
    nodes: Vec<Node>,
    files: Vec<FileRec>,
    names: String,
    ext_ix: HashMap<String, u16>,
    exts: Vec<(String, u64, u64)>,
}

/// A file's type: the part after the last dot, lower case ("" when there is none or it looks
/// like part of the name).
fn ext_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && (1..=10).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') => format!(".{}", ext.to_ascii_lowercase()),
        _ => String::new(),
    }
}

impl Builder {
    fn new(root_name: String) -> Self {
        Self { nodes: vec![Node { name: root_name, parent: u32::MAX, size: 0, files: 0, folders: 0, children: vec![], own: (0, 0) }], files: vec![], names: String::new(), ext_ix: HashMap::new(), exts: vec![] }
    }

    fn dir(&mut self, parent: u32, name: String) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(Node { name, parent, size: 0, files: 0, folders: 0, children: vec![], own: (0, 0) });
        self.nodes[parent as usize].children.push(id);
        id
    }

    fn file(&mut self, parent: u32, name: &str, size: u64) {
        let ext = ext_of(name);
        let e = match self.ext_ix.get(&ext) {
            Some(&e) => e,
            None if self.exts.len() < u16::MAX as usize => {
                let e = self.exts.len() as u16;
                self.ext_ix.insert(ext.clone(), e);
                self.exts.push((ext, 0, 0));
                e
            }
            None => 0,
        };
        self.exts[e as usize].1 += size;
        self.exts[e as usize].2 += 1;
        let name = if name.len() > u16::MAX as usize { &name[..name.char_indices().take_while(|(i, _)| *i < 60_000).last().map(|(i, _)| i).unwrap_or(0)] } else { name };
        self.files.push(FileRec { parent, ext: e, name_len: name.len() as u16, name: self.names.len() as u32, size });
        self.names.push_str(name);
        let n = &mut self.nodes[parent as usize];
        n.size += size;
        n.files += 1;
    }

    fn finish(mut self, root: &Path, errors: u64, method: &'static str) -> Scan {
        self.files.sort_by_key(|f| f.parent);
        let mut i = 0usize;
        while i < self.files.len() {
            let p = self.files[i].parent;
            let start = i;
            while i < self.files.len() && self.files[i].parent == p {
                i += 1;
            }
            self.nodes[p as usize].own = (start as u32, i as u32);
        }
        // children were created after their parents: a reverse pass adds every total upwards
        for i in (1..self.nodes.len()).rev() {
            let (p, s, f, d) = (self.nodes[i].parent as usize, self.nodes[i].size, self.nodes[i].files, self.nodes[i].folders);
            self.nodes[p].size += s;
            self.nodes[p].files += f;
            self.nodes[p].folders += d + 1;
        }
        let mut order: Vec<usize> = (0..self.exts.len()).collect();
        order.sort_by(|a, b| self.exts[*b].1.cmp(&self.exts[*a].1));
        let mut ext_color = vec![0u8; self.exts.len()];
        for (slot, &e) in order.iter().take(15).enumerate() {
            ext_color[e] = slot as u8 + 1;
        }
        let free = drive_root(root).and_then(free_space);
        Scan { root: root.to_path_buf(), nodes: self.nodes, files: self.files, names: self.names, exts: self.exts, ext_color, errors, took: 0.0, free, method, why_slow: None }
    }
}

/// "C" when `p` is a whole drive ("C:\" or "C:").
fn drive_root(p: &Path) -> Option<char> {
    let s = p.to_string_lossy();
    let s = s.trim_end_matches(['\\', '/']);
    let mut c = s.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(l), Some(':'), None) if l.is_ascii_alphabetic() => Some(l.to_ascii_uppercase()),
        _ => None,
    }
}

fn free_space(letter: char) -> Option<u64> {
    let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain(Some(0)).collect();
    let mut free = 0u64;
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(root.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), &mut free) } != 0;
    ok.then_some(free)
}

#[derive(Serialize, Clone)]
pub struct Entry {
    pub id: Option<u32>,
    /// a file's index in the scan (for its path)
    pub file: Option<u32>,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub files: u64,
    pub folders: u64,
    pub dir: bool,
    /// a system folder: the page offers no Recycle Bin button
    pub keep: bool,
}

/// Colours of the treemap and the file-type list: slot 0 is "other types".
pub const PALETTE: [&str; 16] = ["#8a8f9c", "#e07a1f", "#d6c21a", "#2fb34a", "#9b4fd6", "#e0457b", "#2f8fe0", "#3fc6d8", "#c94fc4", "#d83a2f", "#7fae2e", "#1fb39a", "#a8702e", "#5a6ee0", "#e0a64f", "#6fd08a"];

/// Treemap rectangles at most: past it, folders are drawn as one block.
const MAX_RECTS: usize = 40_000;

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
            "free": s.as_ref().and_then(|s| s.free),
            "method": s.as_ref().map(|s| s.method),
            "why_slow": s.as_ref().and_then(|s| s.why_slow.clone()),
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
                // drop the last scan first: two whole drives in memory at once isn't needed
                *me.scan.lock() = None;
                let result = me.scan_path(Path::new(&root));
                if let Some(mut s) = result {
                    s.took = t.elapsed().as_secs_f32();
                    *me.scan.lock() = Some(s);
                }
                me.running.store(false, Ordering::SeqCst);
                done();
            })
            .ok();
    }

    /// The Master File Table for a whole NTFS drive, else the folder walk.
    fn scan_path(&self, root: &Path) -> Option<Scan> {
        if let Some(letter) = drive_root(root) {
            match crate::mft::scan(letter, &self.cancel, &self.progress_files, &self.progress_bytes) {
                Ok(tree) => {
                    let mut b = Builder::new(root.to_string_lossy().to_string());
                    // mft::Tree lists folders parents first, root at 0
                    for d in tree.dirs.into_iter().skip(1) {
                        b.dir(d.parent, d.name);
                    }
                    for (dir, size, name) in tree.files {
                        b.file(dir, &name, size);
                    }
                    return Some(b.finish(root, 0, "MFT"));
                }
                Err(crate::mft::Error::Cancelled) => return None,
                Err(crate::mft::Error::Unsupported(why)) => {
                    self.progress_files.store(0, Ordering::Relaxed);
                    self.progress_bytes.store(0, Ordering::Relaxed);
                    return self.walk(root).map(|mut s| {
                        s.why_slow = Some(why);
                        s
                    });
                }
            }
        }
        self.walk(root)
    }

    /// The folder walk: several threads list folders with FindFirstFileEx (large fetch, no short
    /// names) and take each file's size from the listing, so no file is ever opened. This
    /// thread is the collector: it owns the tree and queues the sub-folders it learns of.
    fn walk(&self, root: &Path) -> Option<Scan> {
        use std::collections::VecDeque;
        use std::sync::mpsc;
        struct Queue {
            jobs: VecDeque<(u32, PathBuf)>,
            done: bool,
        }
        let queue = (Mutex::new(Queue { jobs: VecDeque::from([(0, root.to_path_buf())]), done: false }), parking_lot::Condvar::new());
        let (tx, rx) = mpsc::channel::<(u32, PathBuf, Option<Listing>)>();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(4, 16);
        std::thread::scope(|sc| {
            for _ in 0..threads {
                let (queue, tx) = (&queue, tx.clone());
                sc.spawn(move || loop {
                    let job = {
                        let (m, cv) = queue;
                        let mut q = m.lock();
                        loop {
                            if let Some(j) = q.jobs.pop_front() {
                                break Some(j);
                            }
                            if q.done {
                                break None;
                            }
                            cv.wait(&mut q);
                        }
                    };
                    let Some((id, path)) = job else { return };
                    let listing = if self.cancel.load(Ordering::Relaxed) { None } else { list_dir(&path) };
                    if tx.send((id, path, listing)).is_err() {
                        return;
                    }
                });
            }
            drop(tx);
            let finish = |cancelled: bool| {
                let (m, cv) = &queue;
                let mut q = m.lock();
                q.done = true;
                if cancelled {
                    q.jobs.clear();
                }
                cv.notify_all();
            };
            let mut b = Builder::new(root.to_string_lossy().to_string());
            let mut errors = 0u64;
            let mut pending = 1usize;
            while pending > 0 {
                let Ok((id, path, listing)) = rx.recv() else { break };
                pending -= 1;
                if self.cancel.load(Ordering::Relaxed) {
                    finish(true);
                    return None;
                }
                let Some(l) = listing else {
                    errors += 1;
                    continue;
                };
                let mut bytes = 0u64;
                for (name, size) in &l.files {
                    b.file(id, name, *size);
                    bytes += size;
                }
                self.progress_files.fetch_add(l.files.len() as u64, Ordering::Relaxed);
                self.progress_bytes.fetch_add(bytes, Ordering::Relaxed);
                if !l.dirs.is_empty() {
                    let (m, cv) = &queue;
                    let mut q = m.lock();
                    for name in l.dirs {
                        let path = path.join(&name);
                        let child = b.dir(id, name);
                        q.jobs.push_back((child, path));
                        pending += 1;
                    }
                    cv.notify_all();
                }
            }
            finish(false);
            Some(b.finish(root, errors, "folders"))
        })
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

    /// A folder's contents from the scan: its folders and its own files, biggest first.
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
                Entry { id: Some(c), file: None, name: n.name.clone(), path: path.join(&n.name).to_string_lossy().to_string(), size: n.size, files: n.files, folders: n.folders, dir: true, keep: false }
            })
            .collect();
        let mut own: Vec<(usize, &FileRec)> = (node.own.0 as usize..node.own.1 as usize).map(|i| (i, &s.files[i])).collect();
        own.sort_by(|a, b| b.1.size.cmp(&a.1.size));
        for (i, f) in own.into_iter().take(300) {
            let name = s.file_name(f).to_string();
            out.push(Entry { id: None, file: Some(i as u32), path: path.join(&name).to_string_lossy().to_string(), name, size: f.size, files: 1, folders: 0, dir: false, keep: false });
        }
        out.sort_by(|a, b| b.size.cmp(&a.size));
        out.truncate(300);
        for e in &mut out {
            e.keep = protected(&e.path).is_some();
        }
        let mut crumbs = Vec::new();
        let mut c = id;
        loop {
            let n = &s.nodes[c as usize];
            crumbs.push(json!({ "id": c, "name": n.name }));
            if c == 0 {
                break;
            }
            c = n.parent;
        }
        crumbs.reverse();
        json!({ "id": id, "path": path.to_string_lossy(), "size": node.size, "files": node.files, "folders": node.folders, "items": out, "crumbs": crumbs })
    }

    /// The biggest files of the scan, with their paths.
    pub fn big_files(&self) -> Vec<Value> {
        let g = self.scan.lock();
        let Some(s) = g.as_ref() else { return vec![] };
        let mut top: Vec<&FileRec> = s.files.iter().filter(|f| f.size > 50_000_000).collect();
        top.sort_by(|a, b| b.size.cmp(&a.size));
        top.into_iter()
            .take(100)
            .map(|f| {
                let p = Self::path_of(s, f.parent).join(s.file_name(f)).to_string_lossy().to_string();
                json!({ "path": p, "size": f.size, "name": s.file_name(f), "keep": protected(&p).is_some() })
            })
            .filter(|v| Path::new(v["path"].as_str().unwrap_or("")).exists())
            .collect()
    }

    /// One file's full path (a treemap cell).
    pub fn file_path(&self, file: u32) -> Option<String> {
        let g = self.scan.lock();
        let s = g.as_ref()?;
        let f = s.files.get(file as usize)?;
        Some(Self::path_of(s, f.parent).join(s.file_name(f)).to_string_lossy().to_string())
    }

    /// One folder's full path (a treemap cell).
    pub fn dir_path(&self, id: u32) -> Option<String> {
        let g = self.scan.lock();
        let s = g.as_ref()?;
        s.nodes.get(id as usize)?;
        Some(Self::path_of(s, id).to_string_lossy().to_string())
    }

    /// The file types under folder `id`: the biggest 24 by size, with their colours.
    pub fn types(&self, id: u32) -> Value {
        let g = self.scan.lock();
        let Some(s) = g.as_ref() else { return json!([]) };
        let mut per: HashMap<u16, (u64, u64)> = HashMap::new();
        let mut stack = vec![id];
        while let Some(d) = stack.pop() {
            let Some(n) = s.nodes.get(d as usize) else { continue };
            for f in s.own_files(n) {
                let e = per.entry(f.ext).or_default();
                e.0 += f.size;
                e.1 += 1;
            }
            stack.extend(n.children.iter().copied());
        }
        let mut v: Vec<(u16, (u64, u64))> = per.into_iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        json!(v.into_iter().take(24).map(|(e, (bytes, count))| json!({
            "ext": s.exts[e as usize].0, "bytes": bytes, "count": count, "color": PALETTE[s.ext_color[e as usize] as usize],
        })).collect::<Vec<_>>())
    }

    /// The nested treemap of folder `id` for a w × h canvas, like WizTree: every folder a frame
    /// with a label strip, files coloured by type, files too small to see lumped together, and the
    /// drive's free space beside a whole drive. Flat arrays: rects (x, y, w, h), kinds (0 folder,
    /// 1 file, 2 free space, 3 small files), colours, depths, refs (folder id / file index), names, sizes.
    pub fn treemap(&self, id: u32, w: f64, h: f64) -> Value {
        let g = self.scan.lock();
        let Some(s) = g.as_ref() else { return json!(null) };
        if s.nodes.get(id as usize).is_none() || w < 4.0 || h < 4.0 {
            return json!(null);
        }
        let mut t = Tm::default();
        let node = &s.nodes[id as usize];
        let free = if id == 0 { s.free.unwrap_or(0) } else { 0 };
        if free > 0 && node.size > 0 {
            let r = squarify(&[node.size as f64, free as f64].map(|v| v * w * h / (node.size + free) as f64), 0.0, 0.0, w, h);
            lay(s, id, r[0], 0, &mut t);
            t.push(r[1], 2, 0, 0, -1, "Free space".into(), free);
        } else {
            lay(s, id, [0.0, 0.0, w, h], 0, &mut t);
        }
        json!({ "r": t.rects, "k": t.kinds, "c": t.colors, "d": t.depths, "ref": t.refs, "n": t.names, "s": t.sizes, "palette": PALETTE })
    }

    /// Updates sizes after something was deleted, without rescanning.
    pub fn forget(&self, path: &str, size: u64) {
        let mut g = self.scan.lock();
        let Some(s) = g.as_mut() else { return };
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
        let (files, folders) = if removed_dir { (s.nodes[last as usize].files, s.nodes[last as usize].folders + 1) } else { (1, 0) };
        if !removed_dir {
            // the file itself: size 0, so the treemap and the types forget it
            let name = comps.last().cloned().unwrap_or_default();
            let n = s.nodes[last as usize].own;
            if let Some(i) = (n.0..n.1).find(|&i| s.file_name(&s.files[i as usize]).eq_ignore_ascii_case(&name)) {
                let f = &mut s.files[i as usize];
                let (e, sz) = (f.ext as usize, f.size);
                f.size = 0;
                s.exts[e].1 = s.exts[e].1.saturating_sub(sz);
                s.exts[e].2 = s.exts[e].2.saturating_sub(1);
            }
        }
        for &c in &chain {
            let n = &mut s.nodes[c as usize];
            n.size = n.size.saturating_sub(size);
            n.files = n.files.saturating_sub(files);
            if c != last || !removed_dir {
                n.folders = n.folders.saturating_sub(folders);
            }
        }
        if removed_dir {
            let parent = s.nodes[last as usize].parent;
            s.nodes[parent as usize].children.retain(|&c| c != last);
        }
    }
}

#[derive(Default)]
struct Tm {
    rects: Vec<f32>,
    kinds: Vec<u8>,
    colors: Vec<u8>,
    depths: Vec<u8>,
    refs: Vec<i64>,
    names: Vec<String>,
    sizes: Vec<u64>,
}

impl Tm {
    fn push(&mut self, r: [f64; 4], kind: u8, color: u8, depth: u8, rf: i64, name: String, size: u64) {
        self.rects.extend(r.map(|v| (v * 10.0).round() as f32 / 10.0));
        self.kinds.push(kind);
        self.colors.push(color);
        self.depths.push(depth);
        self.refs.push(rf);
        self.names.push(name);
        self.sizes.push(size);
    }
}

enum Item {
    Dir(u32),
    File(u32),
    /// files and folders too small to see: (count)
    Rest(u64),
}

/// One folder: its frame, a label strip when there is room, and its contents squarified inside.
fn lay(s: &Scan, id: u32, r: [f64; 4], depth: u8, t: &mut Tm) {
    let n = &s.nodes[id as usize];
    let [x, y, w, h] = r;
    t.push(r, 0, 0, depth, id as i64, n.name.clone(), n.size);
    if w < 3.0 || h < 3.0 || t.kinds.len() >= MAX_RECTS || n.size == 0 {
        return;
    }
    let head = if h >= 30.0 && w >= 40.0 { 13.0 } else { 0.0 };
    let pad = if w >= 8.0 && h >= 8.0 { 1.5 } else { 0.0 };
    let (ix, iy, iw, ih) = (x + pad, y + head + pad, w - 2.0 * pad, h - head - 2.0 * pad);
    if iw < 1.0 || ih < 1.0 {
        return;
    }
    let mut items: Vec<(u64, Item)> = n.children.iter().map(|&c| (s.nodes[c as usize].size, Item::Dir(c))).chain((n.own.0..n.own.1).map(|i| (s.files[i as usize].size, Item::File(i)))).filter(|(sz, _)| *sz > 0).collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let total: u64 = items.iter().map(|i| i.0).sum();
    if total == 0 {
        return;
    }
    let scale = iw * ih / total as f64;
    // cells under ~3 px² are lumped into one "small files" cell
    if let Some(cut) = items.iter().position(|(sz, _)| (*sz as f64) * scale < 3.0) {
        if items.len() - cut > 1 {
            let rest: Vec<(u64, Item)> = items.drain(cut..).collect();
            let bytes = rest.iter().map(|r| r.0).sum();
            let count = rest.iter().map(|r| if let Item::Dir(d) = r.1 { s.nodes[d as usize].files } else { 1 }).sum();
            items.push((bytes, Item::Rest(count)));
        }
    }
    let areas: Vec<f64> = items.iter().map(|(sz, _)| *sz as f64 * scale).collect();
    let rects = squarify(&areas, ix, iy, iw, ih);
    for ((sz, item), rr) in items.into_iter().zip(rects) {
        match item {
            Item::Dir(c) => lay(s, c, rr, depth.saturating_add(1), t),
            Item::File(i) => {
                let f = &s.files[i as usize];
                t.push(rr, 1, s.ext_color[f.ext as usize], depth + 1, i as i64, s.file_name(f).to_string(), sz);
            }
            Item::Rest(count) => t.push(rr, 3, 0, depth + 1, -1, format!("{count} small files"), sz),
        }
        if t.kinds.len() >= MAX_RECTS {
            break;
        }
    }
}

/// Squarified treemap (Bruls, Huizing, van Wijk): `areas` biggest first, already in px².
fn squarify(areas: &[f64], mut x: f64, mut y: f64, mut w: f64, mut h: f64) -> Vec<[f64; 4]> {
    let mut out = Vec::with_capacity(areas.len());
    let mut i = 0;
    while i < areas.len() {
        let short = w.min(h);
        if short <= 0.0 {
            out.push([x, y, 0.0, 0.0]);
            i += 1;
            continue;
        }
        let (mut j, mut sum, mut best) = (i, 0.0f64, f64::MAX);
        while j < areas.len() {
            let ns = sum + areas[j];
            let worst = (short * short * areas[i] / (ns * ns)).max(ns * ns / (short * short * areas[j]));
            if worst > best {
                break;
            }
            best = worst;
            sum = ns;
            j += 1;
        }
        let thick = sum / short;
        if w >= h {
            let mut yy = y;
            for a in &areas[i..j] {
                let hh = a / thick;
                out.push([x, yy, thick, hh]);
                yy += hh;
            }
            x += thick;
            w -= thick;
        } else {
            let mut xx = x;
            for a in &areas[i..j] {
                let ww = a / thick;
                out.push([xx, y, ww, thick]);
                xx += ww;
            }
            y += thick;
            h -= thick;
        }
        i = j;
    }
    out
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

/// One folder's direct contents, as the scan needs them.
#[derive(Default)]
struct Listing {
    dirs: Vec<String>,
    /// (name, size)
    files: Vec<(String, u64)>,
}

/// Lists one folder with FindFirstFileEx; the sizes come with the listing, so no file is opened.
/// Symbolic links and junctions are skipped, as before (no folder counted twice); other reparse
/// points (OneDrive files, deduplicated files) count as ordinary files and folders.
fn list_dir(dir: &Path) -> Option<Listing> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW,
    };
    const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    let pattern: Vec<u16> = long_path(dir).encode_wide().chain("\\*".encode_utf16()).chain(Some(0)).collect();
    let mut fd: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let h = unsafe { FindFirstFileExW(pattern.as_ptr(), FindExInfoBasic, &mut fd as *mut _ as *mut _, FindExSearchNameMatch, std::ptr::null(), FIND_FIRST_EX_LARGE_FETCH) };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut out = Listing::default();
    loop {
        let len = fd.cFileName.iter().position(|&c| c == 0).unwrap_or(fd.cFileName.len());
        let name = &fd.cFileName[..len];
        let dot = name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16];
        let link = fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 && matches!(fd.dwReserved0, IO_REPARSE_TAG_SYMLINK | IO_REPARSE_TAG_MOUNT_POINT);
        if !dot && !link {
            let name = std::ffi::OsString::from_wide(name).to_string_lossy().to_string();
            if fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                out.dirs.push(name);
            } else {
                let size = ((fd.nFileSizeHigh as u64) << 32) | fd.nFileSizeLow as u64;
                out.files.push((name, size));
            }
        }
        if unsafe { FindNextFileW(h, &mut fd) } == 0 {
            break;
        }
    }
    unsafe { FindClose(h) };
    Some(out)
}

/// The `\\?\` form, so folders deeper than 260 characters still list.
fn long_path(p: &Path) -> std::ffi::OsString {
    let s = p.as_os_str().to_string_lossy().replace('/', "\\");
    let s = s.trim_end_matches('\\');
    if s.starts_with(r"\\?\") {
        s.into()
    } else if let Some(unc) = s.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{unc}").into()
    } else {
        format!(r"\\?\{s}").into()
    }
}

#[cfg(test)]
mod tests {
    use super::protected;

    /// Sizes and counts come from the listing, deep folders (past 260 characters) are read, and
    /// a junction is not followed (its target would be counted twice).
    #[test]
    fn scan_counts_sizes_deep_folders_and_skips_junctions() {
        let root = std::env::temp_dir().join(format!("deck-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let deep = (0..12).fold(root.join("a"), |p, i| p.join(format!("folder-with-a-long-name-{i:02}")));
        std::fs::create_dir_all(&deep).unwrap();
        assert!(deep.as_os_str().len() > 260, "the test folder must pass MAX_PATH");
        std::fs::write(root.join("one.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(root.join("a").join("two.bin"), vec![0u8; 2345]).unwrap();
        std::fs::write(super::long_path(&deep.join("deep.bin")), vec![0u8; 777]).unwrap();
        let junction = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(root.join("j")).arg(root.join("a")).output().map(|o| o.status.success()).unwrap_or(false);
        let d = super::Disk::new();
        let s = d.walk(&root).unwrap();
        let _ = std::process::Command::new("cmd").args(["/c", "rmdir"]).arg(root.join("j")).output();
        let _ = std::fs::remove_dir_all(super::long_path(&root));
        assert_eq!((s.nodes[0].size, s.nodes[0].files), (1000 + 2345 + 777, 3), "junction made: {junction}");
        assert_eq!(s.nodes.len(), 1 + 1 + 12, "root, a and the 12 deep folders; the junction is skipped");
        assert_eq!(s.nodes[0].folders, 13);
        // the treemap of that scan: the root folder, its folder "a" with the 12 nested ones,
        // and both files directly visible; every file cell inside the canvas
        let d2 = super::Disk::new();
        *d2.scan.lock() = Some(s);
        let tm = d2.treemap(0, 800.0, 600.0);
        let kinds: Vec<u64> = tm["k"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == 1).count(), 3, "{kinds:?}");
        assert_eq!(kinds.iter().filter(|k| **k == 0).count(), 14);
        let r: Vec<f64> = tm["r"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!(r.chunks(4).all(|c| c[0] >= 0.0 && c[1] >= 0.0 && c[0] + c[2] <= 800.1 && c[1] + c[3] <= 600.1));
        let s = d2.scan.lock().take().unwrap();
        assert_eq!(s.files.len(), 3);
        assert_eq!(s.exts.iter().find(|e| e.0 == ".bin").map(|e| (e.1, e.2)), Some((1000 + 2345 + 777, 3)));
        assert_eq!(s.errors, 0);
    }

    /// Live timing of a whole scan: `DECK_SCAN=C:\ cargo test --release scan_speed -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn scan_speed() {
        let root = std::env::var("DECK_SCAN").unwrap_or_else(|_| "C:\\".into());
        let d = super::Disk::new();
        let t = std::time::Instant::now();
        let s = d.walk(std::path::Path::new(&root)).unwrap();
        let n = &s.nodes[0];
        println!("SCAN {root}: {:.2} s, {} files, {} folders, {:.2} GB, {} errors", t.elapsed().as_secs_f32(), n.files, s.nodes.len(), n.size as f64 / 1e9, s.errors);
    }

    #[test]
    fn squarify_fills_the_area_and_keeps_proportions() {
        let areas = [600.0, 300.0, 200.0, 100.0, 50.0, 50.0];
        let r = super::squarify(&areas, 0.0, 0.0, 40.0, 32.5);
        assert_eq!(r.len(), areas.len());
        for (a, [_, _, w, h]) in areas.iter().zip(&r) {
            assert!((w * h - a).abs() < 1e-6, "{w}x{h} for {a}");
        }
        for [x, y, w, h] in &r {
            assert!(*x >= -1e-9 && *y >= -1e-9 && x + w <= 40.0 + 1e-6 && y + h <= 32.5 + 1e-6);
        }
    }

    #[test]
    fn file_types() {
        assert_eq!(super::ext_of("pakchunk70.PAK"), ".pak");
        assert_eq!(super::ext_of(".gitignore"), "");
        assert_eq!(super::ext_of("README"), "");
        assert_eq!(super::ext_of("archive.tar.gz"), ".gz");
        assert_eq!(super::ext_of("My file.with spaces"), "");
    }

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
