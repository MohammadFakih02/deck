//! Clipboard history (text, images, copied files) with pins and search.
//!
//! A hidden message-only window listens for clipboard changes. Content that
//! password managers mark as private ("ExcludeClipboardContentFromMonitor-
//! Processing", "CanIncludeInClipboardHistory" = 0) is never stored.

use crate::util::{from_wide, now_ms, wide};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_BITFIELDS, BI_RGB};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
};

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct Item {
    pub id: u64,
    /// "text" | "image" | "files"
    pub kind: String,
    pub text: String,
    pub files: Vec<String>,
    /// PNG file name inside the clips folder.
    pub image: Option<String>,
    pub w: u32,
    pub h: u32,
    pub hash: u64,
    pub ts: u64,
    pub pinned: bool,
    pub app: Option<String>,
}

pub struct Clip {
    items: Mutex<Vec<Item>>,
    dir: PathBuf,
    pub max: Mutex<usize>,
    pub enabled: Mutex<bool>,
    pub images: Mutex<bool>,
    notify: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

static HWND_CLIP: AtomicIsize = AtomicIsize::new(0);
static TX: OnceLock<Sender<()>> = OnceLock::new();
/// Our own writes: the history already has these, so they only move to the top.
static OWN_WRITE: Mutex<Option<u64>> = Mutex::new(None);

/// Text over this many bytes isn't kept.
const MAX_TEXT: usize = 1_000_000;
/// Unpinned images kept (each is a PNG on disk, screenshots are megabytes).
const MAX_IMAGES: usize = 150;

/// Drops unpinned items beyond `max`, and unpinned images beyond `max_images` (newest first
/// in `items`). Returns the image files to delete.
fn trim(items: &mut Vec<Item>, max: usize, max_images: usize) -> Vec<String> {
    let (mut kept, mut images) = (0, 0);
    let mut removed = Vec::new();
    items.retain(|i| {
        if i.pinned {
            return true;
        }
        // an image over its own limit goes without taking a place in the overall count
        if i.kind == "image" {
            images += 1;
        }
        let keep = (i.kind != "image" || images <= max_images) && {
            kept += 1;
            kept <= max
        };
        if !keep {
            removed.extend(i.image.clone());
        }
        keep
    });
    removed
}

fn hash_of<T: Hash + ?Sized>(t: &T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        if let Some(tx) = TX.get() {
            let _ = tx.send(());
        }
        return LRESULT(0);
    }
    DefWindowProcW(h, msg, w, l)
}

fn clip_hwnd() -> Option<HWND> {
    let v = HWND_CLIP.load(Ordering::Relaxed);
    (v != 0).then(|| HWND(v as *mut _))
}

/// Opens the clipboard, retrying briefly while another app holds it.
fn open() -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(clip_hwnd()) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    false
}

fn reg_format(name: &str) -> u32 {
    let w = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
}

fn available(fmt: u32) -> bool {
    unsafe { IsClipboardFormatAvailable(fmt).is_ok() }
}

fn global_bytes(h: HANDLE) -> Option<Vec<u8>> {
    unsafe {
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g);
        if p.is_null() {
            return None;
        }
        let n = GlobalSize(g);
        let v = std::slice::from_raw_parts(p as *const u8, n).to_vec();
        let _ = GlobalUnlock(g);
        Some(v)
    }
}

/// What is on the clipboard right now (None for private or unsupported content).
fn read_now(want_images: bool) -> Option<(String, String, Vec<String>, Option<(Vec<u8>, u32, u32)>)> {
    if available(reg_format("ExcludeClipboardContentFromMonitorProcessing")) || available(reg_format("Clipboard Viewer Ignore")) {
        return None;
    }
    if !open() {
        return None;
    }
    let out = unsafe {
        // "CanIncludeInClipboardHistory" = 0 means "don't keep this".
        let chh = reg_format("CanIncludeInClipboardHistory");
        let private = available(chh)
            && GetClipboardData(chh).ok().and_then(global_bytes).map(|b| b.len() >= 4 && b[..4] == [0, 0, 0, 0]).unwrap_or(false);
        if private {
            None
        } else if available(CF_HDROP.0 as u32) {
            GetClipboardData(CF_HDROP.0 as u32).ok().map(|h| {
                let drop = HDROP(h.0);
                let n = DragQueryFileW(drop, u32::MAX, None);
                let files: Vec<String> = (0..n.min(500))
                    .map(|i| {
                        let mut buf = vec![0u16; 1024];
                        let len = DragQueryFileW(drop, i, Some(&mut buf));
                        String::from_utf16_lossy(&buf[..len as usize])
                    })
                    .collect();
                ("files".to_string(), String::new(), files, None)
            })
        } else if available(CF_UNICODETEXT.0 as u32) {
            GetClipboardData(CF_UNICODETEXT.0 as u32).ok().and_then(global_bytes).map(|b| {
                let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                ("text".to_string(), from_wide(&w), Vec::new(), None)
            })
            // too big to keep: a cut-down copy would paste back silently incomplete
            .filter(|(_, t, _, _)| t.len() <= MAX_TEXT)
        } else if want_images && available(CF_DIB.0 as u32) {
            GetClipboardData(CF_DIB.0 as u32).ok().and_then(global_bytes).and_then(|b| dib_to_rgba(&b)).map(|(px, w, h)| ("image".to_string(), String::new(), Vec::new(), Some((px, w, h))))
        } else {
            None
        }
    };
    unsafe {
        let _ = CloseClipboard();
    }
    out
}

/// CF_DIB (BITMAPINFOHEADER + pixels) -> top-down RGBA.
fn dib_to_rgba(b: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if b.len() < std::mem::size_of::<BITMAPINFOHEADER>() {
        return None;
    }
    let hdr: BITMAPINFOHEADER = unsafe { std::ptr::read_unaligned(b.as_ptr() as *const _) };
    let w = hdr.biWidth;
    let h = hdr.biHeight.abs();
    if w <= 0 || h <= 0 || w > 8192 || h > 8192 {
        return None;
    }
    let bottom_up = hdr.biHeight > 0;
    let bpp = hdr.biBitCount as usize;
    if bpp != 24 && bpp != 32 {
        return None;
    }
    let mut off = hdr.biSize as usize;
    if hdr.biCompression == BI_BITFIELDS.0 && hdr.biSize == 40 {
        off += 12;
    } else if hdr.biCompression != BI_RGB.0 && hdr.biCompression != BI_BITFIELDS.0 {
        return None;
    }
    off += hdr.biClrUsed as usize * 4;
    let stride = ((w as usize * bpp + 31) / 32) * 4;
    if b.len() < off + stride * h as usize {
        return None;
    }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    let mut any_alpha = false;
    for y in 0..h as usize {
        let sy = if bottom_up { h as usize - 1 - y } else { y };
        let row = &b[off + sy * stride..];
        for x in 0..w as usize {
            let s = &row[x * bpp / 8..];
            let d = &mut out[(y * w as usize + x) * 4..][..4];
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = if bpp == 32 { s[3] } else { 255 };
            any_alpha |= bpp == 32 && s[3] != 0;
        }
    }
    if bpp == 32 && !any_alpha {
        for p in out.chunks_mut(4) {
            p[3] = 255;
        }
    }
    Some((out, w as u32, h as u32))
}

fn foreground_app() -> Option<String> {
    unsafe {
        let h = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        crate::util::process_path(pid).map(|p| crate::audio::friendly_name(&p))
    }
}

impl Clip {
    pub fn new() -> Arc<Self> {
        let dir = crate::settings::Settings::dir().join("clipboard");
        let items: Vec<Item> = std::fs::read_to_string(dir.join("history.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Arc::new(Self {
            items: Mutex::new(items),
            dir,
            max: Mutex::new(400),
            enabled: Mutex::new(true),
            images: Mutex::new(true),
            notify: Mutex::new(None),
        })
    }

    pub fn set_notify(&self, f: Box<dyn Fn() + Send + Sync>) {
        *self.notify.lock() = Some(f);
    }

    fn changed(&self) {
        self.save();
        if let Some(f) = self.notify.lock().as_ref() {
            f();
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let items = self.items.lock().clone();
        let tmp = self.dir.join("history.json.tmp");
        if std::fs::write(&tmp, serde_json::to_string(&items).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, self.dir.join("history.json"));
        }
    }

    pub fn image_path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Starts the listener thread.
    pub fn start(self: &Arc<Self>) {
        let (tx, rx) = channel::<()>();
        if TX.set(tx).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("clip-window".into())
            .spawn(|| unsafe {
                let hinst: HINSTANCE = GetModuleHandleW(None).map(|m| m.into()).unwrap_or_default();
                let class = wide("DeckClipboardListener");
                let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst, lpszClassName: PCWSTR(class.as_ptr()), ..Default::default() };
                RegisterClassW(&wc);
                let Ok(h) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    PCWSTR(class.as_ptr()),
                    PCWSTR::null(),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(hinst),
                    None,
                ) else {
                    return;
                };
                HWND_CLIP.store(h.0 as isize, Ordering::Relaxed);
                let _ = AddClipboardFormatListener(h);
                let mut m = MSG::default();
                while GetMessageW(&mut m, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
            })
            .ok();
        let me = self.clone();
        std::thread::Builder::new()
            .name("clip-reader".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    // Let the copying app finish writing every format.
                    std::thread::sleep(std::time::Duration::from_millis(60));
                    while rx.try_recv().is_ok() {}
                    if *me.enabled.lock() {
                        me.capture();
                    }
                }
            })
            .ok();
    }

    fn capture(&self) {
        let want_images = *self.images.lock();
        let Some((kind, text, files, img)) = read_now(want_images) else { return };
        if kind == "text" && text.trim().is_empty() {
            return;
        }
        let hash = match kind.as_str() {
            "text" => hash_of(&text),
            "files" => hash_of(&files),
            _ => img.as_ref().map(|(px, w, h)| hash_of(&(px.len(), w, h, &px[..px.len().min(65536)], &px[px.len().saturating_sub(65536)..]))).unwrap_or(0),
        };
        let own = OWN_WRITE.lock().take();
        let mut items = self.items.lock();
        if let Some(pos) = items.iter().position(|i| i.hash == hash && i.kind == kind) {
            let mut it = items.remove(pos);
            it.ts = now_ms();
            items.insert(0, it);
            drop(items);
            self.changed();
            return;
        }
        if own == Some(hash) {
            return;
        }
        let id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let mut item = Item { id, kind, text, files, hash, ts: now_ms(), app: foreground_app(), ..Default::default() };
        if let Some((px, w, h)) = img {
            let name = format!("{id}.png");
            let _ = std::fs::create_dir_all(&self.dir);
            match crate::apps::encode_png(&px, w, h) {
                Some(png) if std::fs::write(self.dir.join(&name), &png).is_ok() => {
                    item.image = Some(name);
                    item.w = w;
                    item.h = h;
                }
                _ => return,
            }
        }
        items.insert(0, item);
        let max = *self.max.lock();
        let removed = trim(&mut items, max, MAX_IMAGES);
        drop(items);
        for img in removed {
            let _ = std::fs::remove_file(self.dir.join(img));
        }
        self.changed();
    }

    pub fn list(&self, q: &str, limit: usize) -> Vec<Item> {
        let q = q.trim().to_lowercase();
        let items = self.items.lock();
        let mut out: Vec<Item> = items
            .iter()
            .filter(|i| {
                q.is_empty()
                    || i.text.to_lowercase().contains(&q)
                    || i.files.iter().any(|f| f.to_lowercase().contains(&q))
                    || i.app.as_deref().unwrap_or("").to_lowercase().contains(&q)
                    || (q == "image" && i.kind == "image")
            })
            .take(limit)
            .map(|i| {
                let mut c = i.clone();
                if c.text.len() > 4000 {
                    let mut cut = 4000;
                    while !c.text.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    c.text.truncate(cut);
                }
                c
            })
            .collect();
        // pinned first, keeping recency order otherwise
        out.sort_by_key(|i| !i.pinned);
        out
    }

    pub fn get(&self, id: u64) -> Option<Item> {
        self.items.lock().iter().find(|i| i.id == id).cloned()
    }

    pub fn pin(&self, id: u64, on: bool) {
        if let Some(i) = self.items.lock().iter_mut().find(|i| i.id == id) {
            i.pinned = on;
        }
        self.changed();
    }

    pub fn remove(&self, id: u64) {
        let mut items = self.items.lock();
        if let Some(pos) = items.iter().position(|i| i.id == id) {
            let it = items.remove(pos);
            if let Some(img) = it.image {
                let _ = std::fs::remove_file(self.dir.join(img));
            }
        }
        drop(items);
        self.changed();
    }

    /// Removes everything except pinned items.
    pub fn clear(&self) {
        let mut items = self.items.lock();
        let gone: Vec<Option<String>> = items.iter().filter(|i| !i.pinned).map(|i| i.image.clone()).collect();
        items.retain(|i| i.pinned);
        drop(items);
        for img in gone.into_iter().flatten() {
            let _ = std::fs::remove_file(self.dir.join(img));
        }
        self.changed();
    }

    /// Puts a history item back on the clipboard.
    pub fn copy_item(&self, id: u64) -> Result<(), String> {
        let it = self.get(id).ok_or("That item is gone")?;
        *OWN_WRITE.lock() = Some(it.hash);
        match it.kind.as_str() {
            "text" => set_text(&it.text),
            "files" => set_files(&it.files),
            "image" => {
                let name = it.image.ok_or("image missing")?;
                let bytes = std::fs::read(self.dir.join(name)).map_err(|e| e.to_string())?;
                set_png(&bytes)
            }
            _ => Err("unknown item".into()),
        }
    }
}

unsafe fn put(fmt: u32, bytes: &[u8]) -> Result<(), String> {
    let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
    let p = GlobalLock(g);
    if p.is_null() {
        return Err("GlobalLock failed".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
    let _ = GlobalUnlock(g);
    SetClipboardData(fmt, Some(HANDLE(g.0))).map(|_| ()).map_err(|e| e.to_string())
}

fn with_clipboard(f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if !open() {
        return Err("Another app is using the clipboard".into());
    }
    let r = unsafe { EmptyClipboard() }.map_err(|e| e.to_string()).and_then(|_| f());
    unsafe {
        let _ = CloseClipboard();
    }
    r
}

pub fn set_text(text: &str) -> Result<(), String> {
    let w = wide(text);
    let bytes: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    with_clipboard(|| unsafe { put(CF_UNICODETEXT.0 as u32, &bytes) })
}

pub fn set_files(files: &[String]) -> Result<(), String> {
    // DROPFILES header (20 bytes) + double-null-terminated wide list
    let mut b = Vec::new();
    b.extend_from_slice(&20u32.to_le_bytes());
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    for f in files {
        for c in f.encode_utf16().chain(std::iter::once(0)) {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    b.extend_from_slice(&[0, 0]);
    with_clipboard(|| unsafe { put(CF_HDROP.0 as u32, &b) })
}

/// Puts a PNG on the clipboard both as "PNG" (keeps transparency, used by
/// browsers and Discord) and as a classic DIB (everything else).
pub fn set_png(png_bytes: &[u8]) -> Result<(), String> {
    let dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; r.output_buffer_size().unwrap_or(0)];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return Err("unsupported image".into()),
    };
    let mut dib = Vec::with_capacity(40 + w * h * 4);
    let hdr = BITMAPINFOHEADER {
        biSize: 40,
        biWidth: w as i32,
        biHeight: h as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (w * h * 4) as u32,
        ..Default::default()
    };
    dib.extend_from_slice(unsafe { std::slice::from_raw_parts(&hdr as *const _ as *const u8, 40) });
    for y in (0..h).rev() {
        for x in 0..w {
            let s = &buf[(y * w + x) * ch..];
            dib.extend_from_slice(&[s[2], s[1], s[0], if ch == 4 { s[3] } else { 255 }]);
        }
    }
    let png_fmt = reg_format("PNG");
    with_clipboard(|| unsafe {
        put(CF_DIB.0 as u32, &dib)?;
        put(png_fmt, png_bytes)
    })
}

/// Presses Ctrl+V in whatever window has focus.
pub fn send_paste() {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() } },
    };
    let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(id: u64, kind: &str, pinned: bool) -> Item {
        Item { id, kind: kind.into(), pinned, image: (kind == "image").then(|| format!("{id}.png")), ..Default::default() }
    }

    #[test]
    fn history_keeps_its_limits_and_pins() {
        // newest first: 6 images, 4 texts, one pinned image at the end
        let mut items: Vec<Item> = (0..6).map(|i| it(i, "image", false)).chain((6..10).map(|i| it(i, "text", false))).collect();
        items.push(it(99, "image", true));
        let gone = trim(&mut items, 8, 3);
        // the 3 newest images stay, older ones go (files deleted); the pinned one always stays
        assert_eq!(gone, vec!["3.png", "4.png", "5.png"]);
        let ids: Vec<u64> = items.iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 6, 7, 8, 9, 99]);
        // the overall limit still applies on top
        let gone = trim(&mut items, 4, 3);
        assert!(gone.is_empty(), "only texts were over the limit");
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![0, 1, 2, 6, 99]);
    }
}
