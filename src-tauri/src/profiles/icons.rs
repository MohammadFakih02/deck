//! Profile icons: a built-in set (embedded, written to disk for shortcuts) plus the user's own
//! .ico / .png / .exe. A PNG becomes a multi-size .ico.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

macro_rules! icons {
    ($($n:literal),* $(,)?) => { &[$(($n, include_bytes!(concat!("../../icons/profiles/", $n)) as &[u8])),*] };
}

pub const BUILTIN: &[(&str, &[u8])] = icons![
    "cpu-ultra.ico", "cpu-eco.ico", "cpu-mid.ico", "cpu-full.ico",
    "gpu-green.ico", "gpu-blue.ico", "gpu-amber.ico", "gpu-red.ico", "gpu-flame.ico", "gpu-snow.ico", "gpu-leaf.ico",
    "rocket.ico", "gauge.ico", "bolt.ico", "fan.ico", "gamepad.ico", "film.ico", "moon.ico", "sun.ico", "shield.ico",
    "turtle.ico", "rabbit.ico", "headphones.ico", "code.ico", "battery.ico",
];

/// Where the built-ins live on disk: next to Deck.exe when installed, else in %APPDATA%.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("icons"))) {
        if BUILTIN.iter().all(|(n, _)| d.join(n).exists()) {
            return d;
        }
    }
    crate::settings::Settings::dir().join("icons")
}

/// Writes any missing built-in icon to `dir()`.
pub fn ensure() -> PathBuf {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    for (n, b) in BUILTIN {
        let p = d.join(n);
        if std::fs::metadata(&p).map(|m| m.len() != b.len() as u64).unwrap_or(true) {
            let _ = std::fs::write(&p, b);
        }
    }
    d
}

/// The file (and icon index) a shortcut should use for `icon`: a built-in name, or a path.
pub fn location(icon: &str, fallback: &str) -> (String, i32) {
    let pick = if icon.is_empty() { fallback } else { icon };
    if BUILTIN.iter().any(|(n, _)| *n == pick) {
        return (ensure().join(pick).to_string_lossy().to_string(), 0);
    }
    if Path::new(pick).exists() {
        return (pick.to_string(), 0);
    }
    (ensure().join(fallback).to_string_lossy().to_string(), 0)
}

fn png_64(ico: &[u8]) -> Option<Vec<u8>> {
    let im = image::load_from_memory_with_format(ico, image::ImageFormat::Ico).ok()?;
    let im = im.resize_exact(64, 64, image::imageops::FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::new());
    im.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Built-ins as small PNG data URLs for the picker (made once).
pub fn previews() -> &'static Vec<(String, String)> {
    static P: OnceLock<Vec<(String, String)>> = OnceLock::new();
    P.get_or_init(|| BUILTIN.iter().filter_map(|(n, b)| png_64(b).map(|p| (n.to_string(), format!("data:image/png;base64,{}", crate::util::base64(&p))))).collect())
}

/// Preview of any icon value (built-in name or a file path).
pub fn preview(icon: &str) -> Option<String> {
    if let Some((_, u)) = previews().iter().find(|(n, _)| n == icon) {
        return Some(u.clone());
    }
    let b = std::fs::read(icon).ok()?;
    let fmt = if icon.to_lowercase().ends_with(".png") { image::ImageFormat::Png } else { image::ImageFormat::Ico };
    let im = image::load_from_memory_with_format(&b, fmt).ok()?.resize_exact(64, 64, image::imageops::FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::new());
    im.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", crate::util::base64(&out.into_inner())))
}

/// A PNG as a multi-size .ico (16–256 px, square-padded).
pub fn png_to_ico(png: &[u8]) -> Result<Vec<u8>, String> {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    use image::{imageops, ExtendedColorType, RgbaImage};
    let src = image::load_from_memory(png).map_err(|e| format!("not a picture: {e}"))?.to_rgba8();
    let side = src.width().max(src.height());
    let mut sq = RgbaImage::new(side, side);
    imageops::overlay(&mut sq, &src, ((side - src.width()) / 2) as i64, ((side - src.height()) / 2) as i64);
    let mut frames = vec![];
    for s in [16u32, 24, 32, 48, 64, 256] {
        let im = imageops::resize(&sq, s, s, imageops::FilterType::Lanczos3);
        frames.push(IcoFrame::as_png(im.as_raw(), s, s, ExtendedColorType::Rgba8).map_err(|e| e.to_string())?);
    }
    let mut out = vec![];
    IcoEncoder::new(&mut out).encode_images(&frames).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The user picked a file: .ico and .exe are used where they are; a .png is converted and
/// kept in %APPDATA%\Deck\icons. Returns the icon value to store.
pub fn custom(file: &str) -> Result<String, String> {
    let low = file.to_lowercase();
    if low.ends_with(".ico") || low.ends_with(".exe") {
        return Path::new(file).exists().then(|| file.to_string()).ok_or_else(|| "That file doesn't exist.".into());
    }
    if low.ends_with(".png") {
        let b = std::fs::read(file).map_err(|e| e.to_string())?;
        let ico = png_to_ico(&b)?;
        let d = crate::settings::Settings::dir().join("icons");
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "icon".into());
        let p = d.join(format!("custom-{}-{}.ico", stem, crate::util::now_ms() % 100_000));
        std::fs::write(&p, ico).map_err(|e| e.to_string())?;
        return Ok(p.to_string_lossy().to_string());
    }
    Err("Pick an .ico, .png or .exe file.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_decode() {
        assert!(BUILTIN.len() >= 24);
        assert_eq!(previews().len(), BUILTIN.len(), "every built-in icon decodes");
    }

    #[test]
    fn png_becomes_ico() {
        let im = image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(im).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let ico = png_to_ico(&png.into_inner()).unwrap();
        let back = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).unwrap();
        assert_eq!(back.width(), 256, "the largest frame");
        assert_eq!(u16::from_le_bytes([ico[4], ico[5]]), 6, "six sizes");
    }
}
