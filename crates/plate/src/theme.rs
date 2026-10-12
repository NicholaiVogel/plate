//! Biohazard Night Archive: black surfaces, white ink, clay-blue accent,
//! blue-gray structure, hairline borders and near-square controls.

use gpui::{px, rgb, rgba, Hsla, Pixels, Rgba};
use std::sync::atomic::{AtomicU32, Ordering};

/// UI scale factor (Ctrl +/−), applied to every chrome dimension through
/// `s()`. Image-space geometry stays in raw `px` so 100% zoom still means one
/// image pixel per device pixel.
static UI_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

pub const UI_SCALE_MIN: f32 = 0.6;
pub const UI_SCALE_MAX: f32 = 2.5;

pub fn ui_scale() -> f32 {
    f32::from_bits(UI_SCALE.load(Ordering::Relaxed))
}

pub fn set_ui_scale(v: f32) -> f32 {
    let v = ((v.clamp(UI_SCALE_MIN, UI_SCALE_MAX)) * 20.0).round() / 20.0;
    UI_SCALE.store(v.to_bits(), Ordering::Relaxed);
    v
}

/// A UI length in logical pixels at the current UI scale.
pub fn s(v: f32) -> Pixels {
    px(v * ui_scale())
}

pub const FONT_UI: &str = "Inter Tight";
pub const FONT_MONO: &str = "DM Mono";

pub fn paper() -> Rgba {
    rgb(0x101010)
}
/// The viewer sits a touch darker than the chrome so footage reads cleanly.
pub fn viewer_bg() -> Rgba {
    rgb(0x0a0a0a)
}
pub fn raised() -> Rgba {
    rgb(0x161616)
}
pub fn ink() -> Rgba {
    rgb(0xf7f8f5)
}
pub fn ink_muted() -> Rgba {
    rgba(0xf7f8f58c)
}
pub fn ink_faint() -> Rgba {
    rgba(0xf7f8f54d)
}
pub fn hairline() -> Rgba {
    rgba(0xf7f8f51f)
}
pub fn hairline_strong() -> Rgba {
    rgba(0xf7f8f540)
}
pub fn hover() -> Rgba {
    rgba(0xf7f8f50f)
}
pub fn blue() -> Rgba {
    rgb(0x7895a7)
}
pub fn blue_soft() -> Rgba {
    rgba(0x7895a733)
}
pub fn structure() -> Rgba {
    rgb(0x27343b)
}
pub fn warn() -> Rgba {
    rgb(0xc99a5b)
}
pub fn scrim() -> Rgba {
    rgba(0x000000b3)
}

/// Barely-there rounding: controls, rows and cards.
pub const RADIUS: f32 = 4.0;
/// Modals and floating islands.
pub const RADIUS_LG: f32 = 6.0;

/// Annotation colors, chosen to read on top of footage.
pub const MARKERS: [[u8; 3]; 7] = [
    [0xff, 0x4d, 0x4d],
    [0xff, 0xd2, 0x3f],
    [0x3d, 0xdc, 0x84],
    [0x5a, 0xd1, 0xff],
    [0xc4, 0x6b, 0xff],
    [0xf7, 0xf8, 0xf5],
    [0x10, 0x10, 0x10],
];

pub fn marker(c: [u8; 3]) -> Hsla {
    rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32).into()
}
