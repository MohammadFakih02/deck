//! Sync through a folder that OneDrive / Google Drive / Dropbox keeps in sync, for when the PCs aren't on
//! the same network. Layout inside `<folder>/ClaudeSync-hub`:
//!   machines/<machine id>.ccm   each machine's manifest (only that machine writes it)
//!   chunks/<xx>/<name>          content chunks, write-once, named by a keyed hash
//! Every file is compressed and encrypted with the sync key, and no two machines ever write the same
//! file, so the cloud service never has to resolve a conflict.
use crate::ccsync::crypto::{deflate, hash, inflate, Key};
use crate::ccsync::model::{now_ms, Manifest, CHUNK};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

pub struct Hub {
    pub dir: PathBuf,
    key: Key,
}

fn write_atomic(p: &Path, data: &[u8]) -> io::Result<()> {
    crate::util::write_atomic(p, data)
}

impl Hub {
    pub fn new(base: &str, key: Key) -> io::Result<Hub> {
        let base = base.trim();
        if base.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no cloud folder set"));
        }
        let base = PathBuf::from(base);
        if !base.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("cloud folder {} not found", base.display())));
        }
        let dir = base.join("Deck-ClaudeSync-hub");
        std::fs::create_dir_all(dir.join("machines"))?;
        std::fs::create_dir_all(dir.join("chunks"))?;
        Ok(Hub { dir, key })
    }

    fn chunk_path(&self, name: &str) -> PathBuf {
        self.dir.join("chunks").join(&name[..2]).join(name)
    }

    /// names of every chunk file already in the folder
    pub fn existing(&self) -> HashSet<String> {
        let mut s = HashSet::new();
        if let Ok(rd) = std::fs::read_dir(self.dir.join("chunks")) {
            for d in rd.flatten() {
                if let Ok(rd2) = std::fs::read_dir(d.path()) {
                    for f in rd2.flatten() {
                        let n = f.file_name().to_string_lossy().into_owned();
                        if !n.ends_with(".ccsync-tmp") {
                            s.insert(n);
                        }
                    }
                }
            }
        }
        s
    }

    pub fn name(&self, chunk_hash: &str) -> String {
        self.key.blob_name(chunk_hash)
    }

    pub fn put_chunk(&self, h: &str, data: &[u8]) -> io::Result<()> {
        write_atomic(&self.chunk_path(&self.name(h)), &self.key.seal(&deflate(data)))
    }

    pub fn get_chunk(&self, h: &str) -> io::Result<Vec<u8>> {
        let raw = std::fs::read(self.chunk_path(&self.name(h)))?;
        let d = self.key.open(&raw).and_then(|p| inflate(&p, CHUNK)).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "chunk unreadable (different sync key?)"))?;
        if hash(&d) != h {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "chunk corrupted"));
        }
        Ok(d)
    }

    pub fn put_manifest(&self, m: &Manifest) -> io::Result<()> {
        let j = serde_json::to_vec(m)?;
        write_atomic(&self.dir.join("machines").join(format!("{}.ccm", m.machine_id)), &self.key.seal(&deflate(&j)))
    }

    /// other machines' manifests; ones written with a different key are skipped
    pub fn manifests(&self, my_id: &str) -> (Vec<Manifest>, usize) {
        let mut out = Vec::new();
        let mut unreadable = 0;
        if let Ok(rd) = std::fs::read_dir(self.dir.join("machines")) {
            for f in rd.flatten() {
                let n = f.file_name().to_string_lossy().into_owned();
                let Some(id) = n.strip_suffix(".ccm") else { continue };
                if id == my_id {
                    continue;
                }
                let m = std::fs::read(f.path()).ok().and_then(|b| self.key.open(&b)).and_then(|p| inflate(&p, 512 << 20)).and_then(|j| serde_json::from_slice::<Manifest>(&j).ok());
                match m {
                    Some(m) => out.push(m),
                    None => unreadable += 1,
                }
            }
        }
        (out, unreadable)
    }

    /// deletes chunks no manifest needs any more (only ones older than 3 days, so a manifest that is
    /// still uploading through the cloud service doesn't lose its chunks)
    pub fn gc(&self, my: &Manifest) -> usize {
        let (others, unreadable) = self.manifests(&my.machine_id);
        if unreadable > 0 {
            return 0; // can't see everything that's referenced
        }
        let mut keep: HashSet<String> = HashSet::new();
        for m in others.iter().chain(std::iter::once(my)) {
            for e in m.entries.values() {
                for c in &e.chunks {
                    keep.insert(self.name(c));
                }
            }
        }
        let cutoff = now_ms() - 3 * 86_400_000;
        let mut n = 0;
        for name in self.existing() {
            if keep.contains(&name) {
                continue;
            }
            let p = self.chunk_path(&name);
            let old = std::fs::metadata(&p).and_then(|m| m.modified()).map(|t| crate::ccsync::model::to_ms(t) < cutoff).unwrap_or(false);
            if old && std::fs::remove_file(&p).is_ok() {
                n += 1;
            }
        }
        n
    }
}
