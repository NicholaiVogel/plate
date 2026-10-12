//! SRT and WebVTT captions for previewing over playback.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    /// Lines with markup stripped.
    pub lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Captions {
    pub path: PathBuf,
    pub cues: Vec<Cue>,
}

impl Captions {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        // Captions in the wild are UTF-8 (often with a BOM) or Latin-1.
        let text = match String::from_utf8(raw) {
            Ok(s) => s,
            Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
        };
        let mut cues = parse(&text)?;
        cues.sort_by(|a, b| a.start.total_cmp(&b.start));
        Ok(Self { path: path.to_path_buf(), cues })
    }

    /// Cues showing at `secs` (overlapping cues stack, earliest first).
    pub fn at(&self, secs: f64) -> Vec<&Cue> {
        self.cues.iter().filter(|c| secs >= c.start && secs < c.end).collect()
    }
}

/// Parses SRT or WebVTT; the format is detected from the content.
pub fn parse(text: &str) -> Result<Vec<Cue>> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let vtt = text.trim_start().starts_with("WEBVTT");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let mut lines = block.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).peekable();
        // Skip an index line (SRT) or cue identifier (VTT) before the timing.
        let timing = loop {
            match lines.next() {
                Some(l) if l.contains("-->") => break Some(l),
                Some(l) if vtt && (l.starts_with("WEBVTT") || l.starts_with("NOTE") || l.starts_with("STYLE") || l.starts_with("REGION")) => {
                    break None;
                }
                Some(_) => continue,
                None => break None,
            }
        };
        let Some(timing) = timing else { continue };
        let mut parts = timing.split("-->");
        let (Some(a), Some(b)) = (parts.next(), parts.next()) else { continue };
        // VTT cue settings follow the end time ("00:01.000 line:90%").
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_time(a.trim()), parse_time(b.trim())) else { continue };
        let body: Vec<String> = lines.map(strip_markup).filter(|l| !l.is_empty()).collect();
        if !body.is_empty() && end > start {
            cues.push(Cue { start, end, lines: body });
        }
    }
    if cues.is_empty() && !text.trim().is_empty() {
        bail!("no caption cues found");
    }
    Ok(cues)
}

/// `HH:MM:SS,mmm`, `HH:MM:SS.mmm` or VTT's `MM:SS.mmm`.
fn parse_time(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, sec] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        [m, sec] => (0.0, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Drops `<i>`, `<font …>`, `<c.class>`, `<00:01.000>` and `{\an8}` style
/// markup, and decodes the common entities.
fn strip_markup(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth_angle = false;
    let mut depth_brace = false;
    for ch in line.chars() {
        match ch {
            '<' => depth_angle = true,
            '>' if depth_angle => depth_angle = false,
            '{' => depth_brace = true,
            '}' if depth_brace => depth_brace = false,
            _ if depth_angle || depth_brace => {}
            _ => out.push(ch),
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").trim().to_string()
}

/// Caption files next to a piece of media: `clip.srt`, `clip.en.vtt`, …
/// For a sequence `shot_v001.####.exr` the stem is `shot_v001`.
pub fn find_sidecars(media: &Path, stem_hint: Option<&str>) -> Vec<PathBuf> {
    let Some(dir) = media.parent() else { return Vec::new() };
    let stem = stem_hint
        .map(str::to_string)
        .or_else(|| media.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_default();
    let stem = stem.trim_end_matches(['.', '_', '-']).to_lowercase();
    if stem.is_empty() {
        return Vec::new();
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let ext = crate::scan::ext_of(p);
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            (ext == "srt" || ext == "vtt") && (name.starts_with(&format!("{stem}.")) || name.starts_with(&format!("{stem}_")))
        })
        .collect();
    // Prefer the plain `clip.srt` over language variants.
    found.sort_by_key(|p| (p.file_name().map(|n| n.len()).unwrap_or(0), p.clone()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt() {
        let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i> there\r\nsecond line\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\n{\\an8}Top &amp; bottom\r\n";
        let cues = parse(srt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].end, 2.5);
        assert_eq!(cues[0].lines, vec!["Hello there", "second line"]);
        assert_eq!(cues[1].lines, vec!["Top & bottom"]);
    }

    #[test]
    fn parses_vtt() {
        let vtt = "WEBVTT\n\nNOTE a comment\n\nintro\n00:01.000 --> 00:02.000 line:90% align:center\n<c.yellow>Hi</c>\n\n00:00:05.250 --> 00:00:06.000\nBye";
        let cues = parse(vtt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].lines, vec!["Hi"]);
        assert_eq!(cues[1].start, 5.25);
    }

    #[test]
    fn finds_cues_by_time() {
        let c = Captions { path: PathBuf::new(), cues: parse("1\n00:00:01,000 --> 00:00:02,000\nA\n").unwrap() };
        assert!(c.at(0.5).is_empty());
        assert_eq!(c.at(1.5).len(), 1);
        assert!(c.at(2.0).is_empty());
    }
}
