use half::f16;

/// Packed RGBA storage. 8-bit sources stay 8-bit: a quarter of the memory of
/// half float and no precision to gain from widening.
pub enum Pixels {
    Half(Vec<f16>),
    Byte(Vec<u8>),
}

/// A decoded frame.
///
/// Values are whatever the file encodes (scene-linear for EXR, 0..1 code values for
/// integer formats). Color management happens later, at display time.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f32,
    pub has_alpha: bool,
    pub pixels: Pixels,
}

impl Frame {
    pub fn new(width: u32, height: u32, has_alpha: bool) -> Self {
        let n = width as usize * height as usize * 4;
        Self { width, height, pixel_aspect: 1.0, has_alpha, pixels: Pixels::Half(vec![f16::ZERO; n]) }
    }

    pub fn new_u8(width: u32, height: u32, has_alpha: bool) -> Self {
        let n = width as usize * height as usize * 4;
        Self { width, height, pixel_aspect: 1.0, has_alpha, pixels: Pixels::Byte(vec![0; n]) }
    }

    pub fn from_f32(width: u32, height: u32, has_alpha: bool, rgba: &[f32]) -> Self {
        use rayon::prelude::*;
        let mut f = Self::new(width, height, has_alpha);
        f.half_mut().par_chunks_mut(64 * 1024).zip(rgba.par_chunks(64 * 1024)).for_each(|(dst, src)| {
            for (d, s) in dst.iter_mut().zip(src) {
                *d = f16::from_f32(*s);
            }
        });
        f
    }

    /// The half-float buffer. Panics on an 8-bit frame; only call on frames
    /// built with `new`.
    pub fn half_mut(&mut self) -> &mut [f16] {
        match &mut self.pixels {
            Pixels::Half(p) => p,
            Pixels::Byte(_) => panic!("half_mut on an 8-bit frame"),
        }
    }

    pub fn byte_mut(&mut self) -> &mut [u8] {
        match &mut self.pixels {
            Pixels::Byte(p) => p,
            Pixels::Half(_) => panic!("byte_mut on a half-float frame"),
        }
    }

    pub fn byte_size(&self) -> usize {
        match &self.pixels {
            Pixels::Half(p) => p.len() * 2,
            Pixels::Byte(p) => p.len(),
        }
    }

    /// Raw RGBA at a pixel (8-bit values normalized to 0..1), or None outside the image.
    pub fn sample(&self, x: i64, y: i64) -> Option<[f32; 4]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some(match &self.pixels {
            Pixels::Half(p) => [p[i].to_f32(), p[i + 1].to_f32(), p[i + 2].to_f32(), p[i + 3].to_f32()],
            Pixels::Byte(p) => [p[i] as f32 / 255.0, p[i + 1] as f32 / 255.0, p[i + 2] as f32 / 255.0, p[i + 3] as f32 / 255.0],
        })
    }
}
