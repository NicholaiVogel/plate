//! Saving stills: the displayed frame with annotations burned in.

use crate::display::DisplayImage;
use crate::notes::{arrow_head, Shape};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use anyhow::{bail, Result};
use std::path::Path;
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

fn paint(color: [u8; 3]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(color[0], color[1], color[2], 255);
    p.anti_alias = true;
    p
}

/// Burns shapes into an RGBA pixmap of the display image.
pub fn rasterize(image: &DisplayImage, shapes: &[Shape], font: Option<&[u8]>) -> Result<Pixmap> {
    let (w, h) = (image.width, image.height);
    let Some(mut pix) = Pixmap::new(w, h) else { bail!("empty image") };
    for (d, s) in pix.data_mut().chunks_exact_mut(4).zip(image.bgra.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], 255]);
    }
    let (fw, fh) = (w as f32, h as f32);
    let px = |p: [f32; 2]| (p[0] * fw, p[1] * fh);

    for shape in shapes {
        let mut pb = PathBuilder::new();
        let width = match shape {
            Shape::Pen { points, width, .. } => {
                if let Some(first) = points.first() {
                    let (x, y) = px(*first);
                    pb.move_to(x, y);
                    if points.len() == 1 {
                        pb.line_to(x + 0.01, y);
                    }
                    for p in &points[1..] {
                        let (x, y) = px(*p);
                        pb.line_to(x, y);
                    }
                }
                *width
            }
            Shape::Line { from, to, width, .. } => {
                let ((ax, ay), (bx, by)) = (px(*from), px(*to));
                pb.move_to(ax, ay);
                pb.line_to(bx, by);
                *width
            }
            Shape::Arrow { from, to, width, .. } => {
                let (a, b) = (px(*from), px(*to));
                pb.move_to(a.0, a.1);
                pb.line_to(b.0, b.1);
                let (l, r) = arrow_head([a.0, a.1], [b.0, b.1], (width * fh * 4.0).max(10.0));
                pb.move_to(l[0], l[1]);
                pb.line_to(b.0, b.1);
                pb.line_to(r[0], r[1]);
                *width
            }
            Shape::Rect { from, to, width, .. } => {
                let (a, b) = (px(*from), px(*to));
                if let Some(r) = tiny_skia::Rect::from_ltrb(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)) {
                    pb.push_rect(r);
                }
                *width
            }
            Shape::Ellipse { from, to, width, .. } => {
                let (a, b) = (px(*from), px(*to));
                if let Some(r) = tiny_skia::Rect::from_ltrb(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)) {
                    pb.push_oval(r);
                }
                *width
            }
            Shape::Text { at, text, color, size } => {
                if let Some(font) = font {
                    draw_text(&mut pix, font, text, px(*at), size * fh, *color);
                }
                continue;
            }
        };
        if let Some(path) = pb.finish() {
            let stroke = Stroke {
                width: (width * fh).max(1.0),
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pix.stroke_path(&path, &paint(shape.color()), &stroke, Transform::identity(), None);
        }
    }
    Ok(pix)
}

fn draw_text(pix: &mut Pixmap, font: &[u8], text: &str, origin: (f32, f32), size: f32, color: [u8; 3]) {
    let Ok(font) = FontRef::try_from_slice(font) else { return };
    let scaled = font.as_scaled(PxScale::from(size));
    let (w, h) = (pix.width() as i32, pix.height() as i32);
    let data = pix.data_mut();
    let mut y = origin.1 + scaled.ascent();
    for line in text.lines() {
        let mut x = origin.0;
        let mut prev = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(x, y));
            x += scaled.h_advance(id);
            prev = Some(id);
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, cov| {
                    let (tx, ty) = (bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32);
                    if tx < 0 || ty < 0 || tx >= w || ty >= h {
                        return;
                    }
                    let i = (ty as usize * w as usize + tx as usize) * 4;
                    for c in 0..3 {
                        let d = data[i + c] as f32;
                        data[i + c] = (d + (color[c] as f32 - d) * cov.min(1.0)) as u8;
                    }
                });
            }
        }
        y += scaled.height() + scaled.line_gap();
    }
}

/// Writes PNG, JPEG, TIFF or EXR (8-bit display values) depending on the extension.
pub fn save_still(path: &Path, image: &DisplayImage, shapes: &[Shape], font: Option<&[u8]>) -> Result<()> {
    let pix = rasterize(image, shapes, font)?;
    let rgba = image::RgbaImage::from_raw(pix.width(), pix.height(), pix.data().to_vec())
        .ok_or_else(|| anyhow::anyhow!("pixmap size mismatch"))?;
    let ext = crate::scan::ext_of(path);
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
            let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 95).encode_image(&rgb)?;
        }
        "png" | "tif" | "tiff" => image::DynamicImage::ImageRgba8(rgba).to_rgb8().save(path)?,
        other => bail!("can't save .{other}; use png, jpg or tif"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_burn_into_the_still() {
        let image = DisplayImage { width: 200, height: 100, bgra: vec![0; 200 * 100 * 4] };
        let shapes = vec![
            Shape::Rect { from: [0.1, 0.1], to: [0.5, 0.9], color: [255, 0, 0], width: 0.03 },
            Shape::Arrow { from: [0.6, 0.8], to: [0.9, 0.2], color: [0, 255, 0], width: 0.03 },
        ];
        let pix = rasterize(&image, &shapes, None).unwrap();
        let at = |x: u32, y: u32| {
            let i = ((y * 200 + x) * 4) as usize;
            [pix.data()[i], pix.data()[i + 1], pix.data()[i + 2]]
        };
        assert!(at(20, 50)[0] > 200, "rect left edge should be red");
        assert_eq!(at(60, 50), [0, 0, 0], "rect interior stays untouched");
        assert!(at(150, 50)[1] > 100, "arrow shaft should be green");
    }
}
