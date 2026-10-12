//! Stills and numbered image sequences.
//!
//! EXR goes through the pure-Rust `exr` crate first, common 8/16-bit formats
//! through `image`, and everything else (DPX, Cineon, PSD, DWA-compressed EXR,
//! JPEG 2000, ...) through OpenImageIO.

use crate::frame::Frame;
use crate::media::{self, FormatKey, FrameReader, FrameSource, MediaInfo, MediaKind};
use crate::layers::{layers_of, Layer};
use crate::scan::{ext_of, Sequence};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Default playback rate for sequences that don't carry one.
pub const DEFAULT_SEQUENCE_FPS: f64 = 24.0;

pub struct ImageSource {
    info: MediaInfo,
    frames: Frames,
    /// Every layer in the file; empty for single-layer images.
    layers: Vec<Layer>,
    /// The layer to read; None reads the beauty through the fast path.
    layer: Option<Layer>,
}

#[derive(Clone)]
enum Frames {
    Still(PathBuf),
    Sequence(Sequence),
}

impl Frames {
    fn path(&self, index: u64) -> Option<PathBuf> {
        match self {
            Frames::Still(p) => Some(p.clone()),
            Frames::Sequence(seq) => {
                let frame = seq.first + index as i64;
                seq.present.binary_search(&frame).ok().map(|_| seq.path_for(frame))
            }
        }
    }
}

fn format_key(path: &Path, probe: &plate_native::OiioProbe) -> FormatKey {
    let ext = match ext_of(path).as_str() {
        "jpeg" => "jpg".to_string(),
        "tiff" => "tif".to_string(),
        e => e.to_string(),
    };
    let codec = match probe.format.as_str() {
        "half" | "float" => probe.format.clone(),
        _ => format!("{}bit", probe.bits),
    };
    FormatKey::new(ext, codec)
}

fn build_info(kind: MediaKind, path: &Path, name: String, frame_count: u64, first_frame: i64, notes_path: PathBuf) -> Result<MediaInfo> {
    let probe = plate_native::oiio_probe(&path.to_string_lossy())
        .map_err(|e| anyhow!("can't read {}: {e}", path.display()))?;
    let detail = format!("{} {} channels", probe.format, probe.channels);
    Ok(MediaInfo {
        id: media::next_id(),
        kind,
        path: path.to_path_buf(),
        name,
        width: probe.width,
        height: probe.height,
        pixel_aspect: if probe.pixel_aspect > 0.0 { probe.pixel_aspect } else { 1.0 },
        frame_count,
        first_frame,
        fps: if probe.fps > 0.0 { probe.fps as f64 } else { DEFAULT_SEQUENCE_FPS },
        format: format_key(path, &probe),
        detail,
        has_alpha: probe.has_alpha,
        has_audio: false,
        colorspace_hint: (!probe.colorspace.is_empty()).then(|| probe.colorspace.clone()),
        timecode: None,
        notes_path,
        // Only the `image` crate path keeps 8-bit data as bytes.
        bytes_per_pixel: if probe.bits <= 8 && matches!(ext_of(path).as_str(), "png" | "jpg" | "jpeg" | "tga" | "bmp" | "webp" | "gif") { 4 } else { 8 },
        layers: Vec::new(),
        layer: None,
    })
}

/// Layers of a multi-layer EXR (none for anything else), and the one to
/// show first: the beauty, read through the fast path when it's plain RGBA.
fn discover_layers(path: &Path, info: &mut MediaInfo) -> (Vec<Layer>, Option<Layer>) {
    if !matches!(ext_of(path).as_str(), "exr" | "sxr" | "mxr") {
        return (Vec::new(), None);
    }
    let layers = layers_of(path);
    if layers.len() <= 1 && layers.first().is_none_or(|l| l.is_base()) {
        return (Vec::new(), None);
    }
    info.layers = layers.iter().map(|l| l.name.clone()).collect();
    let first = layers[0].clone();
    info.layer = Some(first.name.clone());
    let selected = (!first.is_base()).then_some(first);
    (layers, selected)
}

impl ImageSource {
    pub fn still(path: &Path) -> Result<Self> {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let mut info = build_info(MediaKind::Still, path, name, 1, 0, media::file_notes_path(path))?;
        let (layers, layer) = discover_layers(path, &mut info);
        Ok(Self { info, frames: Frames::Still(path.to_path_buf()), layers, layer })
    }

    pub fn sequence(seq: Sequence) -> Result<Self> {
        let first = seq.path_for(seq.present[0]);
        let stem = seq.prefix.trim_end_matches(['.', '_', '-']);
        let stem = if stem.is_empty() { "sequence" } else { stem };
        // shot_v001.####.exr -> shot_v001.exr.plate.json
        let notes = seq.dir.join(format!("{stem}{}.plate.json", seq.suffix));
        let mut info = build_info(MediaKind::Sequence, &first, seq.display_name(), seq.len(), seq.first, notes)?;
        let (layers, layer) = discover_layers(&first, &mut info);
        Ok(Self { info, frames: Frames::Sequence(seq), layers, layer })
    }

    pub(crate) fn set_id(&mut self, id: u64) {
        self.info.id = id;
    }

    /// Switches to a layer by name; false if there's no such layer.
    pub fn select_layer(&mut self, name: &str) -> bool {
        let Some(l) = self.layers.iter().find(|l| l.name == name).cloned() else { return false };
        self.info.layer = Some(l.name.clone());
        self.layer = (!l.is_base()).then_some(l);
        true
    }

    pub fn sequence_info(&self) -> Option<&Sequence> {
        match &self.frames {
            Frames::Sequence(s) => Some(s),
            Frames::Still(_) => None,
        }
    }
}

impl FrameSource for ImageSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn reader(&self) -> Result<Box<dyn FrameReader>> {
        Ok(Box::new(ImageReader {
            frames: self.frames.clone(),
            layer: self.layer.clone(),
            size: (self.info.width, self.info.height),
        }))
    }

    fn with_layer(&self, name: &str, id: u64) -> Option<media::Source> {
        let mut copy = ImageSource {
            info: self.info.clone(),
            frames: self.frames.clone(),
            layers: self.layers.clone(),
            layer: self.layer.clone(),
        };
        copy.select_layer(name).then(|| {
            copy.set_id(id);
            std::sync::Arc::new(copy) as media::Source
        })
    }

    fn parallelism(&self) -> usize {
        match self.frames {
            Frames::Still(_) => 1,
            Frames::Sequence(_) => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 12),
        }
    }
}

struct ImageReader {
    frames: Frames,
    layer: Option<Layer>,
    size: (u32, u32),
}

impl FrameReader for ImageReader {
    fn read(&mut self, index: u64) -> Result<Frame> {
        let path = self.frames.path(index).ok_or_else(|| anyhow!("missing frame"))?;
        match &self.layer {
            Some(layer) => read_layer(&path, layer, self.size),
            None => read_image(&path),
        }
    }
}

/// Reads one layer's channels (only those) through OpenImageIO.
pub fn read_layer(path: &Path, layer: &Layer, (w, h): (u32, u32)) -> Result<Frame> {
    let mut f = Frame::new(w, h, layer.map[3] >= 0);
    let half = f.half_mut();
    // SAFETY: f16 is a transparent wrapper over u16.
    let bits: &mut [u16] = unsafe { std::slice::from_raw_parts_mut(half.as_mut_ptr().cast(), half.len()) };
    let info = plate_native::oiio_read_channels_half(&path.to_string_lossy(), layer.subimage, layer.map, bits, w, h)?;
    f.pixel_aspect = if info.pixel_aspect > 0.0 { info.pixel_aspect } else { 1.0 };
    Ok(f)
}

/// Decodes any supported still to half-float RGBA in display-window space.
pub fn read_image(path: &Path) -> Result<Frame> {
    match ext_of(path).as_str() {
        "exr" | "sxr" | "mxr" => read_exr(path).or_else(|_| read_oiio(path)),
        "png" | "jpg" | "jpeg" | "tga" | "bmp" | "webp" | "gif" => read_with_image(path).or_else(|_| read_oiio(path)),
        _ => read_oiio(path),
    }
}

fn read_exr(path: &Path) -> Result<Frame> {
    use exr::prelude::*;

    let image = read_first_rgba_layer_from_file(
        path,
        |size: Vec2<usize>, _channels: &RgbaChannels| (size.width(), vec![f16::ZERO; size.area() * 4]),
        |(width, px): &mut (usize, Vec<f16>), pos: Vec2<usize>, (r, g, b, a): (f16, f16, f16, f16)| {
            let i = (pos.y() * *width + pos.x()) * 4;
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = a;
        },
    )?;

    let layer = &image.layer_data;
    let has_alpha = layer.channel_data.channels.3.is_some();
    let data_size = layer.size;
    let data_pos = layer.attributes.layer_position;
    let display = image.attributes.display_window;
    let (_, pixels) = &layer.channel_data.pixels;

    let (dw, dh) = (display.size.width(), display.size.height());
    let mut frame = if data_pos == display.position && data_size == display.size {
        Frame { width: dw as u32, height: dh as u32, pixel_aspect: 1.0, has_alpha, pixels: crate::frame::Pixels::Half(pixels.clone()) }
    } else {
        // Data window differs from the display window (overscan or bbox crop).
        let mut f = Frame::new(dw as u32, dh as u32, has_alpha);
        let ox = data_pos.x() - display.position.x();
        let oy = data_pos.y() - display.position.y();
        for y in 0..data_size.height() {
            let ty = y as i32 + oy;
            if ty < 0 || ty >= dh as i32 {
                continue;
            }
            for x in 0..data_size.width() {
                let tx = x as i32 + ox;
                if tx < 0 || tx >= dw as i32 {
                    continue;
                }
                let s = (y * data_size.width() + x) * 4;
                let d = (ty as usize * dw + tx as usize) * 4;
                f.half_mut()[d..d + 4].copy_from_slice(&pixels[s..s + 4]);
            }
        }
        f
    };
    frame.pixel_aspect = image.attributes.pixel_aspect;
    Ok(frame)
}

fn read_with_image(path: &Path) -> Result<Frame> {
    let img = image::open(path)?;
    let has_alpha = img.color().has_alpha();
    if matches!(img, image::DynamicImage::ImageRgb8(_) | image::DynamicImage::ImageRgba8(_) | image::DynamicImage::ImageLuma8(_) | image::DynamicImage::ImageLumaA8(_)) {
        let rgba = img.to_rgba8();
        let mut f = Frame::new_u8(rgba.width(), rgba.height(), has_alpha);
        f.byte_mut().copy_from_slice(rgba.as_raw());
        return Ok(f);
    }
    let rgba = img.to_rgba32f();
    Ok(Frame::from_f32(rgba.width(), rgba.height(), has_alpha, rgba.as_raw()))
}

fn read_oiio(path: &Path) -> Result<Frame> {
    let p = path.to_string_lossy();
    let probe = plate_native::oiio_probe(&p)?;
    let mut f = Frame::new(probe.width, probe.height, false);
    // SAFETY: f16 is a transparent wrapper over u16.
    let half = f.half_mut();
    let bits: &mut [u16] = unsafe { std::slice::from_raw_parts_mut(half.as_mut_ptr().cast(), half.len()) };
    let info = plate_native::oiio_read_rgba_half(&p, bits, probe.width, probe.height)?;
    f.has_alpha = info.has_alpha;
    f.pixel_aspect = if info.pixel_aspect > 0.0 { info.pixel_aspect } else { 1.0 };
    Ok(f)
}
