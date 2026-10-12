//! Filesystem discovery: image-sequence grouping, media classification and
//! directory listings for the bin.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Extensions treated as single-frame images (and thus sequence candidates).
pub const IMAGE_EXTS: &[&str] = &[
    "exr", "sxr", "mxr", "dpx", "cin", "tif", "tiff", "png", "jpg", "jpeg", "tga", "bmp",
    "hdr", "webp", "psd", "jp2", "j2k", "j2c", "sgi", "rgb", "rgba", "iff", "pic", "ppm",
    "pgm", "pnm", "heic", "avif", "jxl", "dds", "ktx", "gif",
];

/// Extensions handed to FFmpeg as movies.
pub const VIDEO_EXTS: &[&str] = &[
    "mov", "qt", "mp4", "m4v", "mkv", "webm", "avi", "mxf", "mts", "m2ts", "ts", "mpg",
    "mpeg", "m2v", "wmv", "flv", "ogv", "3gp", "y4m", "r3d", "braw", "dv", "vob", "f4v",
    "ivf", "nut", "gxf", "lxf", "hevc", "h264", "264", "265", "av1", "obu",
];

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_image(path: &Path) -> bool {
    IMAGE_EXTS.contains(&ext_of(path).as_str())
}

pub fn is_video(path: &Path) -> bool {
    VIDEO_EXTS.contains(&ext_of(path).as_str())
}

/// A DCP directory is recognised by its ASSETMAP.
pub fn is_dcp_dir(path: &Path) -> bool {
    path.is_dir() && (path.join("ASSETMAP.xml").is_file() || path.join("ASSETMAP").is_file())
}

/// A numbered image sequence on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub dir: PathBuf,
    /// Everything before the frame number, e.g. `shot_v001.`
    pub prefix: String,
    /// Everything after the frame number, e.g. `.exr`
    pub suffix: String,
    /// Zero-padding width; 0 when unpadded.
    pub padding: usize,
    pub first: i64,
    pub last: i64,
    /// Frame numbers that exist on disk, sorted.
    pub present: Vec<i64>,
}

impl Sequence {
    pub fn path_for(&self, frame: i64) -> PathBuf {
        let num = if self.padding > 0 {
            if frame < 0 {
                format!("-{:0width$}", -frame, width = self.padding)
            } else {
                format!("{:0width$}", frame, width = self.padding)
            }
        } else {
            frame.to_string()
        };
        self.dir.join(format!("{}{}{}", self.prefix, num, self.suffix))
    }

    pub fn len(&self) -> u64 {
        (self.last - self.first + 1) as u64
    }

    pub fn missing(&self) -> u64 {
        self.len() - self.present.len() as u64
    }

    /// `shot_v001.####.exr` style pattern.
    pub fn pattern(&self) -> String {
        let hashes = "#".repeat(self.padding.max(1));
        format!("{}{}{}", self.prefix, hashes, self.suffix)
    }

    pub fn display_name(&self) -> String {
        format!("{} [{}-{}]", self.pattern(), self.first, self.last)
    }
}

/// Splits `name` into (prefix, digits, suffix) around the last run of digits
/// before the extension. A leading `-` directly after a separator is taken as a
/// negative frame sign.
fn split_frame(name: &str) -> Option<(String, String, String)> {
    let dot = name.rfind('.')?;
    let (stem, ext) = name.split_at(dot);
    let bytes = stem.as_bytes();
    let end = bytes.iter().rposition(|b| b.is_ascii_digit())? + 1;
    let mut start = end;
    while start > 0 && bytes[start - 1].is_ascii_digit() {
        start -= 1;
    }
    // The frame number need not be last in the stem: shot.1001_matte.exr is common.
    let mut prefix_end = start;
    if start >= 2 && bytes[start - 1] == b'-' && matches!(bytes[start - 2], b'.' | b'_') {
        prefix_end = start - 1;
    }
    Some((
        stem[..prefix_end].to_string(),
        stem[prefix_end..end].to_string(),
        format!("{}{}", &stem[end..], ext),
    ))
}

fn parse_frame(digits: &str) -> Option<(i64, usize)> {
    let neg = digits.starts_with('-');
    let body = digits.trim_start_matches('-');
    let n: i64 = body.parse().ok()?;
    let padding = if body.len() > 1 && body.starts_with('0') || body.len() >= 4 { body.len() } else { 0 };
    Some((if neg { -n } else { n }, padding))
}

/// Finds the sequence a single image belongs to. Returns None when the file has
/// no frame number or is the only frame.
pub fn sequence_for(path: &Path) -> Option<Sequence> {
    let name = path.file_name()?.to_str()?;
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let (prefix, digits, suffix) = split_frame(name)?;
    let (_, padding) = parse_frame(&digits)?;
    let mut present = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let n = entry.file_name();
        let Some(n) = n.to_str() else { continue };
        if let Some((p, d, s)) = split_frame(n) {
            if p == prefix && s == suffix {
                if let Some((f, pad)) = parse_frame(&d) {
                    if pad == padding || (padding == 0 && pad == 0) {
                        present.push(f);
                    }
                }
            }
        }
    }
    if present.len() < 2 {
        return None;
    }
    present.sort_unstable();
    Some(Sequence {
        dir,
        prefix,
        suffix,
        padding,
        first: present[0],
        last: *present.last().unwrap(),
        present,
    })
}

#[derive(Debug, Clone)]
pub enum EntryKind {
    Directory,
    Dcp,
    Video,
    Still,
    Sequence(Sequence),
}

#[derive(Debug, Clone)]
pub struct BinEntry {
    pub name: String,
    /// For sequences, the path of the first present frame.
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl BinEntry {
    pub fn is_playable(&self) -> bool {
        !matches!(self.kind, EntryKind::Directory)
    }
}

/// Lists a directory for the bin: groups numbered images into sequences, keeps
/// movies and stills, recognises DCP folders, and lists subdirectories.
pub fn scan_dir(dir: &Path) -> std::io::Result<Vec<BinEntry>> {
    // Absolute paths, so entries compare equal to opened media.
    let dir = &dir.canonicalize()?;
    let mut dirs = Vec::new();
    let mut media = Vec::new();
    let mut groups: BTreeMap<(String, String, usize), Vec<(i64, PathBuf)>> = BTreeMap::new();

    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            let kind = if is_dcp_dir(&path) { EntryKind::Dcp } else { EntryKind::Directory };
            dirs.push(BinEntry { name, path, kind });
        } else if is_video(&path) {
            media.push(BinEntry { name, path, kind: EntryKind::Video });
        } else if is_image(&path) {
            match split_frame(&name).and_then(|(p, d, s)| Some((p, s, parse_frame(&d)?))) {
                Some((p, s, (f, pad))) => groups.entry((p, s, pad)).or_default().push((f, path)),
                None => media.push(BinEntry { name, path, kind: EntryKind::Still }),
            }
        }
    }

    for ((prefix, suffix, padding), mut frames) in groups {
        frames.sort_by_key(|(f, _)| *f);
        if frames.len() == 1 {
            let (_, path) = frames.pop().unwrap();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            media.push(BinEntry { name, path, kind: EntryKind::Still });
            continue;
        }
        let seq = Sequence {
            dir: dir.to_path_buf(),
            prefix,
            suffix,
            padding,
            first: frames[0].0,
            last: frames.last().unwrap().0,
            present: frames.iter().map(|(f, _)| *f).collect(),
        };
        media.push(BinEntry {
            name: seq.display_name(),
            path: frames[0].1.clone(),
            kind: EntryKind::Sequence(seq),
        });
    }

    dirs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    media.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    dirs.extend(media);
    Ok(dirs)
}

/// Case-insensitive natural ordering so `shot_2` sorts before `shot_10`.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    b.next();
                }
                let ord = na
                    .trim_start_matches('0')
                    .len()
                    .cmp(&nb.trim_start_matches('0').len())
                    .then_with(|| na.trim_start_matches('0').cmp(nb.trim_start_matches('0')));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_common_names() {
        assert_eq!(
            split_frame("shot_v001.1001.exr").unwrap(),
            ("shot_v001.".into(), "1001".into(), ".exr".into())
        );
        assert_eq!(
            split_frame("plate_0042_matte.png").unwrap(),
            ("plate_".into(), "0042".into(), "_matte.png".into())
        );
        assert_eq!(split_frame("neg.-005.exr").unwrap(), ("neg.".into(), "-005".into(), ".exr".into()));
        assert!(split_frame("noframe.exr").is_none());
    }

    #[test]
    fn path_round_trip() {
        let s = Sequence {
            dir: "/x".into(),
            prefix: "a.".into(),
            suffix: ".exr".into(),
            padding: 4,
            first: 1,
            last: 3,
            present: vec![1, 2, 3],
        };
        assert_eq!(s.path_for(12), PathBuf::from("/x/a.0012.exr"));
        assert_eq!(s.pattern(), "a.####.exr");
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["shot_10", "shot_2", "Shot_1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["Shot_1", "shot_2", "shot_10"]);
    }
}
