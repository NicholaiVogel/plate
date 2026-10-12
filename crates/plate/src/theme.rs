//! Biohazard Night Archive: black surfaces, white ink, clay-blue accent,
//! blue-gray structure, hairline borders and near-square controls.

use gpui::{rgb, rgba, Hsla, Rgba};

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

pub const RADIUS: f32 = 2.0;

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
