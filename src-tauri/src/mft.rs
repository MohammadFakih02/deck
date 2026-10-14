//! WizTree-style drive scan: reads an NTFS drive's Master File Table (every file's name, parent
//! folder and size in one mostly sequential read) instead of listing folder after folder.
//! Read-only, needs admin (a raw volume handle), NTFS only; `disk.rs` falls back to listing
//! folders for anything else. Files with several hard links are counted once (the folder walk
//! counts each link, so Windows' WinSxS looks bigger there than it is).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub enum Error {
    /// not NTFS, not admin, or a layout this reader doesn't handle: list folders instead
    Unsupported(String),
    Cancelled,
}

/// One folder of the result, parents before children (index 0 is the drive's root).
pub struct Dir {
    pub name: String,
    pub parent: u32,
}

pub struct Tree {
    pub dirs: Vec<Dir>,
    /// every file: (folder index, size, name)
    pub files: Vec<(u32, u64, Box<str>)>,
}

#[derive(Default, Clone)]
struct Rec {
    parent: u32,
    /// bit 0 in use, bit 1 folder, bit 2 has a name
    flags: u8,
    size: u64,
    name: Option<Box<str>>,
}

const ROOT: u32 = 5;
const EXTEND: u32 = 11;

fn unsupported<T>(why: &str) -> Result<T, Error> {
    Err(Error::Unsupported(why.into()))
}

fn open_volume(letter: char) -> Result<File, Error> {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ | FILE_SHARE_WRITE: the drive stays in use while it's read
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(format!(r"\\.\{letter}:"))
        .map_err(|e| Error::Unsupported(format!("can't open the volume: {e}")))
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// Undoes NTFS's "update sequence": the last two bytes of every stride were swapped out on disk
/// to catch torn writes. False when the record is torn or not a FILE record.
fn fixup(rec: &mut [u8]) -> bool {
    if &rec[0..4] != b"FILE" {
        return false;
    }
    let (off, count) = (le16(rec, 4) as usize, le16(rec, 6) as usize);
    if count < 2 || off + count * 2 > rec.len() {
        return false;
    }
    let stride = rec.len() / (count - 1);
    let usn = [rec[off], rec[off + 1]];
    for i in 1..count {
        let end = i * stride;
        if end > rec.len() || rec[end - 2..end] != usn {
            return false;
        }
        rec[end - 2] = rec[off + i * 2];
        rec[end - 1] = rec[off + i * 2 + 1];
    }
    true
}

/// A non-resident attribute's data runs: (first cluster, cluster count), sparse runs skipped.
fn runs(attr: &[u8]) -> Vec<(u64, u64)> {
    let mut out = vec![];
    let mut at = le16(attr, 0x20) as usize;
    let mut lcn: i64 = 0;
    while at < attr.len() && attr[at] != 0 {
        let head = attr[at];
        let (nlen, noff) = ((head & 0x0F) as usize, (head >> 4) as usize);
        if nlen == 0 || nlen > 8 || noff > 8 || at + 1 + nlen + noff > attr.len() {
            break;
        }
        let mut len = 0u64;
        for i in 0..nlen {
            len |= (attr[at + 1 + i] as u64) << (8 * i);
        }
        if noff > 0 {
            let mut delta: i64 = 0;
            for i in 0..noff {
                delta |= (attr[at + 1 + nlen + i] as i64) << (8 * i);
            }
            let shift = 64 - 8 * noff as u32;
            delta = (delta << shift) >> shift; // sign-extend
            lcn += delta;
            out.push((lcn as u64, len));
        }
        at += 1 + nlen + noff;
    }
    out
}

/// The attributes of one (fixed-up) record: (type, attribute bytes).
fn attrs(rec: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let mut at = le16(rec, 0x14) as usize;
    std::iter::from_fn(move || {
        if at + 8 > rec.len() {
            return None;
        }
        let kind = le32(rec, at);
        let len = le32(rec, at + 4) as usize;
        if kind == 0xFFFF_FFFF || len < 16 || at + len > rec.len() {
            return None;
        }
        let a = &rec[at..at + len];
        at += len;
        Some((kind, a))
    })
}

/// Reads the whole Master File Table of drive `letter`.
pub fn scan(letter: char, cancel: &AtomicBool, files: &AtomicU64, bytes: &AtomicU64) -> Result<Tree, Error> {
    let mut vol = open_volume(letter)?;
    let mut boot = vec![0u8; 512];
    vol.read_exact(&mut boot).map_err(|e| Error::Unsupported(e.to_string()))?;
    if &boot[3..11] != b"NTFS    " {
        return unsupported("not NTFS");
    }
    let sector = le16(&boot, 0x0B) as u64;
    let spc = boot[0x0D];
    let cluster = sector * if spc > 0x80 { 1u64 << (256 - spc as u32) } else { spc as u64 };
    let mft_lcn = le64(&boot, 0x30);
    let cpr = boot[0x40] as i8;
    let rec_size = if cpr < 0 { 1u64 << (-cpr as u32) } else { cpr as u64 * cluster } as usize;
    if cluster == 0 || !(256..=65536).contains(&rec_size) || sector == 0 {
        return unsupported("unexpected NTFS layout");
    }

    // record 0 is $MFT itself: its data runs say where the table is
    let mut rec0 = vec![0u8; rec_size.max(sector as usize)];
    vol.seek(SeekFrom::Start(mft_lcn * cluster)).map_err(|e| Error::Unsupported(e.to_string()))?;
    vol.read_exact(&mut rec0).map_err(|e| Error::Unsupported(e.to_string()))?;
    rec0.truncate(rec_size);
    if !fixup(&mut rec0) {
        return unsupported("bad $MFT record");
    }
    let Some((mft_runs, mft_size)) = attrs(&rec0).find(|(k, a)| *k == 0x80 && a[8] == 1 && a[9] == 0).map(|(_, a)| (runs(a), le64(a, 0x30))) else {
        // a very fragmented $MFT keeps its runs in other records (an attribute list)
        return unsupported("$MFT data is in an attribute list");
    };
    let total = (mft_size / rec_size as u64) as usize;
    let mut recs: Vec<Rec> = vec![Rec::default(); total];

    // read the table run by run, in 4 MB chunks
    const CHUNK: u64 = 4 << 20;
    let mut buf = vec![0u8; CHUNK as usize];
    let mut index = 0usize;
    'outer: for (lcn, count) in mft_runs {
        let (mut pos, end) = (lcn * cluster, (lcn + count) * cluster);
        while pos < end && index < total {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = (end - pos).min(CHUNK) as usize;
            vol.seek(SeekFrom::Start(pos)).map_err(|e| Error::Unsupported(e.to_string()))?;
            vol.read_exact(&mut buf[..n]).map_err(|e| Error::Unsupported(e.to_string()))?;
            for rec in buf[..n].chunks_exact_mut(rec_size) {
                if index >= total {
                    break 'outer;
                }
                read_record(rec, index, &mut recs, files, bytes);
                index += 1;
            }
            pos += n as u64;
        }
    }
    Ok(build(recs))
}

fn read_record(rec: &mut [u8], index: usize, recs: &mut [Rec], files: &AtomicU64, bytes: &AtomicU64) {
    if !fixup(rec) {
        return;
    }
    let flags = le16(rec, 0x16);
    if flags & 1 == 0 {
        return; // not in use
    }
    let base = (le64(rec, 0x20) & 0xFFFF_FFFF_FFFF) as usize;
    let owner = if base != 0 { base } else { index };
    if owner >= recs.len() {
        return;
    }
    let mut name: Option<(u8, u32, String)> = None; // (namespace, parent, name)
    let mut size: Option<u64> = None;
    for (kind, a) in attrs(rec) {
        match kind {
            0x30 if a[8] == 0 => {
                let v = &a[le16(a, 0x14) as usize..];
                if v.len() < 0x42 {
                    continue;
                }
                let (len, ns) = (v[0x40] as usize, v[0x41]);
                if v.len() < 0x42 + len * 2 {
                    continue;
                }
                // the first long name wins (DOS 8.3 names only when there is nothing else);
                // another long name is a hard link, counted once
                if name.as_ref().is_some_and(|n| n.0 != 2) {
                    continue;
                }
                let parent = (le64(v, 0) & 0xFFFF_FFFF_FFFF) as u32;
                let units: Vec<u16> = v[0x42..0x42 + len * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                name = Some((ns, parent, String::from_utf16_lossy(&units)));
            }
            0x80 if a[9] == 0 => {
                // the unnamed data stream: resident (its value length) or the run starting at 0
                size = Some(if a[8] == 0 { le32(a, 0x10) as u64 } else if le64(a, 0x10) == 0 { le64(a, 0x30) } else { continue });
            }
            _ => {}
        }
    }
    let r = &mut recs[owner];
    if base == 0 {
        r.flags |= 1 | if flags & 2 != 0 { 2 } else { 0 };
    }
    if let Some(s) = size {
        r.size += s;
        if r.flags & 2 == 0 {
            files.fetch_add(1, Ordering::Relaxed);
            bytes.fetch_add(s, Ordering::Relaxed);
        }
    }
    if let Some((_, parent, n)) = name {
        if r.flags & 4 == 0 {
            r.flags |= 4;
            r.parent = parent;
            r.name = Some(n.into_boxed_str());
        }
    }
}

/// The folder tree from the records: folders reachable from the root, parents first; every
/// file's size added to its folder. NTFS's own metadata files ($MFT, $LogFile, $Extend, …) are
/// left out, as Explorer and the folder walk leave them out.
fn build(mut recs: Vec<Rec>) -> Tree {
    let n = recs.len();
    let is_dir = |r: &Rec| r.flags & 3 == 3 && r.flags & 4 != 0;
    let mut kids: Vec<Vec<u32>> = vec![vec![]; n];
    for (i, r) in recs.iter().enumerate() {
        let i = i as u32;
        if i != ROOT && i >= 16 && is_dir(r) && (r.parent as usize) < n && r.parent != i {
            kids[r.parent as usize].push(i);
        }
    }
    let mut node_of = vec![u32::MAX; n];
    let mut dirs = vec![Dir { name: String::new(), parent: u32::MAX }];
    node_of[ROOT as usize] = 0;
    let mut queue = std::collections::VecDeque::from([ROOT]);
    while let Some(d) = queue.pop_front() {
        for &k in &kids[d as usize] {
            if k == EXTEND {
                continue;
            }
            node_of[k as usize] = dirs.len() as u32;
            dirs.push(Dir { name: recs[k as usize].name.take().map(String::from).unwrap_or_default(), parent: node_of[d as usize] });
            queue.push_back(k);
        }
    }
    let mut files = Vec::with_capacity(n);
    for (i, r) in recs.iter_mut().enumerate() {
        if i < 16 || r.flags & 3 != 1 || r.flags & 4 == 0 {
            continue;
        }
        let Some(&node) = node_of.get(r.parent as usize) else { continue };
        if node == u32::MAX {
            continue;
        }
        files.push((node, r.size, r.name.take().unwrap_or_default()));
    }
    Tree { dirs, files }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_runs_decode_with_negative_offsets() {
        // header byte 0x21: 1-byte length, 2-byte offset. Run 1: 16 clusters at 0x1000;
        // run 2: 8 clusters at -0x100 relative (0x0F00); then a sparse run (no offset) skipped
        let mut attr = vec![0u8; 0x40];
        attr[0x20] = 0x40;
        attr.extend_from_slice(&[0x21, 0x10, 0x00, 0x10, 0x21, 0x08, 0x00, 0xFF, 0x01, 0x04, 0x00]);
        assert_eq!(runs(&attr), vec![(0x1000, 0x10), (0x0F00, 0x08)]);
    }

    #[test]
    fn fixup_restores_the_sector_ends() {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        rec[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        rec[6..8].copy_from_slice(&3u16.to_le_bytes()); // usn + 2 strides of 512
        rec[0x30..0x32].copy_from_slice(&[0xAB, 0xCD]);
        rec[0x32..0x34].copy_from_slice(&[1, 2]);
        rec[0x34..0x36].copy_from_slice(&[3, 4]);
        rec[510..512].copy_from_slice(&[0xAB, 0xCD]);
        rec[1022..1024].copy_from_slice(&[0xAB, 0xCD]);
        assert!(fixup(&mut rec));
        assert_eq!((&rec[510..512], &rec[1022..1024]), (&[1u8, 2][..], &[3u8, 4][..]));
        rec[0] = b'X';
        assert!(!fixup(&mut rec));
    }

    /// Live: the MFT scan against the folder walk on C: (needs admin).
    /// `cargo test --release mft_vs_walk -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn mft_vs_walk() {
        let letter = std::env::var("DECK_DRIVE").ok().and_then(|s| s.chars().next()).unwrap_or('C');
        let (c, f, b) = (AtomicBool::new(false), AtomicU64::new(0), AtomicU64::new(0));
        let t = std::time::Instant::now();
        let Ok(tree) = scan(letter, &c, &f, &b) else { panic!("MFT scan failed (admin? NTFS?)") };
        let secs = t.elapsed().as_secs_f32();
        let bytes: u64 = tree.files.iter().map(|f| f.1).sum();
        println!("MFT {letter}: {secs:.2} s, {} files, {} folders, {:.2} GB", tree.files.len(), tree.dirs.len(), bytes as f64 / 1e9);
    }
}
