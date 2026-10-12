//! The display transform baked into a shaper + 3D LUT.
//!
//! The ACES 2.0 output transforms are too expensive to evaluate per pixel on
//! the CPU at playback rates, so the exact OCIO processor is sampled once on a
//! 65³ grid. Frames are half float, so the shaper is a table indexed by the
//! raw f16 bits: no per-pixel log or conversion. Interpolation is tetrahedral.

use anyhow::Result;
use half::f16;
use plate_native::OcioProcessor;
use rayon::prelude::*;

pub const LUT_SIZE: usize = 65;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shaper {
    /// For scene-linear input: log2 spacing from 0 up to 2^10.
    Log,
    /// For display-referred or log-encoded input already in 0..1.
    Unit,
}

const LOG_EPS: f32 = 1.0 / 4096.0;
const LOG_MAX: f32 = 1024.0;

impl Shaper {
    fn forward(self, x: f32) -> f32 {
        let x = if x.is_nan() { 0.0 } else { x };
        match self {
            Shaper::Log => {
                let lo = LOG_EPS.log2();
                let hi = (LOG_MAX + LOG_EPS).log2();
                (((x.max(0.0) + LOG_EPS).log2() - lo) / (hi - lo)).clamp(0.0, 1.0)
            }
            Shaper::Unit => x.clamp(0.0, 1.0),
        }
    }

    fn inverse(self, t: f32) -> f32 {
        match self {
            Shaper::Log => {
                let lo = LOG_EPS.log2();
                let hi = (LOG_MAX + LOG_EPS).log2();
                (lo + t * (hi - lo)).exp2() - LOG_EPS
            }
            Shaper::Unit => t,
        }
    }
}

pub struct DisplayLut {
    /// f16 bits -> lattice coordinate in [0, LUT_SIZE - 1].
    shaper: Vec<f32>,
    /// RGB triples, index (r * N + g) * N + b.
    table: Vec<[f32; 3]>,
}

impl DisplayLut {
    pub fn bake(proc: &OcioProcessor, shaper: Shaper) -> Result<Self> {
        let n = LUT_SIZE;
        let scale = (n - 1) as f32;
        let axis: Vec<f32> = (0..n).map(|i| shaper.inverse(i as f32 / scale)).collect();

        // One slab per red index; each slab is N*N pixels, evaluated in parallel.
        let slabs: Vec<Vec<[f32; 3]>> = (0..n)
            .into_par_iter()
            .map(|r| -> Result<Vec<[f32; 3]>> {
                let mut buf = Vec::with_capacity(n * n * 4);
                for g in 0..n {
                    for b in 0..n {
                        buf.extend_from_slice(&[axis[r], axis[g], axis[b], 1.0]);
                    }
                }
                proc.apply_rgba(&mut buf, n * n, 1)?;
                Ok(buf.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect())
            })
            .collect::<Result<_>>()?;
        let table = slabs.into_iter().flatten().collect();

        let shaper_table = (0..=u16::MAX)
            .map(|bits| shaper.forward(f16::from_bits(bits).to_f32()) * scale)
            .collect();
        Ok(Self { shaper: shaper_table, table })
    }

    #[inline]
    fn node(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        self.table[(r * LUT_SIZE + g) * LUT_SIZE + b]
    }

    /// Display RGB for one half-float pixel.
    #[inline]
    pub fn apply(&self, r: f16, g: f16, b: f16) -> [f32; 3] {
        let fr = self.shaper[r.to_bits() as usize];
        let fg = self.shaper[g.to_bits() as usize];
        let fb = self.shaper[b.to_bits() as usize];
        let max = (LUT_SIZE - 2) as f32;
        let (ri, gi, bi) = (fr.min(max) as usize, fg.min(max) as usize, fb.min(max) as usize);
        let (dr, dg, db) = (fr - ri as f32, fg - gi as f32, fb - bi as f32);

        let c000 = self.node(ri, gi, bi);
        let c111 = self.node(ri + 1, gi + 1, bi + 1);
        let mix = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], w0: f32, w1: f32, w2: f32, w3: f32| {
            [
                a[0] * w0 + b[0] * w1 + c[0] * w2 + d[0] * w3,
                a[1] * w0 + b[1] * w1 + c[1] * w2 + d[1] * w3,
                a[2] * w0 + b[2] * w1 + c[2] * w2 + d[2] * w3,
            ]
        };
        // Tetrahedral interpolation: pick the tetrahedron by the ordering of
        // the fractional parts.
        if dr > dg {
            if dg > db {
                let (c100, c110) = (self.node(ri + 1, gi, bi), self.node(ri + 1, gi + 1, bi));
                mix(c000, c100, c110, c111, 1.0 - dr, dr - dg, dg - db, db)
            } else if dr > db {
                let (c100, c101) = (self.node(ri + 1, gi, bi), self.node(ri + 1, gi, bi + 1));
                mix(c000, c100, c101, c111, 1.0 - dr, dr - db, db - dg, dg)
            } else {
                let (c001, c101) = (self.node(ri, gi, bi + 1), self.node(ri + 1, gi, bi + 1));
                mix(c000, c001, c101, c111, 1.0 - db, db - dr, dr - dg, dg)
            }
        } else if db > dg {
            let (c001, c011) = (self.node(ri, gi, bi + 1), self.node(ri, gi + 1, bi + 1));
            mix(c000, c001, c011, c111, 1.0 - db, db - dg, dg - dr, dr)
        } else if db > dr {
            let (c010, c011) = (self.node(ri, gi + 1, bi), self.node(ri, gi + 1, bi + 1));
            mix(c000, c010, c011, c111, 1.0 - dg, dg - db, db - dr, dr)
        } else {
            let (c010, c110) = (self.node(ri, gi + 1, bi), self.node(ri + 1, gi + 1, bi));
            mix(c000, c010, c110, c111, 1.0 - dg, dg - dr, dr - db, db)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_matches_exact_within_tolerance() {
        let cfg = plate_native::OcioConfig::builtin().unwrap();
        let display = cfg.default_display();
        let view = cfg.default_view(&display);
        let src = cfg.resolve("ACEScg").unwrap();
        let proc = cfg.display_processor(&src, &display, &view, 0.0, 1.0).unwrap();
        let lut = DisplayLut::bake(&proc, Shaper::Log).unwrap();
        let (mut worst, mut total, mut count) = (0f32, 0f32, 0u32);
        for &v in &[0.0f32, 0.002, 0.01, 0.05, 0.18, 0.5, 1.0, 2.5, 8.0, 40.0] {
            for rgb in [[v, v, v], [v, v * 0.5, v * 0.1], [v * 0.2, v, v * 0.7]] {
                let got = lut.apply(f16::from_f32(rgb[0]), f16::from_f32(rgb[1]), f16::from_f32(rgb[2]));
                // Compare against the exact transform of the f16-rounded input.
                let mut exact16 = vec![
                    f16::from_f32(rgb[0]).to_f32(),
                    f16::from_f32(rgb[1]).to_f32(),
                    f16::from_f32(rgb[2]).to_f32(),
                    1.0,
                ];
                proc.apply_rgba(&mut exact16, 1, 1).unwrap();
                for c in 0..3 {
                    let e = (got[c].clamp(0.0, 1.0) - exact16[c].clamp(0.0, 1.0)).abs();
                    worst = worst.max(e);
                    total += e;
                    count += 1;
                }
            }
        }
        // The worst case sits where ACES 2.0 hard-clips at display white, which
        // no lattice reproduces exactly; elsewhere errors are far below a code value.
        assert!(worst < 1.5 / 255.0, "worst error {worst}");
        assert!(total / (count as f32) < 0.25 / 255.0, "mean error {}", total / count as f32);
    }
}
