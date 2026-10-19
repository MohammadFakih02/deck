//! Windows' built-in OCR (Windows.Media.Ocr, as the picker's "copy text" uses): English when
//! that language is installed (the death words are English), else the user's languages.

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

pub struct Ocr {
    engine: OcrEngine,
    max: usize,
    /// English is installed (the death words are English); otherwise the user's languages are used
    pub english: bool,
}

impl Ocr {
    pub fn new() -> Option<Ocr> {
        let en = Language::CreateLanguage(&"en".into()).ok().and_then(|l| OcrEngine::TryCreateFromLanguage(&l).ok());
        let english = en.is_some();
        let engine = en.or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())?;
        let max = OcrEngine::MaxImageDimension().map(|m| m as usize).unwrap_or(4096);
        Some(Ocr { engine, max, english })
    }

    /// The text in a BGRA picture, one line per line found ("" when none).
    pub fn read(&self, w: usize, h: usize, bgra: &[u8]) -> windows::core::Result<String> {
        if w == 0 || h == 0 || w > self.max || h > self.max || bgra.len() < w * h * 4 {
            return Ok(String::new());
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra[..w * h * 4])?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(&writer.DetachBuffer()?, BitmapPixelFormat::Bgra8, w as i32, h as i32)?;
        let result = self.engine.RecognizeAsync(&bmp)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "YOU DIED" in dark red on near black, drawn with GDI into memory (never the real screen).
    fn banner(text: &str, w: i32, h: i32) -> Vec<u8> {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Gdi::*;
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, bmp.into());
            let px = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            for p in px.chunks_mut(4) {
                p.copy_from_slice(&[8, 8, 12, 255]);
            }
            let face = crate::util::wide("Times New Roman");
            let font = CreateFontW(-(h / 3), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, PCWSTR(face.as_ptr()));
            let of = SelectObject(dc, font.into());
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x0000_10A0)); // BGR: dark red
            let t: Vec<u16> = text.encode_utf16().collect();
            let mut r = windows::Win32::Foundation::RECT { left: 0, top: 0, right: w, bottom: h };
            DrawTextW(dc, &mut t.clone(), &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            let _ = GdiFlush();
            let out = px.to_vec();
            SelectObject(dc, of);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            out
        }
    }

    /// Needs Windows' OCR with an installed language; run with `cargo test -- --ignored ocr_reads`.
    #[test]
    #[ignore]
    fn ocr_reads_a_drawn_death_banner() {
        crate::util::com();
        let (w, h) = (560, 240);
        let ocr = Ocr::new().expect("Windows OCR");
        let text = ocr.read(w, h, &banner("YOU DIED", w as i32, h as i32)).unwrap();
        let words: Vec<String> = super::super::detect::COMMON_WORDS.iter().map(|s| s.to_string()).collect();
        assert!(super::super::detect::is_death_text(&text, &words), "read {text:?}");
        let calm = ocr.read(w, h, &banner("Site of Grace", w as i32, h as i32)).unwrap();
        assert!(!super::super::detect::is_death_text(&calm, &words), "read {calm:?}");
    }
}
