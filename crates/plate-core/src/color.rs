//! OCIO config selection, per-format defaults, and processor caching.

use crate::media::{MediaInfo, MediaKind};
use crate::prefs::{ColorChoice, OcioChoice};
use anyhow::Result;
use parking_lot::Mutex;
use crate::lut::{DisplayLut, Shaper};
use plate_native::{ColorSpaceInfo, OcioConfig, OcioProcessor};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ColorManager {
    pub config: OcioConfig,
    pub colorspaces: Vec<ColorSpaceInfo>,
    pub displays: Vec<String>,
    /// Shown in the UI: "ACES studio (built-in)", "$OCIO: /path", ...
    pub label: String,
    processors: Mutex<HashMap<ProcKey, OcioProcessor>>,
    luts: Mutex<HashMap<ProcKey, Arc<DisplayLut>>>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct ProcKey {
    choice: (String, String, String),
    exposure: u32,
    gamma: u32,
}

impl ColorManager {
    /// Loads the chosen config. Falls back to the built-in config and returns
    /// a warning when the choice can't be loaded.
    pub fn load(choice: &OcioChoice) -> (Self, Option<String>) {
        let env = std::env::var("OCIO").ok().filter(|v| !v.is_empty());
        let attempt: Result<(OcioConfig, String)> = match choice {
            OcioChoice::Auto => match &env {
                Some(p) => OcioConfig::from_env().map(|c| (c, format!("$OCIO · {p}"))),
                None => OcioConfig::builtin().map(|c| (c, "ACES studio config (built-in)".into())),
            },
            OcioChoice::Builtin => OcioConfig::builtin().map(|c| (c, "ACES studio config (built-in)".into())),
            OcioChoice::Env => match &env {
                Some(p) => OcioConfig::from_env().map(|c| (c, format!("$OCIO · {p}"))),
                None => Err(anyhow::anyhow!("$OCIO is not set")),
            },
            OcioChoice::File(p) => OcioConfig::from_file(p).map(|c| (c, p.clone())),
        };
        let (cfg, label, warning) = match attempt {
            Ok((c, l)) => (c, l, None),
            Err(e) => {
                let c = OcioConfig::builtin().expect("built-in OCIO config");
                (c, "ACES studio config (built-in)".into(), Some(format!("OCIO config failed to load, using built-in: {e:#}")))
            }
        };
        (Self::new(cfg, label), warning)
    }

    pub fn new(config: OcioConfig, label: String) -> Self {
        Self {
            colorspaces: config.colorspaces(),
            displays: config.displays(),
            label,
            config,
            processors: Mutex::new(HashMap::new()),
            luts: Mutex::new(HashMap::new()),
        }
    }

    /// Key used for remembered choices; names only make sense within a config.
    pub fn memory_key(&self) -> String {
        self.config.source().to_string()
    }

    pub fn views(&self, display: &str) -> Vec<String> {
        self.config.views(display)
    }

    fn first_existing(&self, names: &[&str]) -> Option<String> {
        names.iter().find_map(|n| self.config.resolve(n))
    }

    fn preferred_view(&self, display: &str, names: &[&str]) -> String {
        let views = self.config.views(display);
        names
            .iter()
            .find_map(|n| views.iter().find(|v| v.eq_ignore_ascii_case(n)).cloned())
            .unwrap_or_else(|| self.config.default_view(display))
    }

    /// What a format gets the first time it is opened.
    pub fn default_choice(&self, info: &MediaInfo) -> ColorChoice {
        let display = self.config.default_display();
        let scene_linear = || self.first_existing(&["scene_linear", "ACEScg", "Linear Rec.709 (sRGB)"]);
        let srgb = || {
            self.first_existing(&[
                "sRGB Encoded Rec.709 (sRGB)",
                "sRGB - Texture",
                "Utility - sRGB - Texture",
                "Output - sRGB",
                "sRGB",
                "color_picking",
                "matte_paint",
            ])
        };
        let codec = info.format.codec.as_str();
        let (cs, display_referred) = match info.kind {
            MediaKind::Dcp => (self.first_existing(&["Linear Rec.709 (sRGB)", "lin_rec709", "scene_linear"]), true),
            _ if info.colorspace_hint.as_deref() == Some("pq") => {
                (self.first_existing(&["Rec.2100-PQ - Display", "ST2084-P3-D65 - Display"]).or_else(srgb), true)
            }
            _ if info.colorspace_hint.as_deref() == Some("hlg") => {
                (self.first_existing(&["Rec.2100-HLG - Display"]).or_else(srgb), true)
            }
            _ if codec == "half" || codec == "float" => (scene_linear(), false),
            _ if matches!(info.format.container.as_str(), "dpx" | "cin") => {
                (self.first_existing(&["ADX10", "Cineon", "lg10", "compositing_log"]).or_else(srgb), false)
            }
            _ => (srgb(), true),
        };
        let colorspace = cs.or_else(scene_linear).unwrap_or_else(|| {
            self.colorspaces.first().map(|c| c.name.clone()).unwrap_or_default()
        });
        // Display-referred media (video, 8-bit stills, DCP) should round-trip
        // to what any other player shows, so skip the tone-mapping view.
        let view = if display_referred {
            self.preferred_view(&display, &["Un-tone-mapped", "Raw", "None"])
        } else {
            self.config.default_view(&display)
        };
        ColorChoice { colorspace, display, view }
    }

    /// Drops a remembered choice whose names no longer exist in this config.
    pub fn validate(&self, c: &ColorChoice) -> bool {
        self.config.resolve(&c.colorspace).is_some()
            && self.displays.contains(&c.display)
            && self.config.views(&c.display).contains(&c.view)
    }

    pub fn processor(&self, c: &ColorChoice, exposure: f32, gamma: f32) -> Result<OcioProcessor> {
        let key = ProcKey {
            choice: (c.colorspace.clone(), c.display.clone(), c.view.clone()),
            exposure: exposure.to_bits(),
            gamma: gamma.to_bits(),
        };
        if let Some(p) = self.processors.lock().get(&key) {
            return Ok(p.clone());
        }
        let p = self.config.display_processor(&c.colorspace, &c.display, &c.view, exposure, gamma)?;
        let mut cache = self.processors.lock();
        if cache.len() > 64 {
            cache.clear();
        }
        cache.insert(key, p.clone());
        Ok(p)
    }

    /// The baked display transform for a choice. Scene-linear inputs get a
    /// log shaper; everything else is assumed to live in 0..1.
    pub fn display_lut(&self, c: &ColorChoice, exposure: f32, gamma: f32) -> Result<Arc<DisplayLut>> {
        let key = ProcKey {
            choice: (c.colorspace.clone(), c.display.clone(), c.view.clone()),
            exposure: exposure.to_bits(),
            gamma: gamma.to_bits(),
        };
        if let Some(l) = self.luts.lock().get(&key) {
            return Ok(l.clone());
        }
        let proc = self.processor(c, exposure, gamma)?;
        let lut = Arc::new(DisplayLut::bake(&proc, self.shaper_for(&c.colorspace))?);
        let mut cache = self.luts.lock();
        if cache.len() > 24 {
            cache.clear();
        }
        cache.insert(key, lut.clone());
        Ok(lut)
    }

    pub fn shaper_for(&self, colorspace: &str) -> Shaper {
        match self.config.encoding(colorspace).as_str() {
            "scene-linear" | "display-linear" => Shaper::Log,
            "" => {
                let n = colorspace.to_ascii_lowercase();
                if n.contains("lin") || n.contains("aces2065") || n.contains("acescg") || n == "scene_linear" {
                    Shaper::Log
                } else {
                    Shaper::Unit
                }
            }
            _ => Shaper::Unit,
        }
    }

    /// Colorspaces grouped by family for the picker, families sorted, entries in config order.
    pub fn grouped(&self) -> Vec<(String, Vec<&ColorSpaceInfo>)> {
        let mut groups: Vec<(String, Vec<&ColorSpaceInfo>)> = Vec::new();
        for cs in &self.colorspaces {
            let fam = if cs.family.is_empty() { "Other".to_string() } else { cs.family.clone() };
            match groups.iter_mut().find(|(f, _)| *f == fam) {
                Some((_, v)) => v.push(cs),
                None => groups.push((fam, vec![cs])),
            }
        }
        groups
    }
}
