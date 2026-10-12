//! Movies through FFmpeg: QuickTime, MP4, MXF, MKV and anything else libavformat reads.

use crate::audio::AudioSegment;
use crate::frame::Frame;
use crate::media::{self, FormatKey, FrameReader, FrameSource, MediaInfo, MediaKind};
use crate::scan::ext_of;
use anyhow::{anyhow, bail, Context, Result};
use ffmpeg_next as ff;
use ff::ffi;
use half::f16;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Once;

pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = ff::init();
        ff::log::set_level(ff::log::Level::Error);
    });
}

pub struct VideoSource {
    info: MediaInfo,
    path: PathBuf,
    intra_only: bool,
}

/// Facts about the best video stream of a file.
pub(crate) struct StreamProbe {
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f32,
    pub fps: f64,
    pub frame_count: u64,
    pub codec: String,
    pub detail: String,
    pub has_alpha: bool,
    pub has_audio: bool,
    pub timecode: Option<String>,
    pub transfer: Option<String>,
    pub intra_only: bool,
    pub depth: u32,
}

pub(crate) fn probe(path: &Path) -> Result<StreamProbe> {
    init();
    let ictx = ff::format::input(path).with_context(|| format!("FFmpeg can't open {}", path.display()))?;
    let stream = ictx
        .streams()
        .best(ff::media::Type::Video)
        .ok_or_else(|| anyhow!("no video stream in {}", path.display()))?;
    let dec = ff::codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .video()?;

    let rate = pick_rate(&stream);
    let fps = f64::from(rate);
    let tb = f64::from(stream.time_base());
    let mut frames = stream.frames().max(0) as u64;
    if frames == 0 {
        let dur = if stream.duration() > 0 {
            stream.duration() as f64 * tb
        } else {
            ictx.duration().max(0) as f64 / f64::from(ff::ffi::AV_TIME_BASE)
        };
        frames = (dur * fps).round() as u64;
    }

    let codec = dec.id().name().to_string();
    let fmt = dec.format();
    let (depth, alpha) = pix_depth_alpha(fmt);
    let profile = unsafe {
        let p = ffi::avcodec_profile_name(dec.id().into(), (*dec.as_ptr()).profile);
        if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
    };
    let fmt_name = fmt.descriptor().map(|d| d.name().to_string()).unwrap_or_default();
    let detail = if profile.is_empty() {
        format!("{codec} {fmt_name} {depth}-bit")
    } else {
        format!("{codec} ({profile}) {fmt_name}")
    };

    let sar = dec.aspect_ratio();
    let pixel_aspect = if sar.numerator() > 0 && sar.denominator() > 0 { f64::from(sar) as f32 } else { 1.0 };

    let timecode = stream
        .metadata()
        .get("timecode")
        .map(str::to_string)
        .or_else(|| ictx.metadata().get("timecode").map(str::to_string));
    let transfer = match dec.color_transfer_characteristic() {
        ff::color::TransferCharacteristic::SMPTE2084 => Some("pq".to_string()),
        ff::color::TransferCharacteristic::ARIB_STD_B67 => Some("hlg".to_string()),
        _ => None,
    };

    Ok(StreamProbe {
        width: dec.width(),
        height: dec.height(),
        pixel_aspect,
        fps,
        frame_count: frames.max(1),
        codec,
        detail,
        has_alpha: alpha,
        has_audio: ictx.streams().best(ff::media::Type::Audio).is_some(),
        timecode,
        transfer,
        intra_only: is_intra_only(dec.id()),
        depth,
    })
}

fn pick_rate(stream: &ff::format::stream::Stream) -> ff::Rational {
    let avg = stream.avg_frame_rate();
    if avg.numerator() > 0 && avg.denominator() > 0 {
        return avg;
    }
    let r = stream.rate();
    if r.numerator() > 0 && r.denominator() > 0 {
        return r;
    }
    ff::Rational::new(24, 1)
}

/// Intra-only codecs (ProRes, DNx, JPEG 2000, MJPEG) seek exactly and cheaply,
/// so they can be decoded by several readers in parallel.
pub(crate) fn is_intra_only(id: ff::codec::Id) -> bool {
    unsafe {
        let d = ffi::avcodec_descriptor_get(id.into());
        !d.is_null() && ((*d).props & ffi::AV_CODEC_PROP_INTRA_ONLY as i32) != 0
    }
}

pub(crate) fn pix_depth_alpha(fmt: ff::format::Pixel) -> (u32, bool) {
    unsafe {
        let d = ffi::av_pix_fmt_desc_get(fmt.into());
        if d.is_null() {
            return (8, false);
        }
        let depth = (*d).comp[0].depth as u32;
        let alpha = ((*d).flags & ffi::AV_PIX_FMT_FLAG_ALPHA as u64) != 0;
        (depth, alpha)
    }
}

impl VideoSource {
    pub fn open(path: &Path) -> Result<Self> {
        let p = probe(path)?;
        let ext = ext_of(path);
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let info = MediaInfo {
            id: media::next_id(),
            kind: MediaKind::Video,
            path: path.to_path_buf(),
            name,
            width: p.width,
            height: p.height,
            pixel_aspect: p.pixel_aspect,
            frame_count: p.frame_count,
            first_frame: 0,
            fps: p.fps,
            format: FormatKey::new(ext, p.codec),
            detail: p.detail,
            has_alpha: p.has_alpha,
            has_audio: p.has_audio,
            colorspace_hint: p.transfer,
            timecode: p.timecode,
            notes_path: media::file_notes_path(path),
            bytes_per_pixel: if p.depth > 8 { 8 } else { 4 },
        };
        Ok(Self { info, path: path.to_path_buf(), intra_only: p.intra_only })
    }
}

impl FrameSource for VideoSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn reader(&self) -> Result<Box<dyn FrameReader>> {
        Ok(Box::new(VideoReader::open(&self.path, 0, self.info.frame_count)?))
    }

    fn parallelism(&self) -> usize {
        if self.intra_only {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6)
        } else {
            1
        }
    }

    fn audio(&self) -> Vec<AudioSegment> {
        if !self.info.has_audio {
            return Vec::new();
        }
        vec![AudioSegment::file(self.path.clone(), 0.0, self.info.duration_secs())]
    }
}

/// A stateful decoder that serves frames by index, decoding forward when it
/// can and seeking when it must.
pub struct VideoReader {
    ictx: ff::format::context::Input,
    stream: usize,
    decoder: ff::decoder::Video,
    tb: f64,
    fps: f64,
    start_pts: i64,
    /// Index offset applied to requests (DCP entry points).
    entry: u64,
    frame_count: u64,
    /// The index the next decoded frame is expected to have.
    next_index: Option<u64>,
    eof_sent: bool,
    /// Forward-decode instead of seeking when the target is at most this far
    /// ahead. Zero for intra-only codecs, where seeking is exact and cheap.
    forward_limit: u64,
    converter: Converter,
}

// SAFETY: the reader owns all of its FFmpeg state and is only used from one
// thread at a time; FFmpeg contexts are not tied to a thread.
unsafe impl Send for VideoReader {}

/// Long-GOP forward-decode window; a seek usually costs a whole GOP anyway.
const FORWARD_DECODE_LIMIT: u64 = 96;

impl VideoReader {
    pub fn open(path: &Path, entry: u64, frame_count: u64) -> Result<Self> {
        init();
        let ictx = ff::format::input(path)?;
        let stream = ictx.streams().best(ff::media::Type::Video).ok_or_else(|| anyhow!("no video stream"))?;
        let index = stream.index();
        let tb = f64::from(stream.time_base());
        let fps = f64::from(pick_rate(&stream));
        let start_pts = if stream.start_time() == ffi::AV_NOPTS_VALUE { 0 } else { stream.start_time() };
        let mut ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
        let intra = is_intra_only(ctx.id());
        // Frame threading buffers several packets before the first output, which
        // makes single-frame seeks slow. Intra readers run in parallel instead,
        // so they use slice threading.
        ctx.set_threading(ff::codec::threading::Config {
            kind: if intra { ff::codec::threading::Type::Slice } else { ff::codec::threading::Type::Frame },
            count: if intra { 2 } else { 0 },
        });
        let decoder = ctx.decoder().video()?;
        Ok(Self {
            ictx,
            stream: index,
            decoder,
            tb,
            fps,
            start_pts,
            entry,
            frame_count,
            next_index: None,
            eof_sent: false,
            forward_limit: if intra { 0 } else { FORWARD_DECODE_LIMIT },
            converter: Converter::new()?,
        })
    }

    fn index_of(&self, frame: &ff::frame::Video) -> Option<u64> {
        let pts = frame.timestamp().or_else(|| frame.pts())?;
        let secs = (pts - self.start_pts) as f64 * self.tb;
        Some((secs * self.fps).round().max(0.0) as u64)
    }

    fn seek(&mut self, index: u64) -> Result<()> {
        let secs = index as f64 / self.fps + self.start_pts as f64 * self.tb;
        let ts = (secs * f64::from(ffi::AV_TIME_BASE)) as i64;
        self.ictx.seek(ts, ..ts + 1)?;
        self.decoder.flush();
        self.next_index = None;
        self.eof_sent = false;
        Ok(())
    }

    fn decode_next(&mut self) -> Result<Option<ff::frame::Video>> {
        let mut frame = ff::frame::Video::empty();
        loop {
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => return Ok(Some(frame)),
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == ff::util::error::EAGAIN => {}
                Err(e) => return Err(e.into()),
            }
            if self.eof_sent {
                return Ok(None);
            }
            let mut sent = false;
            while let Some((stream, packet)) = self.ictx.packets().next() {
                if stream.index() == self.stream {
                    // Corrupt packets are skipped rather than aborting the read.
                    let _ = self.decoder.send_packet(&packet);
                    sent = true;
                    break;
                }
            }
            if !sent {
                self.decoder.send_eof()?;
                self.eof_sent = true;
            }
        }
    }

    fn read_index(&mut self, target: u64) -> Result<Frame> {
        let needs_seek = match self.next_index {
            Some(next) => target < next || target > next + self.forward_limit,
            None => true,
        };
        if needs_seek {
            self.seek(target)?;
        }
        let mut last: Option<ff::frame::Video> = None;
        loop {
            match self.decode_next()? {
                Some(mut f) => {
                    let idx = self.index_of(&f).or(self.next_index).unwrap_or(0);
                    self.next_index = Some(idx + 1);
                    if idx >= target {
                        return self.converter.to_frame(&mut f);
                    }
                    last = Some(f);
                }
                None => {
                    // Past the real end (container frame counts can overstate):
                    // hold the last decoded frame.
                    self.next_index = None;
                    return match last {
                        Some(mut f) => self.converter.to_frame(&mut f),
                        None => bail!("frame {target} is past the end of the stream"),
                    };
                }
            }
        }
    }
}

impl FrameReader for VideoReader {
    fn read(&mut self, index: u64) -> Result<Frame> {
        let index = index.min(self.frame_count.saturating_sub(1));
        self.read_index(index + self.entry)
    }
}

/// Converts decoded frames to half-float RGBA.
pub(crate) struct Converter {
    ctx: *mut ffi::SwsContext,
}

impl Converter {
    pub fn new() -> Result<Self> {
        let ctx = unsafe { ffi::sws_alloc_context() };
        if ctx.is_null() {
            bail!("sws_alloc_context failed");
        }
        unsafe {
            // Full chroma interpolation for clean edges on 4:2:0/4:2:2;
            // accurate rounding doubles the cost for at most one code value.
            (*ctx).threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as i32;
            (*ctx).flags = ffi::SwsFlags::SWS_BICUBIC as u32 | ffi::SwsFlags::SWS_FULL_CHR_H_INT as u32;
        }
        Ok(Self { ctx })
    }

    pub fn to_frame(&mut self, src: &mut ff::frame::Video) -> Result<Frame> {
        if src.format() == ff::format::Pixel::XYZ12LE || src.format() == ff::format::Pixel::XYZ12BE {
            return Ok(xyz12_to_frame(src));
        }
        let (depth, alpha) = pix_depth_alpha(src.format());
        let wide = depth > 8;
        let out_fmt = if wide { ff::format::Pixel::RGBA64LE } else { ff::format::Pixel::RGBA };
        let (w, h) = (src.width(), src.height());

        unsafe {
            let s = src.as_mut_ptr();
            // Untagged YUV: HD and up is Rec.709, SD is Rec.601, same as players do.
            if (*s).colorspace == ffi::AVColorSpace::AVCOL_SPC_UNSPECIFIED {
                (*s).colorspace = if h >= 720 {
                    ffi::AVColorSpace::AVCOL_SPC_BT709
                } else {
                    ffi::AVColorSpace::AVCOL_SPC_SMPTE170M
                };
            }
        }
        let mut dst = ff::frame::Video::empty();
        unsafe {
            let d = dst.as_mut_ptr();
            (*d).format = ffi::AVPixelFormat::from(out_fmt) as i32;
            (*d).width = w as i32;
            (*d).height = h as i32;
            (*d).color_range = ffi::AVColorRange::AVCOL_RANGE_JPEG;
            (*d).colorspace = ffi::AVColorSpace::AVCOL_SPC_RGB;
            let r = ffi::sws_scale_frame(self.ctx, d, src.as_ptr());
            if r < 0 {
                bail!("swscale failed: {}", ff::Error::from(r));
            }
        }

        let stride = dst.stride(0);
        let data = dst.data(0);
        let row_len = w as usize * 4;
        let frame = if wide {
            let mut frame = Frame::new(w, h, alpha);
            frame.half_mut().par_chunks_mut(row_len).enumerate().for_each(|(y, out)| {
                let row = &data[y * stride..];
                for (i, o) in out.iter_mut().enumerate() {
                    let v = u16::from_le_bytes([row[i * 2], row[i * 2 + 1]]);
                    *o = f16::from_f32(v as f32 / 65535.0);
                }
            });
            frame
        } else {
            let mut frame = Frame::new_u8(w, h, alpha);
            frame.byte_mut().par_chunks_mut(row_len).enumerate().for_each(|(y, out)| {
                out.copy_from_slice(&data[y * stride..y * stride + row_len]);
            });
            frame
        };
        Ok(frame)
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        unsafe { ffi::sws_free_context(&mut self.ctx) };
    }
}

/// DCDM X'Y'Z' (12-bit, gamma 2.6, 48 cd/m² reference white at 1.0) to
/// display-linear Rec.709 primaries. No chromatic adaptation: this mirrors
/// what a projector shows.
fn xyz12_to_frame(src: &ff::frame::Video) -> Frame {
    const XYZ_TO_709: [[f32; 3]; 3] = [
        [3.2409699, -1.5373832, -0.4986108],
        [-0.9692436, 1.8759675, 0.0415551],
        [0.0556301, -0.2039770, 1.0569972],
    ];
    const NORM: f32 = 52.37 / 48.0;
    let lut: Vec<f32> = (0..4096).map(|v| (v as f32 / 4095.0).powf(2.6) * NORM).collect();

    let (w, h) = (src.width(), src.height());
    let big_endian = src.format() == ff::format::Pixel::XYZ12BE;
    let stride = src.stride(0);
    let data = src.data(0);
    let mut frame = Frame::new(w, h, false);
    frame.half_mut().par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, out)| {
        let row = &data[y * stride..];
        for x in 0..w as usize {
            let mut xyz = [0f32; 3];
            for c in 0..3 {
                let b = [row[(x * 3 + c) * 2], row[(x * 3 + c) * 2 + 1]];
                let v = if big_endian { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) };
                xyz[c] = lut[(v >> 4) as usize];
            }
            for c in 0..3 {
                let m = XYZ_TO_709[c];
                out[x * 4 + c] = f16::from_f32(m[0] * xyz[0] + m[1] * xyz[1] + m[2] * xyz[2]);
            }
            out[x * 4 + 3] = f16::ONE;
        }
    });
    frame
}
