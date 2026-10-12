use crate::frame::Frame;
use crate::scan::{self, Sequence};
use crate::source::{dcp, images, video};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Sequence,
    Still,
    Dcp,
}

/// Identifies "this kind of file" for remembered color settings: container plus
/// codec or sample format, e.g. `mov/prores`, `mp4/h264`, `exr/half`, `dpx/10bit`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormatKey {
    pub container: String,
    pub codec: String,
}

impl FormatKey {
    pub fn new(container: impl Into<String>, codec: impl Into<String>) -> Self {
        Self { container: container.into(), codec: codec.into() }
    }

    pub fn id(&self) -> String {
        format!("{}/{}", self.container, self.codec)
    }

    /// Human label, e.g. `MOV · ProRes`.
    pub fn label(&self) -> String {
        format!("{} · {}", self.container.to_uppercase(), pretty_codec(&self.codec))
    }
}

fn pretty_codec(codec: &str) -> String {
    match codec {
        "prores" => "ProRes".into(),
        "h264" => "H.264".into(),
        "hevc" => "HEVC".into(),
        "av1" => "AV1".into(),
        "vp9" => "VP9".into(),
        "dnxhd" => "DNxHD/HR".into(),
        "jpeg2000" => "JPEG 2000".into(),
        "mjpeg" => "Motion JPEG".into(),
        "half" => "half float".into(),
        "float" => "float".into(),
        other => other.to_string(),
    }
}

/// Everything known about a piece of media without decoding frames.
#[derive(Debug, Clone)]
pub struct MediaInfo {
    /// Unique per open, used as the cache namespace.
    pub id: u64,
    pub kind: MediaKind,
    /// The file the user opened (first frame for sequences, the folder for DCPs).
    pub path: PathBuf,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f32,
    pub frame_count: u64,
    /// Frame number of index 0 (1001 for a typical comp sequence, 0 for movies).
    pub first_frame: i64,
    pub fps: f64,
    pub format: FormatKey,
    /// Codec / encoding detail for the info readout, e.g. `prores (4444) yuva444p12le`.
    pub detail: String,
    pub has_alpha: bool,
    pub has_audio: bool,
    /// A colorspace hint from file metadata, when present.
    pub colorspace_hint: Option<String>,
    /// Timecode of the first frame, when the file carries one.
    pub timecode: Option<String>,
    /// Sidecar file holding review notes for this media.
    pub notes_path: PathBuf,
    /// Cached bytes per pixel: 4 for 8-bit sources, 8 for half float.
    pub bytes_per_pixel: u32,
}

impl MediaInfo {
    pub fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * self.bytes_per_pixel as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frame_count as f64 / self.fps.max(1e-6)
    }
}

/// Decodes frames by clip index (0-based). Each reader is used from one thread.
pub trait FrameReader: Send {
    fn read(&mut self, index: u64) -> Result<Frame>;
}

pub trait FrameSource: Send + Sync {
    fn info(&self) -> &MediaInfo;
    fn reader(&self) -> Result<Box<dyn FrameReader>>;
    /// How many readers can usefully decode in parallel. Image sequences scale
    /// with cores; long-GOP video wants one stateful decoder.
    fn parallelism(&self) -> usize;
    /// Audio segments to play under this clip, if any.
    fn audio(&self) -> Vec<crate::audio::AudioSegment> {
        Vec::new()
    }
}

pub type Source = Arc<dyn FrameSource>;

/// Sidecar path for a single file: `clip.mov` -> `clip.mov.plate.json`.
pub(crate) fn file_notes_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".plate.json");
    path.with_file_name(name)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Opens any supported path: a movie, a still, one frame of a sequence (opens the
/// whole sequence), or a DCP folder.
pub fn open(path: &Path) -> Result<Source> {
    let path = path.canonicalize().with_context(|| format!("{} not found", path.display()))?;
    if path.is_dir() {
        if scan::is_dcp_dir(&path) {
            return Ok(Arc::new(dcp::DcpSource::open(&path)?));
        }
        bail!("{} is a folder; open it as a bin", path.display());
    }
    // ASSETMAP or CPL picked directly: open the package.
    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if fname.starts_with("ASSETMAP") {
        return Ok(Arc::new(dcp::DcpSource::open(path.parent().unwrap())?));
    }
    if scan::is_image(&path) {
        if let Some(seq) = scan::sequence_for(&path) {
            return open_sequence(seq);
        }
        return Ok(Arc::new(images::ImageSource::still(&path)?));
    }
    Ok(Arc::new(video::VideoSource::open(&path)?))
}

pub fn open_sequence(seq: Sequence) -> Result<Source> {
    Ok(Arc::new(images::ImageSource::sequence(seq)?))
}

/// Opens a bin entry.
pub fn open_entry(entry: &scan::BinEntry) -> Result<Source> {
    match &entry.kind {
        scan::EntryKind::Sequence(seq) => open_sequence(seq.clone()),
        scan::EntryKind::Directory => bail!("{} is a folder", entry.name),
        _ => open(&entry.path),
    }
}
