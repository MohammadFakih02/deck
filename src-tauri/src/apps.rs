//! Installed apps (everything in the Start menu's "All apps", including Store
//! apps), read from the shell's AppsFolder, plus their icons.

use crate::util::{com_sta, take_pwstr};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::time::Instant;
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    HGDIOBJ,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItemImageFactory,
    SHCreateItemFromParsingName, SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};

#[derive(Serialize, Clone, Debug)]
pub struct AppEntry {
    /// AppsFolder parsing name: an AUMID or a (known-folder) path.
    pub id: String,
    pub name: String,
    /// Real file path when the entry is a program on disk.
    pub path: Option<String>,
    pub store: bool,
}

pub struct Apps {
    list: Mutex<(Option<Instant>, Vec<AppEntry>)>,
    icons: Mutex<HashMap<String, Option<Vec<u8>>>>,
    icon_tx: Mutex<Option<Sender<(String, Sender<Option<Vec<u8>>>)>>>,
}

/// Start-menu clutter that is never what you want to launch.
fn junk(name: &str, id: &str) -> bool {
    let n = name.to_lowercase();
    let i = id.to_lowercase();
    const WORDS: &[&str] = &[
        "uninstall", "readme", "read me", "release notes", "documentation", "help file", "license", "website",
        "user manual", "changelog", "what's new", "support center",
    ];
    WORDS.iter().any(|w| n.contains(w))
        || i.starts_with("http://")
        || i.starts_with("https://")
        || [".txt", ".pdf", ".html", ".htm", ".chm", ".url", ".rtf", ".md", ".ini", ".log"].iter().any(|e| i.ends_with(e))
}

/// Resolves "{GUID}\sub\app.exe" known-folder paths to real paths.
fn real_path(id: &str) -> Option<String> {
    if id.len() > 3 && id.as_bytes()[1] == b':' {
        return Some(id.to_string());
    }
    if id.starts_with('{') {
        let end = id.find('}')?;
        let guid = &id[1..end];
        let rest = id[end + 1..].trim_start_matches('\\');
        let base = match guid.to_uppercase().as_str() {
            "6D809377-6AF0-444B-8957-A3773F02200E" => std::env::var("ProgramW6432").or_else(|_| std::env::var("ProgramFiles")).ok()?,
            "7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E" => std::env::var("ProgramFiles(x86)").ok()?,
            "1AC14E77-02E7-4E5D-B744-2EB1AE5198B7" => format!("{}\\System32", std::env::var("SystemRoot").ok()?),
            "F38BF404-1D43-42F2-9305-67DE0B28FC23" => std::env::var("SystemRoot").ok()?,
            "D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27" => format!("{}\\SysWOW64", std::env::var("SystemRoot").ok()?),
            _ => return None,
        };
        return Some(format!("{base}\\{rest}"));
    }
    None
}

fn enumerate() -> Vec<AppEntry> {
    com_sta();
    let mut out = Vec::new();
    unsafe {
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else {
            return out;
        };
        let Ok(en) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut items = [None];
            let mut fetched = 0u32;
            if en.Next(&mut items, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = items[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(|p| take_pwstr(p)).unwrap_or_default();
            let id = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).map(|p| take_pwstr(p)).unwrap_or_default();
            if name.is_empty() || id.is_empty() || junk(&name, &id) {
                continue;
            }
            let path = real_path(&id);
            let store = path.is_none() && id.contains('!');
            out.push(AppEntry { id, name, path, store });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name && a.path == b.path);
    out
}

impl Apps {
    pub fn new() -> Self {
        Self { list: Mutex::new((None, Vec::new())), icons: Mutex::new(HashMap::new()), icon_tx: Mutex::new(None) }
    }

    /// Cached list; re-read every 5 minutes (or when `force`).
    pub fn list(&self, force: bool) -> Vec<AppEntry> {
        {
            let g = self.list.lock();
            if let Some(t) = g.0 {
                if !force && t.elapsed().as_secs() < 300 {
                    return g.1.clone();
                }
            }
        }
        // Enumerate on a fresh STA thread: shell objects are apartment-bound.
        let v = std::thread::spawn(enumerate).join().unwrap_or_default();
        *self.list.lock() = (Some(Instant::now()), v.clone());
        v
    }

    pub fn launch(&self, id: &str) -> Result<(), String> {
        crate::util::shell_open(&format!("shell:AppsFolder\\{id}"))
    }

    /// PNG icon for an app id ("app:<id>") or any file path ("file:<path>").
    /// Extraction runs on one STA worker thread.
    pub fn icon(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(v) = self.icons.lock().get(key) {
            return v.clone();
        }
        let tx = {
            let mut g = self.icon_tx.lock();
            if g.is_none() {
                let (tx, rx) = channel::<(String, Sender<Option<Vec<u8>>>)>();
                std::thread::spawn(move || {
                    com_sta();
                    for (k, reply) in rx {
                        let _ = reply.send(extract(&k));
                    }
                });
                *g = Some(tx);
            }
            g.clone().unwrap()
        };
        let (rtx, rrx) = channel();
        tx.send((key.to_string(), rtx)).ok()?;
        let png = rrx.recv_timeout(std::time::Duration::from_secs(5)).ok().flatten();
        let mut icons = self.icons.lock();
        if icons.len() > 3000 {
            icons.clear();
        }
        icons.insert(key.to_string(), png.clone());
        png
    }
}

fn extract(key: &str) -> Option<Vec<u8>> {
    let target = if let Some(id) = key.strip_prefix("app:") {
        format!("shell:AppsFolder\\{id}")
    } else {
        key.strip_prefix("file:")?.to_string()
    };
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(target), None).ok()?;
        let f: IShellItemImageFactory = windows::core::Interface::cast(&item).ok()?;
        let hbmp = f.GetImage(SIZE { cx: 64, cy: 64 }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let png = hbitmap_png(hbmp);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        png
    }
}

/// 32-bit HBITMAP (premultiplied BGRA) -> PNG.
unsafe fn hbitmap_png(hbmp: HBITMAP) -> Option<Vec<u8>> {
    let mut bm = BITMAP::default();
    if GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if w <= 0 || h <= 0 || w > 512 || h > 512 {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = windows::Win32::Graphics::Gdi::GetDC(None);
    let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
    windows::Win32::Graphics::Gdi::ReleaseDC(None, dc);
    if lines == 0 {
        return None;
    }
    let any_alpha = px.chunks(4).any(|p| p[3] != 0);
    for p in px.chunks_mut(4) {
        let (b, g, r, a) = (p[0], p[1], p[2], p[3]);
        if !any_alpha {
            p.copy_from_slice(&[r, g, b, 255]);
        } else if a == 0 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            p.copy_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    encode_png(&px, w as u32, h as u32)
}

pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(rgba).ok()?;
    }
    Some(out)
}
