//! The battery tray icon, phone style: a battery filled to the level with the percentage inside
//! (a bolt while charging without a level), and a tiny controller on its lower edge so it reads
//! as the pad's battery, not the PC's. Green while charging, red when low; dark fills and rims
//! keep it readable on light and dark taskbars alike.

use super::art::{self, body_path, bolt_path, n, path, rect};
use super::battery::Reading;
use super::raster::{self, Img};

/// Straight-alpha RGBA, `size` × `size`.
pub fn battery(size: u32, r: &Reading) -> Vec<u8> {
    let s = size as f32;
    let dark = "#0b1016";
    let color = if r.charging { art::OK_GREEN } else if r.low() { art::LOW_RED } else { "#ffffff" };
    let sw = (s / 16.0).max(1.0);
    let nub = s * 0.09;
    let (bx, by, bw, bh) = (sw / 2.0, s * 0.04, s - sw / 2.0 - nub - 0.5, s * 0.66);
    let inset = sw * 1.6;
    let mut body = rect(bx, by, bw, bh, s * 0.13, &format!(r#"fill="{dark}" fill-opacity="0.8" stroke="{color}" stroke-width="{}""#, n(sw)));
    body += &rect(bx + bw + sw / 2.0, by + bh * 0.3, nub, bh * 0.4, nub * 0.35, &format!(r#"fill="{color}""#));
    body += &rect(bx + inset, by + inset, ((bw - 2.0 * inset) * r.fill()).max(0.0), bh - 2.0 * inset, s * 0.06, &format!(r#"fill="{color}" fill-opacity="0.35""#));
    if r.percent.is_none() && r.charging {
        let bolt = bolt_path(bx + bw / 2.0, by + bh / 2.0, bh * 0.8, 1.4);
        body += &path(&bolt, &format!(r#"fill="{color}" stroke="{dark}" stroke-width="{}" stroke-linejoin="round""#, n(sw * 0.6)));
    }
    // the DS4 outline (design units x 8..401, y 26..258), half the icon wide, bottom right
    let c = s * 0.52 / 393.0;
    body += &format!(r#"<g transform="translate({},{}) scale({})">{}</g>"#, n(s - 401.0 * c), n(s - 258.0 * c), c, path(&body_path(), &format!(r##"fill="#cfd4e0" stroke="{dark}" stroke-width="{}""##, n(sw * 1.2 / c))));
    let (mut img, _) = raster::svg(&body, "", 1.0, (0.0, 0.0, s, s));
    if let Some(p) = r.percent {
        let text = p.to_string();
        let fg = if color == art::OK_GREEN { "#ffffff" } else { color };
        // big, then cropped to the ink and shrunk to fit
        let big = art::text_line(&text, bh * 3.0, "bold", fg);
        let rim = art::text_line(&text, bh * 3.0, "bold", dark);
        if let Some((x0, y0, x1, y1)) = big.alpha_box() {
            let (big, rim) = (raster::crop(&big, x0, y0, x1, y1), raster::crop(&rim, x0, y0, x1, y1));
            let scale = ((bw - 2.0 * inset) / big.w() as f32).min((bh - 2.0 * inset) / big.h() as f32);
            let (w, h) = ((big.w() as f32 * scale).max(1.0) as u32, (big.h() as f32 * scale).max(1.0) as u32);
            let (big, rim) = (raster::scaled(&big, w, h), raster::scaled(&rim, w, h));
            let x = (bx + (bw - w as f32) / 2.0).round() as i32;
            let y = (by + (bh - h as f32) / 2.0).round() as i32;
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                img.blit(&rim, x + dx, y + dy);
            }
            img.blit(&big, x, y);
        }
    }
    let img = if img.w() as u32 == size && img.h() as u32 == size { img } else { fit(&img, size) };
    img.rgba_straight()
}

fn fit(img: &Img, size: u32) -> Img {
    let mut out = Img::new(size, size);
    out.blit(img, 0, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_icon_is_size_squared() {
        for size in [16, 20, 24, 32] {
            for r in [
                Reading { percent: Some(64), level: None, charging: false },
                Reading { percent: Some(9), level: None, charging: false },
                Reading { percent: None, level: None, charging: true },
                Reading { percent: None, level: Some("medium"), charging: false },
            ] {
                let px = battery(size, &r);
                assert_eq!(px.len(), (size * size * 4) as usize);
                assert!(px.chunks(4).any(|p| p[3] > 200), "something is drawn");
            }
        }
    }

    #[test]
    fn levels_look_different() {
        let a = battery(32, &Reading { percent: Some(80), level: None, charging: false });
        let b = battery(32, &Reading { percent: Some(10), level: None, charging: false });
        assert_ne!(a, b);
    }
}
