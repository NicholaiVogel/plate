//! Frame -> display pixels: OCIO view transform, viewer exposure/gamma and
//! channel isolation, producing premultiplied BGRA8 for the GPU.

use crate::frame::{Frame, Pixels};
use crate::lut::DisplayLut;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Channel {
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
    Alpha,
    Luma,
}

impl Channel {
    pub const ALL: [Channel; 6] = [Channel::Rgb, Channel::Red, Channel::Green, Channel::Blue, Channel::Alpha, Channel::Luma];

    pub fn label(self) -> &'static str {
        match self {
            Channel::Rgb => "RGB",
            Channel::Red => "Red",
            Channel::Green => "Green",
            Channel::Blue => "Blue",
            Channel::Alpha => "Alpha",
            Channel::Luma => "Luma",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Channel::Rgb => "RGB",
            Channel::Red => "R",
            Channel::Green => "G",
            Channel::Blue => "B",
            Channel::Alpha => "A",
            Channel::Luma => "Y",
        }
    }
}

pub struct DisplayImage {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl DisplayImage {
    /// Display-referred RGB (0..1) at a pixel.
    pub fn rgb_at(&self, x: i64, y: i64) -> Option<[f32; 3]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = &self.bgra[i..i + 4];
        Some([p[2] as f32 / 255.0, p[1] as f32 / 255.0, p[0] as f32 / 255.0])
    }
}

const BAND_ROWS: usize = 16;

#[inline]
fn to_u8(v: f32) -> u8 {
    // NaN maps to 0 through the clamp.
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
fn write(o: &mut [u8], d: [f32; 3], alpha: f32, channel: Channel) {
    let (r, g, b) = match channel {
        Channel::Alpha => (alpha, alpha, alpha),
        Channel::Red => (d[0], d[0], d[0]),
        Channel::Green => (d[1], d[1], d[1]),
        Channel::Blue => (d[2], d[2], d[2]),
        Channel::Luma => {
            let y = 0.2126 * d[0] + 0.7152 * d[1] + 0.0722 * d[2];
            (y, y, y)
        }
        Channel::Rgb => (d[0], d[1], d[2]),
    };
    o[0] = to_u8(b);
    o[1] = to_u8(g);
    o[2] = to_u8(r);
    o[3] = 255;
}

pub fn render(frame: &Frame, lut: &DisplayLut, channel: Channel) -> DisplayImage {
    let w = frame.width as usize;
    let h = frame.height as usize;
    let mut bgra = vec![0u8; w * h * 4];
    let band = w * 4 * BAND_ROWS;
    let alpha_only = channel == Channel::Alpha;
    match &frame.pixels {
        Pixels::Half(px) => bgra.par_chunks_mut(band).zip(px.par_chunks(band)).for_each(|(out, src)| {
            for (o, p) in out.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                let d = if alpha_only { [0.0; 3] } else { lut.apply(p[0], p[1], p[2]) };
                write(o, d, p[3].to_f32(), channel);
            }
        }),
        Pixels::Byte(px) => bgra.par_chunks_mut(band).zip(px.par_chunks(band)).for_each(|(out, src)| {
            for (o, p) in out.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                let d = if alpha_only { [0.0; 3] } else { lut.apply_u8(p[0], p[1], p[2]) };
                write(o, d, p[3] as f32 / 255.0, channel);
            }
        }),
    }
    DisplayImage { width: frame.width, height: frame.height, bgra }
}
