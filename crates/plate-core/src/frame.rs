use half::f16;

/// A decoded frame: packed RGBA in half float.
///
/// Values are whatever the file encodes (scene-linear for EXR, 0..1 code values for
/// integer formats). Color management happens later, at display time.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f32,
    pub has_alpha: bool,
    pub pixels: Vec<f16>,
}

impl Frame {
    pub fn new(width: u32, height: u32, has_alpha: bool) -> Self {
        Self {
            width,
            height,
            pixel_aspect: 1.0,
            has_alpha,
            pixels: vec![f16::ZERO; width as usize * height as usize * 4],
        }
    }

    pub fn from_f32(width: u32, height: u32, has_alpha: bool, rgba: &[f32]) -> Self {
        use rayon::prelude::*;
        let mut pixels = vec![f16::ZERO; width as usize * height as usize * 4];
        pixels
            .par_chunks_mut(64 * 1024)
            .zip(rgba.par_chunks(64 * 1024))
            .for_each(|(dst, src)| {
                for (d, s) in dst.iter_mut().zip(src) {
                    *d = f16::from_f32(*s);
                }
            });
        Self { width, height, pixel_aspect: 1.0, has_alpha, pixels }
    }

    pub fn byte_size(&self) -> usize {
        self.pixels.len() * 2
    }

    /// Raw RGBA at a pixel, or None outside the image.
    pub fn sample(&self, x: i64, y: i64) -> Option<[f32; 4]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = &self.pixels[i..i + 4];
        Some([p[0].to_f32(), p[1].to_f32(), p[2].to_f32(), p[3].to_f32()])
    }
}
