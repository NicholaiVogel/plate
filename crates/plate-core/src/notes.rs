//! Review comments with frame-anchored annotations, saved as a JSON sidecar.

use crate::media::MediaInfo;
use crate::prefs::{data_dir, write_atomic};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Coordinates are normalized to the image (0..1 on both axes); stroke widths
/// and text sizes are fractions of image height, so notes survive any zoom or
/// proxy resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Pen { points: Vec<[f32; 2]>, color: [u8; 3], width: f32 },
    Line { from: [f32; 2], to: [f32; 2], color: [u8; 3], width: f32 },
    Arrow { from: [f32; 2], to: [f32; 2], color: [u8; 3], width: f32 },
    Rect { from: [f32; 2], to: [f32; 2], color: [u8; 3], width: f32 },
    Ellipse { from: [f32; 2], to: [f32; 2], color: [u8; 3], width: f32 },
    Text { at: [f32; 2], text: String, color: [u8; 3], size: f32 },
}

impl Shape {
    pub fn color(&self) -> [u8; 3] {
        match self {
            Shape::Pen { color, .. }
            | Shape::Line { color, .. }
            | Shape::Arrow { color, .. }
            | Shape::Rect { color, .. }
            | Shape::Ellipse { color, .. }
            | Shape::Text { color, .. } => *color,
        }
    }

    /// Distance from a normalized point to the shape, in normalized units, for
    /// the eraser.
    pub fn distance(&self, p: [f32; 2]) -> f32 {
        fn seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let (x, y) = (a[0] + t * dx, a[1] + t * dy);
            ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()
        }
        match self {
            Shape::Pen { points, .. } => points.windows(2).map(|w| seg(p, w[0], w[1])).fold(f32::MAX, f32::min),
            Shape::Line { from, to, .. } | Shape::Arrow { from, to, .. } => seg(p, *from, *to),
            Shape::Rect { from, to, .. } => {
                let (a, b, c, d) = (*from, [to[0], from[1]], *to, [from[0], to[1]]);
                seg(p, a, b).min(seg(p, b, c)).min(seg(p, c, d)).min(seg(p, d, a))
            }
            Shape::Ellipse { from, to, .. } => {
                let (cx, cy) = ((from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0);
                let (rx, ry) = (((to[0] - from[0]) / 2.0).abs().max(1e-6), ((to[1] - from[1]) / 2.0).abs().max(1e-6));
                let (nx, ny) = ((p[0] - cx) / rx, (p[1] - cy) / ry);
                let r = (nx * nx + ny * ny).sqrt();
                (r - 1.0).abs() * rx.min(ry)
            }
            Shape::Text { at, size, .. } => {
                let (dx, dy) = (p[0] - at[0], p[1] - at[1]);
                if dx >= -0.01 && dy >= -0.01 && dy <= *size * 1.4 && dx <= *size * 8.0 { 0.0 } else { (dx * dx + dy * dy).sqrt() }
            }
        }
    }
}

/// The two barbs of an arrow head, in the same space as `from`/`to`.
pub fn arrow_head(from: [f32; 2], to: [f32; 2], size: f32) -> ([f32; 2], [f32; 2]) {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / len, dy / len);
    let size = size.min(len * 0.6);
    let angle: f32 = 0.45;
    let (c, s) = (angle.cos(), angle.sin());
    let left = [to[0] - size * (ux * c - uy * s), to[1] - size * (uy * c + ux * s)];
    let right = [to[0] - size * (ux * c + uy * s), to[1] - size * (uy * c - ux * s)];
    (left, right)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    /// Clip index (0-based).
    pub frame: u64,
    pub text: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub done: bool,
}

impl Comment {
    pub fn new(frame: u64, text: String, author: String, shapes: Vec<Shape>) -> Self {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Self {
            id: format!("{:x}{:04x}", now.as_millis(), std::process::id() & 0xffff),
            frame,
            text,
            author,
            created: now.as_secs(),
            shapes,
            done: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Notes {
    pub version: u32,
    pub media: String,
    pub comments: Vec<Comment>,
}

/// Fallback location when the media folder is read-only.
fn fallback_path(info: &MediaInfo) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    info.notes_path.hash(&mut h);
    data_dir().join("notes").join(format!("{:016x}.json", h.finish()))
}

impl Notes {
    pub fn load(info: &MediaInfo) -> Self {
        for p in [info.notes_path.clone(), fallback_path(info)] {
            if let Ok(s) = std::fs::read_to_string(&p) {
                match serde_json::from_str::<Notes>(&s) {
                    Ok(mut n) => {
                        n.comments.sort_by_key(|c| (c.frame, c.created));
                        return n;
                    }
                    Err(e) => log::warn!("{}: {e}", p.display()),
                }
            }
        }
        Notes { version: 1, media: info.path.to_string_lossy().into(), comments: Vec::new() }
    }

    /// Writes next to the media, falling back to the app data folder. Returns
    /// where it landed. An empty note set removes the sidecar.
    pub fn save(&self, info: &MediaInfo) -> Result<PathBuf> {
        let primary = info.notes_path.clone();
        let fallback = fallback_path(info);
        if self.comments.is_empty() {
            for p in [&primary, &fallback] {
                if p.exists() {
                    let _ = std::fs::remove_file(p);
                }
            }
            return Ok(primary);
        }
        let json = serde_json::to_string_pretty(self)?;
        match write_atomic(&primary, json.as_bytes()) {
            Ok(()) => Ok(primary),
            Err(e) => {
                log::info!("{} not writable ({e}); saving notes to {}", primary.display(), fallback.display());
                if let Some(d) = fallback.parent() {
                    std::fs::create_dir_all(d)?;
                }
                write_atomic(&fallback, json.as_bytes())?;
                Ok(fallback)
            }
        }
    }

    pub fn on_frame(&self, frame: u64) -> impl Iterator<Item = &Comment> {
        self.comments.iter().filter(move |c| c.frame == frame)
    }
}

/// True when `path` can be created or replaced.
pub fn writable(path: &Path) -> bool {
    path.parent().is_some_and(|d| {
        let probe = d.join(format!(".plate-write-test-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    })
}
