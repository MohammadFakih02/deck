//! A small file-name index of the user's folders for the search box, and the
//! fuzzy scorer shared by all search results.

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", ".svn", "target", "appdata", "$recycle.bin", "__pycache__", ".gradle", ".cargo",
    ".rustup", ".nuget", ".npm", "venv", ".venv", "obj", "system volume information", ".vs", ".idea", ".cache",
    "site-packages", "build", "dist", ".next", ".turbo",
];
const MAX_ENTRIES: usize = 600_000;

#[derive(Clone)]
struct Entry {
    lower: String,
    path: Arc<str>,
    dir: bool,
}

#[derive(Serialize, Clone)]
pub struct FileHit {
    pub path: String,
    pub name: String,
    pub dir: bool,
    pub score: i32,
}

pub struct Index {
    entries: RwLock<Vec<Entry>>,
    pub building: AtomicBool,
    built: Mutex<Option<(Instant, usize, f32)>>,
}

impl Index {
    pub fn new() -> Self {
        Self { entries: RwLock::new(Vec::new()), building: AtomicBool::new(false), built: Mutex::new(None) }
    }

    /// Rebuilds in the background (no-op if a build is already running).
    pub fn rebuild(self: &Arc<Self>, roots: Vec<String>) {
        if self.building.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("file-index".into())
            .spawn(move || {
                let t = Instant::now();
                let mut out = Vec::with_capacity(100_000);
                'roots: for root in &roots {
                    let walk = jwalk::WalkDir::new(root)
                        .skip_hidden(true)
                        .follow_links(false)
                        .parallelism(jwalk::Parallelism::RayonNewPool(2))
                        .process_read_dir(|_, _, _, children| {
                            children.retain(|c| {
                                c.as_ref()
                                    .map(|e| !(e.file_type().is_dir() && SKIP_DIRS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str())))
                                    .unwrap_or(false)
                            });
                        });
                    for e in walk.into_iter().flatten() {
                        if e.depth() == 0 {
                            continue;
                        }
                        let name = e.file_name().to_string_lossy().to_string();
                        out.push(Entry {
                            lower: name.to_lowercase(),
                            path: Arc::from(e.path().to_string_lossy().as_ref()),
                            dir: e.file_type().is_dir(),
                        });
                        if out.len() >= MAX_ENTRIES {
                            break 'roots;
                        }
                    }
                }
                let n = out.len();
                *me.entries.write() = out;
                *me.built.lock() = Some((Instant::now(), n, t.elapsed().as_secs_f32()));
                me.building.store(false, Ordering::SeqCst);
            })
            .ok();
    }

    pub fn age_secs(&self) -> Option<u64> {
        self.built.lock().map(|(t, _, _)| t.elapsed().as_secs())
    }

    pub fn info(&self) -> serde_json::Value {
        let b = *self.built.lock();
        serde_json::json!({
            "building": self.building.load(Ordering::Relaxed),
            "count": b.map(|x| x.1),
            "took": b.map(|x| x.2),
            "age": b.map(|x| x.0.elapsed().as_secs()),
        })
    }

    pub fn search(&self, q: &str, limit: usize) -> Vec<FileHit> {
        let q = q.trim().to_lowercase();
        if q.len() < 2 {
            return Vec::new();
        }
        let entries = self.entries.read();
        let mut hits: Vec<(i32, &Entry)> = entries
            .iter()
            .filter_map(|e| {
                // Files must contain the query; fuzzy matching is too noisy here.
                let pos = e.lower.find(&q)?;
                let mut s = 100 - (e.lower.len() as i32 - q.len() as i32).min(60);
                if pos == 0 {
                    s += 40;
                } else if !e.lower.as_bytes()[pos - 1].is_ascii_alphanumeric() {
                    s += 20;
                }
                if e.dir {
                    s += 10;
                }
                s -= (e.path.matches('\\').count() as i32 * 2).min(30);
                Some((s, e))
            })
            .collect();
        hits.sort_by_key(|b| std::cmp::Reverse(b.0));
        hits.truncate(limit);
        hits.into_iter()
            .map(|(score, e)| FileHit {
                name: Path::new(&*e.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                path: e.path.to_string(),
                dir: e.dir,
                score,
            })
            .collect()
    }
}

/// Scores `name` against the query: prefix > word start > substring >
/// in-order letters (fuzzy, e.g. "vsc" -> "Visual Studio Code").
/// None = no match.
pub fn score(name: &str, q: &str) -> Option<i32> {
    let n = name.to_lowercase();
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    if n == q {
        return Some(1000);
    }
    if n.starts_with(&q) {
        return Some(800 - n.len() as i32);
    }
    // word starts
    let words: Vec<&str> = n.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| w.starts_with(&q)) {
        return Some(650 - n.len() as i32);
    }
    // initials: "vsc" -> visual studio code
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if initials.starts_with(&q) && q.len() >= 2 {
        return Some(600 - n.len() as i32);
    }
    if n.contains(&q) {
        return Some(450 - n.len() as i32);
    }
    // in-order letters, penalising gaps
    let mut it = n.chars().peekable();
    let mut gaps = 0;
    for qc in q.chars() {
        if qc == ' ' {
            continue;
        }
        let mut found = false;
        for c in it.by_ref() {
            if c == qc {
                found = true;
                break;
            }
            gaps += 1;
        }
        if !found {
            return None;
        }
    }
    (q.len() >= 2).then(|| 250 - gaps.min(200) - n.len() as i32)
}

/// `score`, plus one typo for names people type by hand (apps, games): two letters swapped or
/// one letter too many. A typo match always ranks below every real match.
pub fn score_loose(name: &str, q: &str) -> Option<i32> {
    if let Some(s) = score(name, q) {
        return Some(s);
    }
    let c: Vec<char> = q.trim().to_lowercase().chars().collect();
    if c.len() < 4 {
        return None;
    }
    let mut tries: Vec<String> = Vec::new();
    for i in 0..c.len() - 1 {
        if c[i] != c[i + 1] {
            let mut v = c.clone();
            v.swap(i, i + 1);
            tries.push(v.into_iter().collect());
        }
    }
    for i in 0..c.len() {
        tries.push(c.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, ch)| *ch).collect());
    }
    // only the strong kinds of match count (prefix, word start, initials, inside the name), so a
    // typo can't turn into a loose letters-in-order hit on some unrelated long name
    tries.iter().filter_map(|t| score(name, t).filter(|s| *s >= 300)).max().map(|s| s / 4)
}

#[cfg(test)]
mod tests {
    use super::{score, score_loose};

    #[test]
    fn one_typo_still_finds_the_app() {
        for (name, q) in [("Discord", "dsicord"), ("Discord", "discrod"), ("Steam", "steeam"), ("Spotify", "spotfiy"), ("Visual Studio Code", "visaul")] {
            assert!(score_loose(name, q).is_some(), "{q} -> {name}");
        }
        assert!(score_loose("Steam", "xyzw").is_none());
        assert!(score_loose("Notepad", "dsicord").is_none());
        // too short to guess
        assert!(score_loose("Steam", "tse").is_none());
        // a real match always beats a typo match
        assert!(score_loose("Discord", "disc").unwrap() > score_loose("Discord", "dsicord").unwrap());
        assert!(score_loose("Media Player Discovery", "dis").unwrap() > score_loose("Discord", "dsicord").unwrap());
    }

    #[test]
    fn fuzzy() {
        assert!(score("Visual Studio Code", "vsc").is_some());
        assert!(score("Visual Studio Code", "code").unwrap() > score("Visual Studio Code", "vsc").unwrap() - 100);
        assert!(score("Discord", "dis").unwrap() > score("Media Player Discovery", "dis").unwrap());
        assert!(score("Steam", "xyz").is_none());
        assert_eq!(score("Steam", "steam"), Some(1000));
    }
}
