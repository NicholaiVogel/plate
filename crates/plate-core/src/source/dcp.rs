//! Unencrypted Digital Cinema Packages (Interop and SMPTE).
//!
//! The ASSETMAP resolves asset UUIDs to MXF files; the CPL orders them into
//! reels. Picture essence is JPEG 2000 in X'Y'Z', decoded by FFmpeg and
//! converted to display-linear Rec.709 in `video::xyz12_to_frame`.

use super::video::{self, VideoReader};
use crate::audio::AudioSegment;
use crate::frame::Frame;
use crate::media::{self, FormatKey, FrameReader, FrameSource, MediaInfo, MediaKind};
use anyhow::{anyhow, bail, Context, Result};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct ReelAsset {
    path: PathBuf,
    entry: u64,
    duration: u64,
    edit_rate: f64,
}

#[derive(Debug, Clone)]
struct Reel {
    picture: ReelAsset,
    sound: Option<ReelAsset>,
}

pub struct DcpSource {
    info: MediaInfo,
    reels: Vec<Reel>,
}

fn child<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn text(node: Node, name: &str) -> Option<String> {
    child(node, name).and_then(|c| c.text()).map(|t| t.trim().to_string())
}

fn strip_urn(id: &str) -> String {
    id.trim().trim_start_matches("urn:uuid:").to_ascii_lowercase()
}

fn parse_rate(s: &str) -> Option<f64> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.parse().ok()?;
    let d: f64 = it.next().unwrap_or("1").parse().ok()?;
    (d != 0.0).then_some(n / d)
}

fn read_assetmap(dir: &Path) -> Result<HashMap<String, PathBuf>> {
    let path = ["ASSETMAP.xml", "ASSETMAP"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow!("no ASSETMAP in {}", dir.display()))?;
    let xml = std::fs::read_to_string(&path)?;
    let doc = Document::parse(&xml).context("ASSETMAP is not valid XML")?;
    let mut map = HashMap::new();
    let list = child(doc.root_element(), "AssetList").ok_or_else(|| anyhow!("ASSETMAP has no AssetList"))?;
    for asset in list.children().filter(|c| c.tag_name().name() == "Asset") {
        let Some(id) = text(asset, "Id") else { continue };
        let Some(chunk) = child(asset, "ChunkList").and_then(|c| child(c, "Chunk")) else { continue };
        let Some(p) = text(chunk, "Path") else { continue };
        map.insert(strip_urn(&id), dir.join(p.trim_start_matches("file://")));
    }
    Ok(map)
}

struct Cpl {
    title: String,
    reels: Vec<Reel>,
}

fn parse_asset(node: Node, assets: &HashMap<String, PathBuf>) -> Result<ReelAsset> {
    let id = text(node, "Id").ok_or_else(|| anyhow!("reel asset without Id"))?;
    let path = assets
        .get(&strip_urn(&id))
        .cloned()
        .ok_or_else(|| anyhow!("asset {id} is not in the ASSETMAP (supplemental or VF package?)"))?;
    let edit_rate = text(node, "EditRate").and_then(|r| parse_rate(&r)).unwrap_or(24.0);
    let intrinsic: u64 = text(node, "IntrinsicDuration").and_then(|v| v.parse().ok()).unwrap_or(0);
    let entry: u64 = text(node, "EntryPoint").and_then(|v| v.parse().ok()).unwrap_or(0);
    let duration = text(node, "Duration").and_then(|v| v.parse().ok()).unwrap_or(intrinsic.saturating_sub(entry));
    Ok(ReelAsset { path, entry, duration, edit_rate })
}

fn parse_cpl(path: &Path, assets: &HashMap<String, PathBuf>) -> Result<Option<Cpl>> {
    let xml = std::fs::read_to_string(path)?;
    let Ok(doc) = Document::parse(&xml) else { return Ok(None) };
    let root = doc.root_element();
    if root.tag_name().name() != "CompositionPlaylist" {
        return Ok(None);
    }
    let title = text(root, "ContentTitleText").unwrap_or_else(|| "DCP".into());
    let mut reels = Vec::new();
    let list = child(root, "ReelList").ok_or_else(|| anyhow!("CPL has no ReelList"))?;
    for reel in list.children().filter(|c| c.tag_name().name() == "Reel") {
        let Some(asset_list) = child(reel, "AssetList") else { continue };
        let picture = child(asset_list, "MainPicture").or_else(|| child(asset_list, "MainStereoscopicPicture"));
        let Some(picture) = picture else { continue };
        let picture = parse_asset(picture, assets)?;
        let sound = child(asset_list, "MainSound").and_then(|s| parse_asset(s, assets).ok());
        reels.push(Reel { picture, sound });
    }
    if reels.is_empty() {
        bail!("CPL {} has no picture reels", path.display());
    }
    Ok(Some(Cpl { title, reels }))
}

impl DcpSource {
    pub fn open(dir: &Path) -> Result<Self> {
        let assets = read_assetmap(dir)?;
        // A package can hold several CPLs (versions, trailers); play the longest.
        let mut best: Option<Cpl> = None;
        for p in assets.values().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml"))) {
            if let Ok(Some(cpl)) = parse_cpl(p, &assets) {
                let len = |c: &Cpl| c.reels.iter().map(|r| r.picture.duration).sum::<u64>();
                if best.as_ref().is_none_or(|b| len(&cpl) > len(b)) {
                    best = Some(cpl);
                }
            }
        }
        let cpl = best.ok_or_else(|| anyhow!("no CompositionPlaylist found in {}", dir.display()))?;
        let first = &cpl.reels[0].picture;
        if !first.path.is_file() {
            bail!("picture essence {} is missing", first.path.display());
        }
        let probe = video::probe(&first.path)?;
        let frame_count = cpl.reels.iter().map(|r| r.picture.duration).sum();
        let has_audio = cpl.reels.iter().any(|r| r.sound.as_ref().is_some_and(|s| s.path.is_file()));
        let info = MediaInfo {
            id: media::next_id(),
            kind: MediaKind::Dcp,
            path: dir.to_path_buf(),
            name: format!("{} (DCP)", cpl.title),
            width: probe.width,
            height: probe.height,
            pixel_aspect: 1.0,
            frame_count,
            first_frame: 0,
            fps: first.edit_rate,
            format: FormatKey::new("dcp", probe.codec),
            detail: format!("{}, {} reel(s)", probe.detail, cpl.reels.len()),
            has_alpha: false,
            has_audio,
            colorspace_hint: Some("dcdm".into()),
            timecode: None,
            notes_path: dir.join("plate-notes.json"),
        };
        Ok(Self { info, reels: cpl.reels })
    }
}

impl FrameSource for DcpSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn reader(&self) -> Result<Box<dyn FrameReader>> {
        Ok(Box::new(DcpReader { reels: self.reels.clone(), current: None }))
    }

    fn parallelism(&self) -> usize {
        // JPEG 2000 is intra-only and slow to decode: several readers in parallel.
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8)
    }

    fn audio(&self) -> Vec<AudioSegment> {
        self.reels
            .iter()
            .map(|r| match &r.sound {
                Some(s) if s.path.is_file() => AudioSegment::file(
                    s.path.clone(),
                    s.entry as f64 / s.edit_rate,
                    r.picture.duration as f64 / r.picture.edit_rate,
                ),
                // A reel without sound still occupies time on the timeline.
                _ => AudioSegment::silence(r.picture.duration as f64 / r.picture.edit_rate),
            })
            .collect()
    }
}

struct DcpReader {
    reels: Vec<Reel>,
    current: Option<(usize, VideoReader)>,
}

impl FrameReader for DcpReader {
    fn read(&mut self, index: u64) -> Result<Frame> {
        let mut remaining = index;
        let mut reel_idx = self.reels.len() - 1;
        for (i, r) in self.reels.iter().enumerate() {
            if remaining < r.picture.duration {
                reel_idx = i;
                break;
            }
            remaining -= r.picture.duration;
        }
        let pic = &self.reels[reel_idx].picture;
        let remaining = remaining.min(pic.duration.saturating_sub(1));
        if self.current.as_ref().map(|(i, _)| *i) != Some(reel_idx) {
            self.current = Some((reel_idx, VideoReader::open(&pic.path, pic.entry, pic.duration)?));
        }
        self.current.as_mut().unwrap().1.read(remaining)
    }
}
