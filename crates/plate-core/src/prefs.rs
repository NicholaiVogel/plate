//! Persistent preferences in `~/.config/plate/prefs.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OcioChoice {
    /// `$OCIO` when set, else the built-in ACES studio config.
    #[default]
    Auto,
    Builtin,
    Env,
    File(String),
}

/// Input colorspace plus display/view, remembered per format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorChoice {
    pub colorspace: String,
    pub display: String,
    pub view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub ocio: OcioChoice,
    /// Config source -> format id (`mov/prores`) -> choice.
    pub color_memory: BTreeMap<String, BTreeMap<String, ColorChoice>>,
    pub cache_gb: f32,
    pub volume: f32,
    pub muted: bool,
    pub loop_playback: bool,
    pub show_comments: bool,
    pub show_bin: bool,
    pub annotation_color: usize,
    pub stroke_width: f32,
    pub author: String,
    pub sequence_fps: f64,
    pub recent: Vec<String>,
    /// UI scale factor, 1.0 = 100%.
    pub ui_scale: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            ocio: OcioChoice::Auto,
            color_memory: BTreeMap::new(),
            cache_gb: default_cache_gb(),
            volume: 1.0,
            muted: false,
            loop_playback: true,
            show_comments: true,
            show_bin: true,
            annotation_color: 0,
            stroke_width: 4.0,
            author: std::env::var("USER").unwrap_or_else(|_| "me".into()),
            sequence_fps: 24.0,
            recent: Vec::new(),
            ui_scale: 1.0,
        }
    }
}

/// A quarter of physical memory, capped at 48 GB.
fn default_cache_gb() -> f32 {
    let total_kb = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
        })
        .unwrap_or(16.0 * 1024.0 * 1024.0);
    ((total_kb / 1024.0 / 1024.0) * 0.25).clamp(2.0, 48.0) as f32
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("plate")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("plate")
}

impl Prefs {
    fn path() -> PathBuf {
        config_dir().join("prefs.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = write_atomic(&path, json.as_bytes()) {
                    log::warn!("saving prefs: {e}");
                }
            }
            Err(e) => log::warn!("serializing prefs: {e}"),
        }
    }

    pub fn remembered(&self, config: &str, format: &str) -> Option<&ColorChoice> {
        self.color_memory.get(config).and_then(|m| m.get(format))
    }

    pub fn remember(&mut self, config: &str, format: &str, choice: ColorChoice) {
        self.color_memory.entry(config.to_string()).or_default().insert(format.to_string(), choice);
    }

    pub fn forget(&mut self, config: &str, format: &str) {
        if let Some(m) = self.color_memory.get_mut(config) {
            m.remove(format);
        }
    }

    pub fn push_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(20);
    }
}

pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
