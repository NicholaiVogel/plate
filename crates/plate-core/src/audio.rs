//! Audio playback. The output device's consumed-sample count is the master
//! clock while audio plays, so picture follows sound rather than drifting.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use ffmpeg_next as ff;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A stretch of audio on the clip timeline. Segments play back to back.
#[derive(Debug, Clone)]
pub struct AudioSegment {
    /// None means silence.
    pub path: Option<PathBuf>,
    /// Where in the file this segment starts, in seconds.
    pub source_offset: f64,
    pub duration: f64,
}

impl AudioSegment {
    pub fn file(path: PathBuf, source_offset: f64, duration: f64) -> Self {
        Self { path: Some(path), source_offset, duration }
    }

    pub fn silence(duration: f64) -> Self {
        Self { path: None, source_offset: 0.0, duration }
    }
}

enum Command {
    SetTrack(Vec<AudioSegment>),
    Play(f64),
    Pause,
}

struct Shared {
    buffer: Mutex<VecDeque<f32>>,
    playing: AtomicBool,
    /// Output frames consumed since the last Play.
    consumed: AtomicU64,
    /// Bumped on every Play/Pause so stale decoded audio is discarded.
    generation: AtomicU64,
    volume: AtomicU32,
    muted: AtomicBool,
    start_secs: Mutex<f64>,
    rate: AtomicU32,
}

/// Owns the output stream and a decode thread.
pub struct AudioPlayer {
    tx: Sender<Command>,
    shared: Arc<Shared>,
    has_track: AtomicBool,
}

const BUFFER_SECS: f64 = 0.35;

impl AudioPlayer {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared {
            buffer: Mutex::new(VecDeque::new()),
            playing: AtomicBool::new(false),
            consumed: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            volume: AtomicU32::new(1.0f32.to_bits()),
            muted: AtomicBool::new(false),
            start_secs: Mutex::new(0.0),
            rate: AtomicU32::new(48_000),
        });
        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("plate-audio".into())
            .spawn(move || {
                if let Err(e) = audio_thread(rx, thread_shared) {
                    log::warn!("audio disabled: {e}");
                }
            })
            .expect("spawn audio thread");
        Self { tx, shared, has_track: AtomicBool::new(false) }
    }

    pub fn set_track(&self, segments: Vec<AudioSegment>) {
        let has = segments.iter().any(|s| s.path.is_some());
        self.has_track.store(has, Ordering::Relaxed);
        let _ = self.tx.send(Command::SetTrack(segments));
    }

    pub fn has_track(&self) -> bool {
        self.has_track.load(Ordering::Relaxed)
    }

    pub fn play_from(&self, secs: f64) {
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.playing.store(false, Ordering::SeqCst);
        self.shared.buffer.lock().clear();
        self.shared.consumed.store(0, Ordering::SeqCst);
        *self.shared.start_secs.lock() = secs;
        let _ = self.tx.send(Command::Play(secs));
    }

    pub fn pause(&self) {
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.playing.store(false, Ordering::SeqCst);
        self.shared.buffer.lock().clear();
        let _ = self.tx.send(Command::Pause);
    }

    /// Current playback position in clip seconds, while playing.
    pub fn position(&self) -> Option<f64> {
        if !self.shared.playing.load(Ordering::Acquire) {
            return None;
        }
        let rate = self.shared.rate.load(Ordering::Relaxed) as f64;
        let consumed = self.shared.consumed.load(Ordering::Acquire) as f64;
        Some(*self.shared.start_secs.lock() + consumed / rate)
    }

    /// True once the buffer has been primed after `play_from`.
    pub fn is_running(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    pub fn set_muted(&self, m: bool) {
        self.shared.muted.store(m, Ordering::Relaxed);
    }

    pub fn muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

fn build_stream(shared: Arc<Shared>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
    let supported = device.default_output_config()?;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    shared.rate.store(config.sample_rate as u32, Ordering::Relaxed);

    fn make<T: cpal::SizedSample + cpal::FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        channels: usize,
        shared: Arc<Shared>,
    ) -> Result<cpal::Stream> {
        let stream = device.build_output_stream(
            config.clone(),
            move |out: &mut [T], _| {
                let playing = shared.playing.load(Ordering::Acquire);
                let gain = if shared.muted.load(Ordering::Relaxed) {
                    0.0
                } else {
                    f32::from_bits(shared.volume.load(Ordering::Relaxed))
                };
                let mut buf = shared.buffer.lock();
                let mut consumed = 0u64;
                for frame in out.chunks_mut(channels) {
                    let (l, r) = if playing && buf.len() >= 2 {
                        consumed += 1;
                        (buf.pop_front().unwrap(), buf.pop_front().unwrap())
                    } else {
                        (0.0, 0.0)
                    };
                    for (c, s) in frame.iter_mut().enumerate() {
                        let v = match c {
                            0 => l,
                            1 => r,
                            _ => 0.0,
                        };
                        *s = T::from_sample(v * gain);
                    }
                }
                drop(buf);
                if playing {
                    shared.consumed.fetch_add(consumed, Ordering::AcqRel);
                }
            },
            |e| log::warn!("audio stream error: {e}"),
            None,
        )?;
        Ok(stream)
    }

    let stream = match format {
        cpal::SampleFormat::F32 => make::<f32>(&device, &config, channels, shared)?,
        cpal::SampleFormat::I16 => make::<i16>(&device, &config, channels, shared)?,
        cpal::SampleFormat::I32 => make::<i32>(&device, &config, channels, shared)?,
        cpal::SampleFormat::U16 => make::<u16>(&device, &config, channels, shared)?,
        other => return Err(anyhow!("unsupported output sample format {other:?}")),
    };
    stream.play()?;
    Ok(stream)
}

/// Decodes one segment from a position, producing interleaved stereo f32.
struct SegmentDecoder {
    ictx: ff::format::context::Input,
    stream: usize,
    decoder: ff::decoder::Audio,
    resampler: Option<ff::software::resampling::Context>,
    out_rate: u32,
    tb: f64,
    /// Samples (per channel) still to discard to land exactly on the target.
    skip_until: f64,
    /// Seconds of output left in this segment.
    remaining: f64,
    eof: bool,
}

// SAFETY: owned exclusively by the audio thread.
unsafe impl Send for SegmentDecoder {}

impl SegmentDecoder {
    fn open(path: &PathBuf, source_secs: f64, duration: f64, out_rate: u32) -> Result<Self> {
        crate::source::video::init();
        let mut ictx = ff::format::input(path)?;
        let stream = ictx.streams().best(ff::media::Type::Audio).ok_or_else(|| anyhow!("no audio stream"))?;
        let index = stream.index();
        let tb = f64::from(stream.time_base());
        let decoder = ff::codec::context::Context::from_parameters(stream.parameters())?.decoder().audio()?;
        if source_secs > 0.0 {
            let ts = (source_secs * f64::from(ff::ffi::AV_TIME_BASE)) as i64;
            let _ = ictx.seek(ts, ..ts + 1);
        }
        Ok(Self {
            ictx,
            stream: index,
            decoder,
            resampler: None,
            out_rate,
            tb,
            skip_until: source_secs,
            remaining: duration,
            eof: false,
        })
    }

    /// Appends up to roughly one packet of audio. Returns false at the end.
    fn pull(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        if self.remaining <= 0.0 {
            return Ok(false);
        }
        let mut frame = ff::frame::Audio::empty();
        loop {
            if self.decoder.receive_frame(&mut frame).is_ok() {
                break;
            }
            if self.eof {
                return Ok(false);
            }
            let mut sent = false;
            while let Some((s, packet)) = self.ictx.packets().next() {
                if s.index() == self.stream {
                    let _ = self.decoder.send_packet(&packet);
                    sent = true;
                    break;
                }
            }
            if !sent {
                let _ = self.decoder.send_eof();
                self.eof = true;
            }
        }

        if frame.channel_layout().channels() == 0 || frame.channel_layout().is_empty() {
            frame.set_channel_layout(ff::ChannelLayout::default(frame.channels() as i32));
        }
        if self.resampler.is_none() {
            self.resampler = Some(ff::software::resampling::Context::get(
                frame.format(),
                frame.channel_layout(),
                frame.rate(),
                ff::format::Sample::F32(ff::format::sample::Type::Packed),
                ff::ChannelLayout::STEREO,
                self.out_rate,
            )?);
        }
        let frame_start = frame.timestamp().map(|t| t as f64 * self.tb);
        let mut resampled = ff::frame::Audio::empty();
        self.resampler.as_mut().unwrap().run(&frame, &mut resampled)?;
        let n = resampled.samples();
        let bytes = &resampled.data(0)[..n * 2 * 4];
        let mut samples: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();

        // After a keyframe seek the first frames start before the target.
        if let Some(start) = frame_start {
            if start < self.skip_until {
                let drop = (((self.skip_until - start) * self.out_rate as f64) as usize * 2).min(samples.len());
                samples.drain(..drop);
            }
        }
        self.skip_until = f64::MIN;

        let max = (self.remaining * self.out_rate as f64) as usize * 2;
        if samples.len() > max {
            samples.truncate(max);
        }
        self.remaining -= samples.len() as f64 / 2.0 / self.out_rate as f64;
        out.extend_from_slice(&samples);
        Ok(true)
    }
}

/// Plays the track from `secs`: yields decoders segment by segment.
struct TrackCursor {
    segments: Vec<AudioSegment>,
    index: usize,
    decoder: Option<SegmentDecoder>,
    silence_left: f64,
    out_rate: u32,
}

impl TrackCursor {
    fn new(segments: Vec<AudioSegment>, secs: f64, out_rate: u32) -> Self {
        let mut t = secs.max(0.0);
        let mut index = segments.len();
        let mut offset = 0.0;
        for (i, s) in segments.iter().enumerate() {
            if t < s.duration {
                index = i;
                offset = t;
                break;
            }
            t -= s.duration;
        }
        let mut c = Self { segments, index, decoder: None, silence_left: 0.0, out_rate };
        c.enter(offset);
        c
    }

    fn enter(&mut self, offset: f64) {
        self.decoder = None;
        self.silence_left = 0.0;
        let Some(seg) = self.segments.get(self.index) else { return };
        let left = seg.duration - offset;
        match &seg.path {
            Some(p) => match SegmentDecoder::open(p, seg.source_offset + offset, left, self.out_rate) {
                Ok(d) => self.decoder = Some(d),
                Err(e) => {
                    log::warn!("audio segment {}: {e}", p.display());
                    self.silence_left = left;
                }
            },
            None => self.silence_left = left,
        }
    }

    fn pull(&mut self, out: &mut Vec<f32>) -> bool {
        loop {
            if self.index >= self.segments.len() {
                return false;
            }
            if self.silence_left > 0.0 {
                let chunk = self.silence_left.min(0.05);
                out.extend(std::iter::repeat_n(0.0, (chunk * self.out_rate as f64) as usize * 2));
                self.silence_left -= chunk;
                return true;
            }
            if let Some(d) = self.decoder.as_mut() {
                match d.pull(out) {
                    Ok(true) => return true,
                    Ok(false) => {}
                    Err(e) => log::warn!("audio decode: {e}"),
                }
            }
            self.index += 1;
            self.enter(0.0);
        }
    }
}

fn audio_thread(rx: Receiver<Command>, shared: Arc<Shared>) -> Result<()> {
    let mut stream: Option<cpal::Stream> = None;
    let mut segments: Vec<AudioSegment> = Vec::new();
    let mut cursor: Option<(u64, TrackCursor)> = None;
    let mut scratch = Vec::new();

    loop {
        let timeout = if cursor.is_some() { Duration::from_millis(5) } else { Duration::from_secs(3600) };
        match rx.recv_timeout(timeout) {
            Ok(Command::SetTrack(s)) => {
                segments = s;
                cursor = None;
            }
            Ok(Command::Play(secs)) => {
                if stream.is_none() && segments.iter().any(|s| s.path.is_some()) {
                    match build_stream(shared.clone()) {
                        Ok(s) => stream = Some(s),
                        Err(e) => log::warn!("audio output unavailable: {e}"),
                    }
                }
                if stream.is_some() && segments.iter().any(|s| s.path.is_some()) {
                    let rate = shared.rate.load(Ordering::Relaxed);
                    let generation = shared.generation.load(Ordering::SeqCst);
                    cursor = Some((generation, TrackCursor::new(segments.clone(), secs, rate)));
                }
            }
            Ok(Command::Pause) => cursor = None,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
        }

        let Some((generation, cur)) = cursor.as_mut() else { continue };
        if *generation != shared.generation.load(Ordering::SeqCst) {
            continue;
        }
        let rate = shared.rate.load(Ordering::Relaxed) as f64;
        let target = (BUFFER_SECS * rate) as usize * 2;
        while shared.buffer.lock().len() < target {
            scratch.clear();
            if !cur.pull(&mut scratch) {
                break;
            }
            if *generation != shared.generation.load(Ordering::SeqCst) {
                break;
            }
            shared.buffer.lock().extend(scratch.iter().copied());
        }
        // Start the clock only once there is something to play, so the first
        // video frame and the first sample line up.
        if *generation == shared.generation.load(Ordering::SeqCst) && !shared.playing.load(Ordering::Acquire) {
            shared.consumed.store(0, Ordering::SeqCst);
            shared.playing.store(true, Ordering::Release);
        }
    }
}
