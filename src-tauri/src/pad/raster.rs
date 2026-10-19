//! SVG → pixels for the overlay art (resvg), soft glows and shadows, and the few compositing
//! operations a frame needs. Pixels are premultiplied RGBA (tiny-skia's own format).

use resvg::tiny_skia::{IntRect, Pixmap, PixmapPaint, Transform};
use resvg::usvg;
use std::sync::{Arc, OnceLock};

/// An image with premultiplied alpha.
#[derive(Clone)]
pub struct Img(pub Pixmap);

impl Img {
    pub fn new(w: u32, h: u32) -> Img {
        Img(Pixmap::new(w.max(1), h.max(1)).expect("pixmap size"))
    }
    pub fn w(&self) -> i32 {
        self.0.width() as i32
    }
    pub fn h(&self) -> i32 {
        self.0.height() as i32
    }
    /// Draws `src` over this image with its top-left corner at (x, y).
    pub fn blit(&mut self, src: &Img, x: i32, y: i32) {
        self.0.draw_pixmap(x, y, src.0.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    /// Draws only the columns [cx0, cx1) of `src` (in `src`'s own pixels).
    pub fn blit_cols(&mut self, src: &Img, x: i32, y: i32, cx0: i32, cx1: i32) {
        let (cx0, cx1) = (cx0.max(0), cx1.min(src.w()));
        if cx1 <= cx0 {
            return;
        }
        if let Some(part) = IntRect::from_xywh(cx0, 0, (cx1 - cx0) as u32, src.h() as u32).and_then(|r| src.0.clone_rect(r)) {
            self.0.draw_pixmap(x + cx0, y, part.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    /// Premultiplied BGRA, the layout UpdateLayeredWindow wants.
    pub fn bgra(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(self.0.data());
        for px in out.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
    }
    /// Straight-alpha RGBA (for tray icons).
    pub fn rgba_straight(&self) -> Vec<u8> {
        let mut v = self.0.data().to_vec();
        for px in v.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        v
    }
    /// The bounding box of pixels with any alpha: (x0, y0, x1, y1), exclusive.
    pub fn alpha_box(&self) -> Option<(i32, i32, i32, i32)> {
        let (w, h) = (self.w(), self.h());
        let d = self.0.data();
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, -1, -1);
        for y in 0..h {
            for x in 0..w {
                if d[((y * w + x) * 4 + 3) as usize] > 8 {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
        }
        (x1 >= 0).then_some((x0, y0, x1 + 1, y1 + 1))
    }
}

/// Segoe UI from Windows' fonts folder (regular, semibold, bold), loaded once; any sans
/// font Windows has when it's missing.
fn options() -> &'static usvg::Options<'static> {
    static OPT: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPT.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        let dir = crate::util::windows_dir().join("Fonts");
        for f in ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf"] {
            if let Ok(b) = std::fs::read(dir.join(f)) {
                db.load_font_data(b);
            }
        }
        if db.is_empty() {
            for f in ["arial.ttf", "arialbd.ttf", "tahoma.ttf"] {
                if let Ok(b) = std::fs::read(dir.join(f)) {
                    db.load_font_data(b);
                }
            }
        }
        let family = db.faces().next().and_then(|f| f.families.first().map(|(n, _)| n.clone())).unwrap_or_else(|| "Segoe UI".into());
        usvg::Options { font_family: family, fontdb: Arc::new(db), ..Default::default() }
    })
}

/// Renders a whole SVG document at its own size.
pub fn doc(svg: &str) -> Option<Img> {
    let tree = usvg::Tree::from_str(svg, options()).ok()?;
    let size = tree.size().to_int_size();
    let mut pm = Pixmap::new(size.width().max(1), size.height().max(1))?;
    resvg::render(&tree, Transform::identity(), &mut pm.as_mut());
    Some(Img(pm))
}

/// Rasterises SVG `body` (design units) covering `bx` = (x0, y0, x1, y1) at scale `k`. The box
/// snaps to whole pixels, so separately drawn sprites line up exactly. Returns the image and
/// where its top-left corner goes, in pixels.
pub fn svg(body: &str, defs: &str, k: f32, bx: (f32, f32, f32, f32)) -> (Img, (i32, i32)) {
    let px0 = (bx.0 * k).floor() as i32;
    let py0 = (bx.1 * k).floor() as i32;
    let w = ((bx.2 * k).ceil() as i32 - px0).max(1);
    let h = ((bx.3 * k).ceil() as i32 - py0).max(1);
    let s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{} {} {} {}"><defs>{defs}</defs>{body}</svg>"#,
        px0 as f32 / k,
        py0 as f32 / k,
        w as f32 / k,
        h as f32 / k
    );
    (doc(&s).unwrap_or_else(|| Img::new(w as u32, h as u32)), (px0, py0))
}

/// "#rrggbb" → (r, g, b).
pub fn rgb(c: &str) -> (u8, u8, u8) {
    let c = c.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(c.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    (p(0), p(2), p(4))
}

/// One pass of a box blur of radius `r` over a single channel, along rows (`w` × `h`).
fn box_rows(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let at = |x: isize| -> f32 { if x < 0 || x >= w as isize { 0.0 } else { row[x as usize] } };
        let mut acc: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for x in 0..w {
            dst[y * w + x] = acc * norm;
            acc += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
    }
}

fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            out[x * h + y] = src[y * w + x];
        }
    }
    out
}

/// A soft coloured blur of an image's shape (glows and drop shadows): its alpha, blurred by
/// about `radius` pixels (three box passes ≈ a gaussian), tinted `color`, times `strength`.
/// Returns the halo and how far it reaches past the image on every side.
pub fn halo(src: &Img, color: &str, radius: f32, strength: f32) -> (Img, i32) {
    let pad = (radius * 2.0).ceil() as i32;
    let (w, h) = ((src.w() + 2 * pad) as usize, (src.h() + 2 * pad) as usize);
    let mut a = vec![0.0f32; w * h];
    let d = src.0.data();
    for y in 0..src.h() as usize {
        for x in 0..src.w() as usize {
            a[(y + pad as usize) * w + x + pad as usize] = d[(y * src.w() as usize + x) * 4 + 3] as f32;
        }
    }
    let r = ((radius / 2.0).round() as usize).max(1);
    let mut tmp = vec![0.0f32; w * h];
    for _ in 0..3 {
        box_rows(&a, &mut tmp, w, h, r);
        std::mem::swap(&mut a, &mut tmp);
    }
    let mut t = transpose(&a, w, h);
    let mut tmp = vec![0.0f32; w * h];
    for _ in 0..3 {
        box_rows(&t, &mut tmp, h, w, r);
        std::mem::swap(&mut t, &mut tmp);
    }
    let a = transpose(&t, h, w);
    let (cr, cg, cb) = rgb(color);
    let mut out = Img::new(w as u32, h as u32);
    let px = out.0.data_mut();
    for (i, v) in a.iter().enumerate() {
        let al = (v * strength).clamp(0.0, 255.0);
        let m = al / 255.0;
        px[i * 4] = (cr as f32 * m).round() as u8;
        px[i * 4 + 1] = (cg as f32 * m).round() as u8;
        px[i * 4 + 2] = (cb as f32 * m).round() as u8;
        px[i * 4 + 3] = al.round() as u8;
    }
    (out, pad)
}

/// Scales an image to `w` × `h` (smooth).
pub fn scaled(src: &Img, w: u32, h: u32) -> Img {
    let mut out = Img::new(w, h);
    let t = Transform::from_scale(w as f32 / src.w() as f32, h as f32 / src.h() as f32);
    let paint = PixmapPaint { quality: resvg::tiny_skia::FilterQuality::Bicubic, ..Default::default() };
    out.0.draw_pixmap(0, 0, src.0.as_ref(), &paint, t, None);
    out
}

/// A crop of an image.
pub fn crop(src: &Img, x0: i32, y0: i32, x1: i32, y1: i32) -> Img {
    IntRect::from_ltrb(x0, y0, x1, y1).and_then(|r| src.0.clone_rect(r)).map(Img).unwrap_or_else(|| Img::new(1, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_snaps_to_pixels() {
        let (img, pos) = svg(r##"<rect x="10" y="10" width="20" height="10" fill="#fff"/>"##, "", 1.5, (9.0, 9.0, 31.0, 21.0));
        assert_eq!(pos, (13, 13));
        assert_eq!((img.w(), img.h()), (34, 19));
        assert!(img.alpha_box().is_some());
    }

    #[test]
    fn halo_spreads_past_the_shape() {
        let (img, _) = svg(r##"<rect x="0" y="0" width="10" height="10" fill="#fff"/>"##, "", 1.0, (0.0, 0.0, 10.0, 10.0));
        let (h, pad) = halo(&img, "#ff0000", 4.0, 1.0);
        assert_eq!(pad, 8);
        assert_eq!(h.w(), 26);
        let b = h.alpha_box().unwrap();
        assert!(b.0 < pad && b.2 > pad + 10);
    }

    #[test]
    fn text_renders() {
        let (img, _) = svg(r##"<text x="20" y="10" font-size="12" font-weight="700" fill="#fff" text-anchor="middle" dominant-baseline="central">64%</text>"##, "", 1.0, (0.0, 0.0, 40.0, 20.0));
        assert!(img.alpha_box().is_some(), "a font was found");
    }

    #[test]
    fn straight_alpha_unpremultiplies() {
        let mut i = Img::new(1, 1);
        i.0.data_mut().copy_from_slice(&[64, 0, 0, 128]);
        assert_eq!(i.rgba_straight(), vec![128, 0, 0, 128]);
    }
}
