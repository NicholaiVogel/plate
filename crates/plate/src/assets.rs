//! Embedded fonts and line icons (24px grid, drawn for plate).

use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/InterTight-Regular.ttf"),
    include_bytes!("../assets/fonts/InterTight-Medium.ttf"),
    include_bytes!("../assets/fonts/InterTight-SemiBold.ttf"),
    include_bytes!("../assets/fonts/DMMono-Regular.ttf"),
    include_bytes!("../assets/fonts/DMMono-Medium.ttf"),
];

/// Used when burning text annotations into saved stills.
pub const BAKE_FONT: &[u8] = include_bytes!("../assets/fonts/InterTight-Medium.ttf");

const S: &str = r##"fill="none" stroke="#000" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round""##;

fn icon_body(name: &str) -> Option<String> {
    let p = |d: &[&str]| d.iter().map(|d| format!(r#"<path d="{d}"/>"#)).collect::<String>();
    Some(match name {
        "play" => r##"<path d="M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.5-7.2a.8.8 0 0 0 0-1.4L8.2 4.1a.8.8 0 0 0-1.2.7z" fill="#000" stroke="none"/>"##.into(),
        "pause" => r##"<rect x="6" y="4.5" width="4" height="15" rx=".8" fill="#000" stroke="none"/><rect x="14" y="4.5" width="4" height="15" rx=".8" fill="#000" stroke="none"/>"##.into(),
        "step-back" => p(&["M17 6v12l-8-6z", "M7 6v12"]),
        "step-forward" => p(&["M7 6v12l8-6z", "M17 6v12"]),
        "loop" => p(&["m17 2 4 4-4 4", "M3 11v-1a4 4 0 0 1 4-4h14", "m7 22-4-4 4-4", "M21 13v1a4 4 0 0 1-4 4H3"]),
        "volume" => p(&["M11 5 6 9H2v6h4l5 4z", "M15.5 8.5a5 5 0 0 1 0 7", "M19 5a10 10 0 0 1 0 14"]),
        "volume-x" => p(&["M11 5 6 9H2v6h4l5 4z", "m22 9-6 6", "m16 9 6 6"]),
        "chevron-down" => p(&["m6 9 6 6 6-6"]),
        "chevron-right" => p(&["m9 6 6 6-6 6"]),
        "pencil" => p(&["M21.17 6.81a2.82 2.82 0 0 0-3.99-3.99L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5z", "m15 5 4 4"]),
        "scribble" => p(&["M3 17c2.5-5 4.5-9 6.5-9 2.5 0 .5 8 3.5 8s3.5-6 8-9"]),
        "arrow" => p(&["M5 19 19 5", "M9 5h10v10"]),
        "line" => p(&["M5 19 19 5"]),
        "rect" => format!(r#"<rect x="4" y="5" width="16" height="14" rx="1.5"/>"#),
        "ellipse" => format!(r#"<ellipse cx="12" cy="12" rx="9" ry="7"/>"#),
        "text" => p(&["M5 7V5h14v2", "M12 5v14", "M9 19h6"]),
        "eraser" => p(&["m7 21-4.3-4.3c-1-1-1-2.5 0-3.4l9.6-9.6c1-1 2.5-1 3.4 0l5.6 5.6c1 1 1 2.5 0 3.4L13 21", "M22 21H7", "m5 11 9 9"]),
        "undo" => p(&["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 0 11H11"]),
        "trash" => p(&["M3 6h18", "M8 6V4h8v2", "M19 6l-1 14H6L5 6", "M10 11v6", "M14 11v6"]),
        "send" => p(&["M22 2 11 13", "M22 2 15 22l-4-9-9-4z"]),
        "save" => p(&["M12 3v12", "m7 10 5 5 5-5", "M5 21h14"]),
        "folder" => p(&["M3 6.5A2.5 2.5 0 0 1 5.5 4H9l2 3h7.5A2.5 2.5 0 0 1 21 9.5v8a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z"]),
        "film" => format!(r#"<rect x="3" y="3" width="18" height="18" rx="2"/>{}"#, p(&["M7 3v18", "M17 3v18", "M3 8h4", "M3 16h4", "M17 8h4", "M17 16h4", "M3 12h18"])),
        "layers" => p(&["m12 3 9 4.5-9 4.5-9-4.5z", "m3 12 9 4.5 9-4.5", "m3 16.5 9 4.5 9-4.5"]),
        "image" => format!(r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/>{}"#, p(&["m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"])),
        "projector" => format!(r#"<rect x="2" y="8" width="20" height="10" rx="2"/><circle cx="16" cy="13" r="3"/>{}"#, p(&["M6 12h3", "M6 21v-3", "M18 21v-3"])),
        "crosshair" => format!(r#"<circle cx="12" cy="12" r="8"/>{}"#, p(&["M12 2v5", "M12 17v5", "M2 12h5", "M17 12h5"])),
        "message" => p(&["M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"]),
        "compare" => format!(r#"<rect x="3" y="3" width="18" height="18" rx="2"/>{}"#, p(&["M12 3v18"])),
        "grid" => format!(r#"<rect x="3" y="3" width="18" height="18" rx="2"/>{}"#, p(&["M9 3v18", "M15 3v18", "M3 9h18", "M3 15h18"])),
        "captions" => format!(r#"<rect x="2.5" y="5" width="19" height="14" rx="2"/>{}"#, p(&["M10.5 10.2a2.2 2.2 0 1 0 0 3.6", "M17 10.2a2.2 2.2 0 1 0 0 3.6"])),
        "expand" => p(&["M15 3h6v6", "M9 21H3v-6", "M21 3l-7 7", "M3 21l7-7"]),
        "maximize" => p(&["M8 3H5a2 2 0 0 0-2 2v3", "M21 8V5a2 2 0 0 0-2-2h-3", "M3 16v3a2 2 0 0 0 2 2h3", "M16 21h3a2 2 0 0 0 2-2v-3"]),
        "x" => p(&["M18 6 6 18", "m6 6 12 12"]),
        "check" => p(&["M20 6 9 17l-5-5"]),
        "sidebar" => format!(r#"<rect x="3" y="3" width="18" height="18" rx="2"/>{}"#, p(&["M9 3v18"])),
        "up" => p(&["M12 19V5", "m5 12 7-7 7 7"]),
        "sliders" => p(&["M4 21v-7", "M4 10V3", "M12 21v-9", "M12 8V3", "M20 21v-5", "M20 12V3", "M1 14h6", "M9 8h6", "M17 16h6"]),
        "aperture" => format!(r#"<circle cx="12" cy="12" r="9"/>{}"#, p(&["m14.3 3.3 3.9 10.6", "M8.2 4.9h11.5", "M3.3 11.1 9 1.4", "M9.7 20.7 5.8 10.1", "M15.8 19.1H4.3", "M20.7 12.9 15 22.6"])),
        "eye" => format!(r#"<circle cx="12" cy="12" r="3"/>{}"#, p(&["M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"])),
        "circle" => r##"<circle cx="12" cy="12" r="6" fill="#000" stroke="none"/>"##.into(),
        _ => return None,
    })
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg")) else {
            return Ok(None);
        };
        Ok(icon_body(name).map(|body| {
            Cow::Owned(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" {S}>{body}</svg>"#).into_bytes())
        }))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
