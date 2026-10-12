//! EXR layers (AOVs): grouping channel names into layers and mapping each
//! layer onto display RGBA.
//!
//! Handles the common conventions: unprefixed `R G B A`; `diffuse.R`;
//! Nuke's `diffuse.red`; Blender's `ViewLayer.Diffuse Color.R`; data passes
//! like `N.X/Y/Z`, `P.X/Y/Z`, `depth.Z`; and multi-part files.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Display name: the channel prefix, the part name, or "RGBA".
    pub name: String,
    pub subimage: u32,
    /// Channel index (within the part) feeding R, G, B, A; -1 for none.
    pub map: [i32; 4],
    /// Channel suffixes, for the picker ("R G B A", "Z").
    pub channels: Vec<String>,
}

impl Layer {
    /// Unprefixed RGBA in the first part, readable by the fast path.
    pub fn is_base(&self) -> bool {
        self.subimage == 0 && self.name == BASE
    }
}

pub const BASE: &str = "RGBA";

fn slot(suffix: &str) -> Option<usize> {
    match suffix.to_ascii_lowercase().as_str() {
        "r" | "red" | "x" | "u" => Some(0),
        "g" | "green" | "y" | "v" => Some(1),
        "b" | "blue" | "z" | "w" => Some(2),
        "a" | "alpha" => Some(3),
        _ => None,
    }
}

/// Groups one part's channels into layers, in first-seen order.
pub fn group(subimage: u32, part_name: &str, channels: &[String]) -> Vec<Layer> {
    let mut layers: Vec<(String, Vec<(usize, String)>)> = Vec::new();
    for (i, ch) in channels.iter().enumerate() {
        let (prefix, suffix) = match ch.rfind('.') {
            Some(dot) => (&ch[..dot], &ch[dot + 1..]),
            None => ("", ch.as_str()),
        };
        let name = match (prefix.is_empty(), part_name.is_empty()) {
            (false, _) => prefix.to_string(),
            (true, false) => part_name.to_string(),
            (true, true) => BASE.to_string(),
        };
        match layers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => v.push((i, suffix.to_string())),
            None => layers.push((name, vec![(i, suffix.to_string())])),
        }
    }
    layers
        .into_iter()
        .map(|(name, chans)| {
            let mut map = [-1i32; 4];
            if chans.len() == 1 {
                // A single channel (depth, a mask) shows as grey.
                let i = chans[0].0 as i32;
                map = [i, i, i, -1];
            } else {
                let mut rest = Vec::new();
                for (i, suffix) in &chans {
                    match slot(suffix) {
                        Some(s) if map[s] < 0 => map[s] = *i as i32,
                        _ => rest.push(*i as i32),
                    }
                }
                // Unrecognised names fill the remaining colour slots in order.
                for i in rest {
                    if let Some(s) = (0..3).find(|s| map[*s] < 0) {
                        map[s] = i;
                    }
                }
                if map[1] < 0 && map[0] >= 0 {
                    map[1] = map[0];
                }
                if map[2] < 0 && map[1] >= 0 {
                    map[2] = map[1];
                }
            }
            Layer { name, subimage, map, channels: chans.into_iter().map(|(_, s)| s).collect() }
        })
        .collect()
}

/// All layers in a file, the beauty first. Empty for files OIIO can't read.
pub fn layers_of(path: &Path) -> Vec<Layer> {
    let Ok(parts) = plate_native::oiio_parts(&path.to_string_lossy()) else { return Vec::new() };
    let mut out: Vec<Layer> = parts.iter().flat_map(|p| group(p.subimage, &p.name, &p.channels)).collect();
    // Names can repeat across parts; keep them distinct for the picker.
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for l in out.iter_mut() {
        let n = seen.entry(l.name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            l.name = format!("{} ({})", l.name, l.subimage);
        }
    }
    if let Some(i) = out.iter().position(|l| is_beauty(&l.name)) {
        let beauty = out.remove(i);
        out.insert(0, beauty);
    }
    out
}

fn is_beauty(name: &str) -> bool {
    let tail = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
    name == BASE || matches!(tail.as_str(), "combined" | "beauty" | "rgba" | "finalimage")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unreal_mrq_style() {
        let ch = names(&["R", "G", "B", "A", "FinalImageSceneDepth.R", "FinalImageSceneDepth.G", "FinalImageSceneDepth.B", "FinalImageSceneDepth.A"]);
        let l = group(0, "", &ch);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].name, "RGBA");
        assert!(l[0].is_base());
        assert_eq!(l[0].map, [0, 1, 2, 3]);
        assert_eq!(l[1].name, "FinalImageSceneDepth");
        assert_eq!(l[1].map, [4, 5, 6, 7]);
    }

    #[test]
    fn blender_and_data_passes() {
        let ch = names(&[
            "ViewLayer.Combined.A",
            "ViewLayer.Combined.B",
            "ViewLayer.Combined.G",
            "ViewLayer.Combined.R",
            "ViewLayer.Depth.Z",
            "ViewLayer.Normal.X",
            "ViewLayer.Normal.Y",
            "ViewLayer.Normal.Z",
        ]);
        let l = group(0, "", &ch);
        assert_eq!(l[0].name, "ViewLayer.Combined");
        assert_eq!(l[0].map, [3, 2, 1, 0]);
        assert_eq!(l[1].name, "ViewLayer.Depth");
        assert_eq!(l[1].map, [4, 4, 4, -1]);
        assert_eq!(l[2].map, [5, 6, 7, -1]);
    }

    #[test]
    fn nuke_long_names_and_parts() {
        let l = group(2, "spec", &names(&["spec.red", "spec.green", "spec.blue"]));
        assert_eq!(l[0].name, "spec");
        assert_eq!(l[0].subimage, 2);
        assert_eq!(l[0].map, [0, 1, 2, -1]);
        let l = group(1, "diffuse", &names(&["R", "G", "B"]));
        assert_eq!(l[0].name, "diffuse");
        assert!(!l[0].is_base());
    }
}
