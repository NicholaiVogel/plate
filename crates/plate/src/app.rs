//! Application state and behaviour. Rendering lives in `ui/`.

use crate::text_input::{InputEvent, TextInput};
use futures::StreamExt;
use gpui::{
    point, px, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, PathPromptOptions, Pixels,
    Point, RenderImage, Window,
};
use plate_core::{
    audio::AudioPlayer,
    cache::FrameCache,
    color::ColorManager,
    display::{self, Channel},
    frame::Frame,
    loader::{FrameStatus, Loader},
    lut::DisplayLut,
    media::{self, MediaInfo, MediaKind, Source},
    notes::{Comment, Notes, Shape},
    prefs::{ColorChoice, Prefs},
    scan::{self, BinEntry, EntryKind},
};
use smallvec::SmallVec;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Clip {
    pub source: Source,
    pub info: MediaInfo,
    pub loader: Loader,
    pub notes: Notes,
    pub choice: ColorChoice,
    /// True when `choice` came from the per-format memory.
    pub remembered: bool,
    pub in_point: Option<u64>,
    pub out_point: Option<u64>,
}

impl Clip {
    pub fn range(&self) -> (u64, u64) {
        let last = self.info.frame_count.saturating_sub(1);
        let a = self.in_point.unwrap_or(0).min(last);
        let b = self.out_point.unwrap_or(last).min(last).max(a);
        (a, b)
    }

    pub fn frame_label(&self, index: u64) -> i64 {
        self.info.first_frame + index as i64
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pen,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Text,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 7] = [Tool::Pen, Tool::Arrow, Tool::Line, Tool::Rect, Tool::Ellipse, Tool::Text, Tool::Eraser];

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Pen => "scribble",
            Tool::Arrow => "arrow",
            Tool::Line => "line",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Text => "text",
            Tool::Eraser => "eraser",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Pen => "Pen",
            Tool::Arrow => "Arrow",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
            Tool::Eraser => "Eraser",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tool::Pen => "1",
            Tool::Arrow => "2",
            Tool::Line => "3",
            Tool::Rect => "4",
            Tool::Ellipse => "5",
            Tool::Text => "6",
            Tool::Eraser => "0",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modal {
    Colorspace,
    Display,
    Channel,
    Exposure,
    Compare,
    Settings,
    Help,
    TextAnnotation,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareMode {
    Wipe,
    SideBySide,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CounterMode {
    Frames,
    Timecode,
    Index,
}

/// What the viewer shows for one slot (A or B).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PresentKey {
    pub clip: u64,
    pub frame: u64,
    pub lut: usize,
    pub channel: Channel,
}

#[derive(Default)]
pub struct Presenter {
    ready: HashMap<PresentKey, Arc<RenderImage>>,
    order: VecDeque<PresentKey>,
    pending: HashSet<PresentKey>,
    /// Last image actually painted per slot, kept while a newer one renders.
    pub shown: [Option<(PresentKey, Arc<RenderImage>)>; 2],
    pub to_drop: Vec<Arc<RenderImage>>,
}

const PRESENT_CAPACITY: usize = 12;

impl Presenter {
    fn insert(&mut self, key: PresentKey, image: Arc<RenderImage>) {
        self.pending.remove(&key);
        if self.ready.insert(key.clone(), image).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > PRESENT_CAPACITY {
            if let Some(old) = self.order.pop_front() {
                if let Some(img) = self.ready.remove(&old) {
                    let in_use = self.shown.iter().flatten().any(|(_, s)| Arc::ptr_eq(s, &img));
                    if !in_use {
                        self.to_drop.push(img);
                    }
                }
            }
        }
    }

    pub fn get(&self, key: &PresentKey) -> Option<Arc<RenderImage>> {
        self.ready.get(key).cloned()
    }

    /// Releases every cached GPU image (clip closed or color settings
    /// changed). What's on screen stays until its replacement is ready, so
    /// stepping exposure doesn't flash.
    pub fn clear(&mut self) {
        let shown: Vec<_> = self.shown.iter().flatten().map(|(_, i)| i.clone()).collect();
        self.to_drop.extend(self.ready.drain().map(|(_, v)| v).filter(|v| !shown.iter().any(|s| Arc::ptr_eq(s, v))));
        self.order.clear();
        self.pending.clear();
    }

    /// Records what a slot now shows, releasing the previous image if
    /// nothing else holds it.
    pub fn show(&mut self, slot: usize, key: PresentKey, image: Arc<RenderImage>) {
        if let Some((_, old)) = self.shown[slot].replace((key, image)) {
            let cached = self.ready.values().any(|v| Arc::ptr_eq(v, &old));
            let current = self.shown[slot].as_ref().is_some_and(|(_, n)| Arc::ptr_eq(n, &old));
            let other = self.shown[1 - slot].as_ref().is_some_and(|(_, n)| Arc::ptr_eq(n, &old));
            if !cached && !current && !other {
                self.to_drop.push(old);
            }
        }
    }
}

pub struct Playback {
    /// 1 forward, -1 reverse, 0 stopped.
    pub dir: i32,
    pub speed: f32,
    anchor_frame: f64,
    anchor: Instant,
    pub audio_clock: bool,
    /// Last audio position seen and when it last moved, to detect a stalled
    /// or missing audio clock.
    audio_seen: Option<f64>,
    audio_moved: Instant,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            dir: 0,
            speed: 1.0,
            anchor_frame: 0.0,
            anchor: Instant::now(),
            audio_clock: false,
            audio_seen: None,
            audio_moved: Instant::now(),
        }
    }
}

/// How long the audio clock may take to start before picture carries on with
/// the wall clock. Once running, a stall of a few frames is enough.
const AUDIO_START_GRACE: Duration = Duration::from_millis(400);

pub enum Drag {
    None,
    Pan { start: Point<Pixels>, pan: Point<Pixels> },
    Draw,
    Wipe,
    Scrub,
    /// A left click on the image that may turn into a pan.
    Click { start: Point<Pixels>, pan: Point<Pixels> },
}

pub struct Probe {
    pub pos: Point<Pixels>,
    pub pixel: (i64, i64),
    pub raw: Option<[f32; 4]>,
    pub shown: Option<[f32; 3]>,
}

pub struct Bin {
    pub dir: Option<PathBuf>,
    pub title: String,
    pub entries: Vec<BinEntry>,
}

pub struct Toast {
    pub text: String,
    pub until: Instant,
}

pub struct Plate {
    pub prefs: Prefs,
    pub color: Arc<ColorManager>,
    pub cache: Arc<FrameCache>,
    pub audio: AudioPlayer,
    pub viewer_focus: FocusHandle,

    pub clip: Option<Clip>,
    pub compare: Option<Clip>,
    pub compare_mode: CompareMode,
    pub wipe: f32,
    pub bin: Option<Bin>,

    pub frame: u64,
    pub playback: Playback,
    pub counter: CounterMode,

    pub channel: Channel,
    pub exposure: f32,
    pub gamma: f32,
    pub zoom: Option<f32>,
    pub pan: Point<Pixels>,

    pub drawing: bool,
    pub tool: Tool,
    pub draft: Vec<Shape>,
    pub draft_frame: Option<u64>,
    pub in_progress: Option<Shape>,
    pub text_anchor: Option<[f32; 2]>,
    pub focused_comment: Option<String>,
    pub probe_on: bool,
    pub probe: Option<Probe>,

    pub modal: Option<Modal>,
    pub drag: Drag,
    pub comment_input: Entity<TextInput>,
    pub search_input: Entity<TextInput>,
    pub annotation_input: Entity<TextInput>,
    pub toast: Option<Toast>,

    pub presenter: Presenter,
    /// Viewer and timeline geometry from the last paint, for mouse mapping.
    pub viewer_bounds: Rc<Cell<Bounds<Pixels>>>,
    stats: Option<(Instant, u32, u64)>,
    bin_watcher: Option<notify::RecommendedWatcher>,
    pub window_title: String,
    pub timeline_bounds: Rc<Cell<Bounds<Pixels>>>,
    frame_ready_tx: futures::channel::mpsc::UnboundedSender<()>,
}

const MAX_ZOOM: f32 = 32.0;
const MIN_ZOOM: f32 = 0.02;

impl Plate {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Prefs::load();
        crate::theme::set_ui_scale(prefs.ui_scale);
        let (color, warning) = ColorManager::load(&prefs.ocio);
        let cache = Arc::new(FrameCache::new((prefs.cache_gb as f64 * 1024.0 * 1024.0 * 1024.0) as usize));
        let audio = AudioPlayer::new();
        audio.set_volume(prefs.volume);
        audio.set_muted(prefs.muted);

        let comment_input = cx.new(|cx| TextInput::new(cx, "Leave your comment…"));
        let search_input = cx.new(|cx| TextInput::new(cx, "Search colorspaces"));
        let annotation_input = cx.new(|cx| TextInput::new(cx, "Annotation text"));
        cx.subscribe_in(&comment_input, window, Self::on_comment_input).detach();
        cx.subscribe_in(&search_input, window, Self::on_search_input).detach();
        cx.subscribe_in(&annotation_input, window, Self::on_annotation_input).detach();

        // Decode workers ping this channel; the UI repaints on the main thread.
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
        cx.spawn(async move |this, cx| {
            while rx.next().await.is_some() {
                // Coalesce bursts of arrivals into one repaint.
                while let Ok(()) = rx.try_recv() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        let viewer_focus = cx.focus_handle();
        window.focus(&viewer_focus);

        // The disk cache folder belongs to this session only.
        cx.on_app_quit(|this, _| {
            this.cache.set_disk(None);
            async {}
        })
        .detach();

        let mut app = Self {
            channel: Channel::Rgb,
            exposure: 0.0,
            gamma: 1.0,
            zoom: None,
            pan: point(px(0.), px(0.)),
            prefs,
            color: Arc::new(color),
            cache,
            audio,
            viewer_focus,
            clip: None,
            compare: None,
            compare_mode: CompareMode::Wipe,
            wipe: 0.5,
            bin: None,
            frame: 0,
            playback: Playback::default(),
            counter: CounterMode::Frames,
            drawing: false,
            tool: Tool::Pen,
            draft: Vec::new(),
            draft_frame: None,
            in_progress: None,
            text_anchor: None,
            focused_comment: None,
            probe_on: false,
            probe: None,
            modal: None,
            drag: Drag::None,
            comment_input,
            search_input,
            annotation_input,
            toast: None,
            presenter: Presenter::default(),
            viewer_bounds: Rc::new(Cell::new(Bounds::default())),
            window_title: String::new(),
            bin_watcher: None,
            stats: std::env::var("PLATE_STATS").is_ok().then(|| (Instant::now(), 0, u64::MAX)),
            timeline_bounds: Rc::new(Cell::new(Bounds::default())),
            frame_ready_tx: tx,
        };
        if let Some(w) = warning {
            app.show_toast(w);
        }
        app.apply_disk_cache();
        app
    }

    /// Starts, moves or stops the disk cache to match prefs. Any previous
    /// session folder is deleted.
    pub fn apply_disk_cache(&mut self) {
        self.cache.set_disk(None);
        if !self.prefs.disk_cache {
            return;
        }
        let location = self.prefs.disk_cache_location();
        let budget = (self.prefs.disk_cache_gb as f64 * 1024.0 * 1024.0 * 1024.0) as usize;
        match plate_core::disk::DiskTier::new(&location, budget) {
            Ok(tier) => self.cache.set_disk(Some(tier)),
            Err(e) => self.show_toast(format!("Disk cache unavailable at {}: {e}", location.display())),
        }
    }

    // ---------------------------------------------------------------- opening

    pub fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let path = canonical.as_path();
        if path.is_dir() && !scan::is_dcp_dir(path) {
            self.open_folder(path, cx);
            return;
        }
        // Opening a file also lists its folder in the bin, so neighbours are a
        // keypress away.
        if let Some(parent) = path.parent() {
            if self.bin.as_ref().and_then(|b| b.dir.as_deref()) != Some(parent) && !scan::is_dcp_dir(path) {
                if let Ok(entries) = scan::scan_dir(parent) {
                    self.set_bin(Bin { dir: Some(parent.to_path_buf()), title: dir_title(parent), entries }, cx);
                }
            }
        }
        match media::open(path) {
            Ok(source) => self.set_clip(source, cx),
            Err(e) => self.show_toast(format!("{e:#}")),
        }
    }

    pub fn open_entry(&mut self, entry: &BinEntry, cx: &mut Context<Self>) {
        if matches!(entry.kind, EntryKind::Directory) {
            self.open_folder(&entry.path.clone(), cx);
            return;
        }
        match media::open_entry(entry) {
            Ok(source) => self.set_clip(source, cx),
            Err(e) => self.show_toast(format!("{e:#}")),
        }
    }

    pub fn open_folder(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let dir = canonical.as_path();
        match scan::scan_dir(dir) {
            Ok(entries) => {
                let first = entries.iter().find(|e| e.is_playable()).cloned();
                self.set_bin(Bin { dir: Some(dir.to_path_buf()), title: dir_title(dir), entries }, cx);
                self.prefs.show_bin = true;
                if let Some(e) = first {
                    self.open_entry(&e, cx);
                }
                self.prefs.push_recent(&dir.to_string_lossy());
                self.prefs.save();
                cx.notify();
            }
            Err(e) => self.show_toast(format!("{}: {e}", dir.display())),
        }
    }

    /// Several paths from the command line or a drop: one folder opens as a
    /// bin; several files become an ad-hoc bin.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        match paths.len() {
            0 => {}
            1 => self.open_path(&paths[0], cx),
            _ => {
                let entries: Vec<BinEntry> = paths
                    .iter()
                    .filter_map(|p| {
                        let name = p.file_name()?.to_string_lossy().to_string();
                        let kind = if scan::is_dcp_dir(p) {
                            EntryKind::Dcp
                        } else if p.is_dir() {
                            EntryKind::Directory
                        } else if let Some(seq) = scan::is_image(p).then(|| scan::sequence_for(p)).flatten() {
                            return Some(BinEntry { name: seq.display_name(), path: p.clone(), kind: EntryKind::Sequence(seq) });
                        } else if scan::is_image(p) {
                            EntryKind::Still
                        } else {
                            EntryKind::Video
                        };
                        Some(BinEntry { name, path: p.clone(), kind })
                    })
                    .collect();
                let first = entries.iter().find(|e| e.is_playable()).cloned();
                self.set_bin(Bin { dir: None, title: format!("{} items", entries.len()), entries }, cx);
                self.prefs.show_bin = true;
                if let Some(e) = first {
                    self.open_entry(&e, cx);
                }
            }
        }
    }

    /// Shows a bin and watches its folder so it tracks the disk live.
    pub fn set_bin(&mut self, bin: Bin, cx: &mut Context<Self>) {
        self.bin_watcher = None;
        if let Some(dir) = bin.dir.clone() {
            self.bin_watcher = watch_dir(&dir, cx);
        }
        self.bin = Some(bin);
    }

    /// Rescans the bin folder after a change on disk. A sequence that is
    /// open and has grown (a render in progress) is reopened at the same
    /// frame, and frames that were missing are retried.
    pub fn refresh_bin(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.bin.as_ref().and_then(|b| b.dir.clone()) else { return };
        log::debug!("rescanning {}", dir.display());
        let entries = match scan::scan_dir(&dir) {
            Ok(e) => e,
            // The folder itself went away; keep what we have.
            Err(_) => return,
        };
        if let Some(clip) = &self.clip {
            clip.loader.retry_failed();
            let current = clip.info.path.clone();
            let grown = entries.iter().find_map(|e| match &e.kind {
                EntryKind::Sequence(seq) if entry_matches(e, &current) => {
                    let changed = clip.info.first_frame != seq.first || clip.info.frame_count != seq.len();
                    changed.then(|| seq.clone())
                }
                _ => None,
            });
            if let Some(seq) = grown {
                self.swap_sequence(seq);
            }
        }
        if let Some(bin) = self.bin.as_mut() {
            bin.entries = entries;
        }
        cx.notify();
    }

    /// Replaces the open sequence with a re-scanned one in place: same frame,
    /// range, color, notes and playback. When only the end moved the cache
    /// is kept, since every index still means the same frame.
    fn swap_sequence(&mut self, seq: scan::Sequence) {
        let Some(old) = self.clip.take() else { return };
        let same_start = old.info.first_frame == seq.first;
        let opened = if same_start { media::reopen_sequence(seq.clone(), old.info.id) } else { media::open_sequence(seq.clone()) };
        let source = match opened {
            Ok(s) => s,
            Err(e) => {
                self.clip = Some(old);
                log::warn!("rescanned sequence won't open: {e:#}");
                return;
            }
        };
        let to_index = |label: i64| (label - seq.first).clamp(0, seq.len() as i64 - 1) as u64;
        let frame = to_index(old.frame_label(self.frame));
        let in_point = old.in_point.map(|f| to_index(old.frame_label(f)));
        let out_point = old.out_point.map(|f| to_index(old.frame_label(f)));
        let tx = self.frame_ready_tx.clone();
        let loader = Loader::new(source.clone(), self.cache.clone(), move |_| {
            let _ = tx.unbounded_send(());
        });
        if !same_start {
            self.cache.remove_source(old.info.id);
            self.presenter.clear();
        }
        let info = source.info().clone();
        let Clip { notes, choice, remembered, .. } = old;
        self.clip = Some(Clip { source, info, loader, notes, choice, remembered, in_point, out_point });
        self.frame = frame;
        self.reconfigure_loaders();
        self.sync_loaders();
    }

    fn make_clip(&mut self, source: Source) -> Clip {
        let info = source.info().clone();
        let key = self.color.memory_key();
        let (choice, remembered) = match self.prefs.remembered(&key, &info.format.id()) {
            Some(c) if self.color.validate(c) => (c.clone(), true),
            _ => (self.color.default_choice(&info), false),
        };
        let tx = self.frame_ready_tx.clone();
        let loader = Loader::new(source.clone(), self.cache.clone(), move |_| {
            let _ = tx.unbounded_send(());
        });
        let notes = Notes::load(&info);
        let mut clip = Clip { source, info, loader, notes, choice, remembered, in_point: None, out_point: None };
        self.configure_loader(&mut clip, 1);
        clip
    }

    fn set_clip(&mut self, source: Source, cx: &mut Context<Self>) {
        self.stop();
        if let Some(old) = self.clip.take() {
            self.cache.remove_source(old.info.id);
        }
        self.presenter.clear();
        let clip = self.make_clip(source);
        self.prefs.push_recent(&clip.info.path.to_string_lossy());
        self.prefs.save();
        self.audio.set_track(clip.source.audio());
        self.frame = 0;
        self.zoom = None;
        self.pan = point(px(0.), px(0.));
        self.draft.clear();
        self.draft_frame = None;
        self.focused_comment = None;
        self.probe = None;
        clip.loader.set_playhead(0, 0);
        self.clip = Some(clip);
        self.reconfigure_loaders();
        cx.notify();
    }

    pub fn set_compare(&mut self, source: Option<Source>, cx: &mut Context<Self>) {
        if let Some(old) = self.compare.take() {
            self.cache.remove_source(old.info.id);
        }
        self.presenter.clear();
        if let Some(s) = source {
            let clip = self.make_clip(s);
            clip.loader.set_playhead(self.frame, 0);
            self.compare = Some(clip);
        }
        self.reconfigure_loaders();
        cx.notify();
    }

    fn configure_loader(&self, clip: &mut Clip, clips: usize) {
        let info = &clip.info;
        let frame_bytes = info.frame_bytes().max(1);
        let share = self.cache.budget() / clips.max(1);
        let budget_frames = (share / frame_bytes) as u64;
        let mut window = (budget_frames as f64 * 0.85) as u64;
        // Long-GOP movies decode on one thread. Cache the whole clip when it
        // fits in half the budget, so scrubbing never waits on a GOP decode;
        // otherwise stay ten seconds ahead rather than chewing through a
        // feature film while paused.
        let whole_clip_fits = (info.frame_count as usize).saturating_mul(frame_bytes) <= share / 2;
        if info.kind == MediaKind::Video && clip.source.parallelism() == 1 && !whole_clip_fits {
            window = window.min((info.fps * 10.0) as u64);
        }
        clip.loader.set_window(window.clamp(2, info.frame_count.max(2)));
        clip.loader.set_range(clip.in_point.or(clip.out_point).map(|_| clip.range()));
    }

    pub fn reconfigure_loaders(&mut self) {
        let n = 1 + self.compare.is_some() as usize;
        if let Some(mut c) = self.clip.take() {
            self.configure_loader(&mut c, n);
            self.clip = Some(c);
        }
        if let Some(mut c) = self.compare.take() {
            self.configure_loader(&mut c, n);
            self.compare = Some(c);
        }
    }

    pub fn prompt_open(&mut self, folders: bool, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: !folders,
            prompt: Some(if folders { "Open folder".into() } else { "Open".into() }),
        });
        cx.spawn(async move |this, cx| match rx.await {
            Ok(Ok(Some(paths))) => {
                let _ = this.update(cx, |app, cx| app.open_paths(paths, cx));
            }
            Ok(Err(e)) => {
                let _ = this.update(cx, |app, cx| {
                    app.show_toast(format!("File dialog unavailable ({e:#}); drag files onto the window instead"));
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }

    pub fn prompt_compare(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Compare with".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.first() {
                    let _ = this.update(cx, |app, cx| match media::open(p) {
                        Ok(s) => app.set_compare(Some(s), cx),
                        Err(e) => app.show_toast(format!("{e:#}")),
                    });
                }
            }
        })
        .detach();
    }

    pub fn step_clip(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(bin) = &self.bin else { return };
        let playable: Vec<&BinEntry> = bin.entries.iter().filter(|e| e.is_playable()).collect();
        if playable.is_empty() {
            return;
        }
        let current = self.clip.as_ref().map(|c| c.info.path.clone());
        let idx = playable
            .iter()
            .position(|e| Some(&e.path) == current.as_ref() || current.as_ref().is_some_and(|c| entry_matches(e, c)));
        let next = match idx {
            Some(i) => (i as i32 + delta).rem_euclid(playable.len() as i32) as usize,
            None => 0,
        };
        let entry = playable[next].clone();
        self.open_entry(&entry, cx);
    }

    // --------------------------------------------------------------- playback

    pub fn fps(&self) -> f64 {
        self.clip.as_ref().map(|c| c.info.fps).filter(|f| *f > 0.0).unwrap_or(24.0)
    }

    pub fn is_playing(&self) -> bool {
        self.playback.dir != 0
    }

    pub fn play(&mut self, dir: i32, speed: f32) {
        let Some(clip) = &self.clip else { return };
        if clip.info.frame_count <= 1 {
            return;
        }
        let (a, b) = clip.range();
        // Restart from the top when play is pressed on the last frame.
        if dir > 0 && self.frame >= b {
            self.frame = a;
        } else if dir < 0 && self.frame <= a {
            self.frame = b;
        }
        self.playback.dir = dir;
        self.playback.speed = speed;
        self.playback.anchor_frame = self.frame as f64;
        self.playback.anchor = Instant::now();
        self.playback.audio_clock = dir == 1 && (speed - 1.0).abs() < 1e-3 && self.audio.has_track();
        self.playback.audio_seen = None;
        self.playback.audio_moved = Instant::now();
        if self.playback.audio_clock {
            self.audio.play_from(self.frame as f64 / self.fps());
        } else {
            self.audio.pause();
        }
        self.sync_loaders();
    }

    pub fn stop(&mut self) {
        if self.playback.dir != 0 {
            self.playback.dir = 0;
            self.audio.pause();
            self.sync_loaders();
        }
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play(1, 1.0);
        }
    }

    /// Advances the playhead from the clock. Called every frame while playing.
    pub fn tick(&mut self) {
        if self.playback.dir == 0 {
            return;
        }
        let Some(clip) = &self.clip else { return };
        let fps = self.fps();
        let (a, b) = clip.range();
        if self.playback.audio_clock {
            let pos = self.audio.position();
            if pos != self.playback.audio_seen {
                self.playback.audio_seen = pos;
                self.playback.audio_moved = Instant::now();
            } else if self.playback.audio_moved.elapsed()
                > if pos.is_some() { Duration::from_secs_f64(2.5 / fps) } else { AUDIO_START_GRACE }
            {
                // Audio ended early (codec padding), underran, or never
                // started: hand over to the wall clock from where we are.
                self.playback.audio_clock = false;
                self.playback.anchor_frame = pos.map(|p| p * fps).unwrap_or(self.frame as f64);
                self.playback.anchor = Instant::now();
            }
        }
        let target = if self.playback.audio_clock {
            match self.playback.audio_seen {
                Some(secs) => secs * fps,
                // Audio is still priming; hold.
                None => self.playback.anchor_frame,
            }
        } else {
            let elapsed = self.playback.anchor.elapsed().as_secs_f64();
            self.playback.anchor_frame + elapsed * fps * self.playback.speed as f64 * self.playback.dir as f64
        };
        let frame = target.floor();
        let looping = self.prefs.loop_playback;
        if frame > b as f64 {
            if looping {
                self.frame = a;
                let (dir, speed) = (self.playback.dir, self.playback.speed);
                self.play(dir, speed);
                return;
            }
            self.frame = b;
            self.stop();
            return;
        }
        if frame < a as f64 {
            if looping {
                self.frame = b;
                let (dir, speed) = (self.playback.dir, self.playback.speed);
                self.play(dir, speed);
                return;
            }
            self.frame = a;
            self.stop();
            return;
        }
        let f = frame as u64;
        if f != self.frame {
            self.frame = f;
            self.sync_loaders();
        }
    }

    pub fn sync_loaders(&self) {
        let dir = self.playback.dir;
        if let Some(c) = &self.clip {
            c.loader.set_playhead(self.frame, dir);
        }
        if let Some(c) = &self.compare {
            c.loader.set_playhead(self.frame.min(c.info.frame_count.saturating_sub(1)), dir);
        }
    }

    pub fn seek(&mut self, frame: u64) {
        let Some(clip) = &self.clip else { return };
        let frame = frame.min(clip.info.frame_count.saturating_sub(1));
        let was_playing = self.is_playing();
        let (dir, speed) = (self.playback.dir, self.playback.speed);
        self.frame = frame;
        if was_playing {
            self.play(dir, speed);
        } else {
            self.sync_loaders();
        }
        self.probe = None;
    }

    pub fn step(&mut self, delta: i64) {
        self.stop();
        let Some(clip) = &self.clip else { return };
        let last = clip.info.frame_count.saturating_sub(1) as i64;
        self.seek((self.frame as i64 + delta).clamp(0, last) as u64);
    }

    pub fn set_in(&mut self) {
        let f = self.frame;
        if let Some(c) = self.clip.as_mut() {
            c.in_point = Some(f);
            if c.out_point.is_some_and(|o| o < f) {
                c.out_point = None;
            }
        }
        self.reconfigure_loaders();
    }

    pub fn set_out(&mut self) {
        let f = self.frame;
        if let Some(c) = self.clip.as_mut() {
            c.out_point = Some(f);
            if c.in_point.is_some_and(|i| i > f) {
                c.in_point = None;
            }
        }
        self.reconfigure_loaders();
    }

    pub fn clear_in_out(&mut self) {
        if let Some(c) = self.clip.as_mut() {
            c.in_point = None;
            c.out_point = None;
        }
        self.reconfigure_loaders();
    }

    // ------------------------------------------------------------------ color

    pub fn lut_for(&self, clip: &Clip) -> Option<Arc<DisplayLut>> {
        match self.color.display_lut(&clip.choice, self.exposure, self.gamma) {
            Ok(l) => Some(l),
            Err(e) => {
                log::warn!("display transform: {e:#}");
                None
            }
        }
    }

    /// Applies a new color choice to the A clip and remembers it for every
    /// file of the same format.
    pub fn set_choice(&mut self, choice: ColorChoice) {
        let key = self.color.memory_key();
        if let Some(c) = self.clip.as_mut() {
            self.prefs.remember(&key, &c.info.format.id(), choice.clone());
            c.choice = choice.clone();
            c.remembered = true;
        }
        // The B clip follows the memory too when it shares the format.
        if let (Some(a), Some(b)) = (&self.clip, self.compare.as_mut()) {
            if a.info.format == b.info.format {
                b.choice = choice;
                b.remembered = true;
            }
        }
        self.prefs.save();
        self.presenter.clear();
    }

    pub fn reset_choice(&mut self) {
        let key = self.color.memory_key();
        if let Some(c) = self.clip.as_mut() {
            self.prefs.forget(&key, &c.info.format.id());
            c.choice = self.color.default_choice(&c.info);
            c.remembered = false;
        }
        self.prefs.save();
        self.presenter.clear();
    }

    pub fn reload_color(&mut self, cx: &mut Context<Self>) {
        let (color, warning) = ColorManager::load(&self.prefs.ocio);
        self.color = Arc::new(color);
        if let Some(w) = warning {
            self.show_toast(w);
        }
        let key = self.color.memory_key();
        for clip in [self.clip.as_mut(), self.compare.as_mut()].into_iter().flatten() {
            let (choice, remembered) = match self.prefs.remembered(&key, &clip.info.format.id()) {
                Some(c) if self.color.validate(c) => (c.clone(), true),
                _ => (self.color.default_choice(&clip.info), false),
            };
            clip.choice = choice;
            clip.remembered = remembered;
        }
        self.presenter.clear();
        cx.notify();
    }

    pub fn set_channel(&mut self, ch: Channel) {
        self.channel = if self.channel == ch && ch != Channel::Rgb { Channel::Rgb } else { ch };
    }

    pub fn set_exposure(&mut self, stops: f32) {
        self.exposure = (stops * 4.0).round() / 4.0;
        self.exposure = self.exposure.clamp(-12.0, 12.0);
    }

    // ---------------------------------------------------------------- display

    /// The current image for a slot, kicking off a render if needed. Returns
    /// what to paint now (possibly the previous frame while the new one renders).
    pub fn present(&mut self, slot: usize, cx: &mut Context<Self>) -> SlotView {
        let clip = if slot == 0 { self.clip.as_ref() } else { self.compare.as_ref() };
        let Some(clip) = clip else { return SlotView::Empty };
        let frame = self.frame.min(clip.info.frame_count.saturating_sub(1));
        let Some(lut) = self.lut_for(clip) else { return SlotView::Empty };
        let key = PresentKey { clip: clip.info.id, frame, lut: Arc::as_ptr(&lut) as usize, channel: self.channel };

        if let Some(img) = self.presenter.get(&key) {
            if slot == 0 {
                self.count_presented(frame);
            }
            self.presenter.show(slot, key, img.clone());
            self.prefetch_display(slot, &lut, cx);
            return SlotView::Image(img);
        }
        let status = clip.loader.get(frame);
        match status {
            // Paused or scrubbing: convert right away so the viewer tracks the
            // mouse without waiting a repaint for a background render.
            FrameStatus::Ready(f) if self.playback.dir == 0 => {
                let img = to_render_image(display::render(&f, &lut, self.channel));
                self.presenter.insert(key.clone(), img.clone());
                self.presenter.show(slot, key, img.clone());
                return SlotView::Image(img);
            }
            FrameStatus::Ready(f) => self.render_async(key.clone(), f, lut.clone(), cx),
            FrameStatus::Failed(e) => return SlotView::Missing(e),
            FrameStatus::Loading if self.playback.dir == 0 => {
                // Scrubbing ahead of the decoder: show the nearest decoded
                // frame now, and the exact one as soon as it lands.
                let last = clip.info.frame_count.saturating_sub(1);
                let near = (1..=48u64).find_map(|d| {
                    [frame.checked_sub(d), (frame + d <= last).then_some(frame + d)]
                        .into_iter()
                        .flatten()
                        .find_map(|i| match clip.loader.get(i) {
                            FrameStatus::Ready(f) => Some((i, f)),
                            _ => None,
                        })
                });
                if let Some((i, f)) = near {
                    let near_key = PresentKey { frame: i, ..key.clone() };
                    let img = match self.presenter.get(&near_key) {
                        Some(img) => img,
                        None => {
                            let img = to_render_image(display::render(&f, &lut, self.channel));
                            self.presenter.insert(near_key.clone(), img.clone());
                            img
                        }
                    };
                    self.presenter.show(slot, near_key, img.clone());
                    return SlotView::Image(img);
                }
            }
            FrameStatus::Loading => {}
        }
        self.prefetch_display(slot, &lut, cx);
        match &self.presenter.shown[slot] {
            // Hold the previous frame of the same clip while this one decodes.
            Some((k, img)) if k.clip == key.clip => SlotView::Image(img.clone()),
            _ => SlotView::Loading,
        }
    }

    fn count_presented(&mut self, frame: u64) {
        let playing = self.playback.dir != 0;
        let fps = self.fps();
        let Some((start, count, last)) = self.stats.as_mut() else { return };
        if !playing {
            return;
        }
        if *last != frame {
            *count += 1;
            *last = frame;
        }
        let elapsed = start.elapsed().as_secs_f64();
        if elapsed >= 2.0 {
            log::info!("presented {:.1} distinct frames/s (clip {:.3} fps)", *count as f64 / elapsed, fps);
            *start = Instant::now();
            *count = 0;
        }
    }

    /// While playing, convert the next few decoded frames ahead of time.
    fn prefetch_display(&mut self, slot: usize, lut: &Arc<DisplayLut>, cx: &mut Context<Self>) {
        if self.playback.dir == 0 {
            return;
        }
        let clip = if slot == 0 { self.clip.as_ref() } else { self.compare.as_ref() };
        let Some(clip) = clip else { return };
        let (a, b) = clip.range();
        let mut jobs = Vec::new();
        for k in 1..=4i64 {
            let f = self.frame as i64 + k * self.playback.dir as i64;
            if f < a as i64 || f > b as i64 {
                break;
            }
            let key = PresentKey { clip: clip.info.id, frame: f as u64, lut: Arc::as_ptr(lut) as usize, channel: self.channel };
            if self.presenter.ready.contains_key(&key) || self.presenter.pending.contains(&key) {
                continue;
            }
            if let FrameStatus::Ready(fr) = clip.loader.get(f as u64) {
                jobs.push((key, fr));
            }
        }
        for (key, fr) in jobs {
            self.render_async(key, fr, lut.clone(), cx);
        }
    }

    fn render_async(&mut self, key: PresentKey, frame: Arc<Frame>, lut: Arc<DisplayLut>, cx: &mut Context<Self>) {
        if !self.presenter.pending.insert(key.clone()) {
            return;
        }
        let channel = key.channel;
        let task = cx.background_spawn(async move { display::render(&frame, &lut, channel) });
        cx.spawn(async move |this, cx| {
            let img = task.await;
            let render = to_render_image(img);
            let _ = this.update(cx, |app, cx| {
                app.presenter.insert(key, render);
                cx.notify();
            });
        })
        .detach();
    }

    // ----------------------------------------------------------- view geometry

    /// Where an image lands inside `area` with the current zoom and pan.
    pub fn image_rect(&self, area: Bounds<Pixels>, width: u32, height: u32, aspect: f32, scale_factor: f32) -> Bounds<Pixels> {
        image_rect(area, width, height, aspect, self.zoom, self.pan, scale_factor)
    }

    /// Logical pixels per image pixel.
    pub fn view_scale(&self, area: Bounds<Pixels>, width: u32, height: u32, aspect: f32, scale_factor: f32) -> f32 {
        view_scale(area, width, height, aspect, self.zoom, scale_factor)
    }

    pub fn zoom_at(&mut self, factor: f32, at: Option<Point<Pixels>>, scale_factor: f32) {
        let Some(clip) = &self.clip else { return };
        let area = self.viewer_bounds.get();
        let (w, h, a) = (clip.info.width, clip.info.height, clip.info.pixel_aspect);
        let current = self.view_scale(area, w, h, a, scale_factor);
        let new_zoom = (current * scale_factor * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let new_scale = new_zoom / scale_factor;
        // Keep the point under the cursor fixed.
        let at = at.unwrap_or(area.center());
        let center = area.center() + self.pan;
        let k = new_scale / current;
        let nx = f32::from(at.x) - (f32::from(at.x) - f32::from(center.x)) * k;
        let ny = f32::from(at.y) - (f32::from(at.y) - f32::from(center.y)) * k;
        self.pan = point(px(nx) - area.center().x, px(ny) - area.center().y);
        self.zoom = Some(new_zoom);
    }

    pub fn fit(&mut self) {
        self.zoom = None;
        self.pan = point(px(0.), px(0.));
    }

    pub fn one_to_one(&mut self) {
        self.zoom = Some(1.0);
        self.pan = point(px(0.), px(0.));
    }

    /// Screen position -> normalized image coordinates for slot A.
    pub fn to_image(&self, pos: Point<Pixels>, scale_factor: f32) -> Option<[f32; 2]> {
        let clip = self.clip.as_ref()?;
        let area = self.slot_area(0);
        let r = self.image_rect(area, clip.info.width, clip.info.height, clip.info.pixel_aspect, scale_factor);
        Some([
            (f32::from(pos.x - r.origin.x) / f32::from(r.size.width)),
            (f32::from(pos.y - r.origin.y) / f32::from(r.size.height)),
        ])
    }

    /// The viewer area for a slot; side-by-side splits it.
    pub fn slot_area(&self, slot: usize) -> Bounds<Pixels> {
        let b = self.viewer_bounds.get();
        if self.compare.is_some() && self.compare_mode == CompareMode::SideBySide {
            let half = b.size.width / 2.;
            let origin = if slot == 0 { b.origin } else { point(b.origin.x + half, b.origin.y) };
            Bounds { origin, size: gpui::size(half, b.size.height) }
        } else {
            b
        }
    }

    // ------------------------------------------------------------ annotations

    pub fn current_marker(&self) -> [u8; 3] {
        crate::theme::MARKERS[self.prefs.annotation_color % crate::theme::MARKERS.len()]
    }

    /// Stroke width as a fraction of image height.
    pub fn stroke(&self) -> f32 {
        self.prefs.stroke_width / 1000.0
    }

    pub fn begin_shape(&mut self, p: [f32; 2]) {
        if self.draft_frame.is_some_and(|f| f != self.frame) && !self.draft.is_empty() {
            self.show_toast("Finish or clear the annotation on the other frame first".into());
            return;
        }
        self.draft_frame = Some(self.frame);
        self.stop();
        let color = self.current_marker();
        let width = self.stroke();
        self.in_progress = match self.tool {
            Tool::Pen => Some(Shape::Pen { points: vec![p], color, width }),
            Tool::Arrow => Some(Shape::Arrow { from: p, to: p, color, width }),
            Tool::Line => Some(Shape::Line { from: p, to: p, color, width }),
            Tool::Rect => Some(Shape::Rect { from: p, to: p, color, width }),
            Tool::Ellipse => Some(Shape::Ellipse { from: p, to: p, color, width }),
            Tool::Eraser => {
                self.erase_at(p);
                None
            }
            Tool::Text => None,
        };
    }

    pub fn extend_shape(&mut self, p: [f32; 2], shift: bool) {
        if self.tool == Tool::Eraser {
            self.erase_at(p);
            return;
        }
        match self.in_progress.as_mut() {
            Some(Shape::Pen { points, .. }) => {
                if points.last().is_none_or(|l| (l[0] - p[0]).abs() + (l[1] - p[1]).abs() > 0.0008) {
                    points.push(p);
                }
            }
            Some(Shape::Arrow { from, to, .. }) | Some(Shape::Line { from, to, .. }) => {
                *to = if shift { snap_45(*from, p) } else { p };
            }
            Some(Shape::Rect { from, to, .. }) | Some(Shape::Ellipse { from, to, .. }) => {
                *to = if shift {
                    let d = (p[0] - from[0]).abs().max(p[1] - from[1]);
                    [from[0] + d * (p[0] - from[0]).signum(), from[1] + d * (p[1] - from[1]).signum()]
                } else {
                    p
                };
            }
            _ => {}
        }
    }

    pub fn finish_shape(&mut self) {
        if let Some(s) = self.in_progress.take() {
            let trivial = match &s {
                Shape::Arrow { from, to, .. } | Shape::Line { from, to, .. } | Shape::Rect { from, to, .. } | Shape::Ellipse { from, to, .. } => {
                    (from[0] - to[0]).abs() + (from[1] - to[1]).abs() < 0.002
                }
                _ => false,
            };
            if !trivial {
                self.draft.push(s);
            }
        }
    }

    fn erase_at(&mut self, p: [f32; 2]) {
        let radius = 0.012;
        self.draft.retain(|s| s.distance(p) > radius);
        if self.draft.is_empty() {
            self.draft_frame = None;
        }
    }

    pub fn undo_shape(&mut self) {
        self.draft.pop();
        if self.draft.is_empty() {
            self.draft_frame = None;
        }
    }

    pub fn clear_draft(&mut self) {
        self.draft.clear();
        self.draft_frame = None;
        self.in_progress = None;
    }

    pub fn add_text_annotation(&mut self, text: String) {
        let Some(at) = self.text_anchor.take() else { return };
        if text.trim().is_empty() {
            return;
        }
        self.draft_frame = Some(self.frame);
        self.draft.push(Shape::Text { at, text, color: self.current_marker(), size: (self.stroke() * 6.0).max(0.024) });
    }

    pub fn submit_comment(&mut self, text: String) -> bool {
        let shapes = std::mem::take(&mut self.draft);
        let frame = self.draft_frame.take().unwrap_or(self.frame);
        let text = text.trim().to_string();
        if text.is_empty() && shapes.is_empty() {
            return false;
        }
        let Some(clip) = self.clip.as_mut() else { return false };
        let comment = Comment::new(frame, text, self.prefs.author.clone(), shapes);
        let id = comment.id.clone();
        clip.notes.comments.push(comment);
        clip.notes.comments.sort_by_key(|c| (c.frame, c.created));
        self.focused_comment = Some(id);
        self.drawing = false;
        self.save_notes();
        true
    }

    pub fn delete_comment(&mut self, id: &str) {
        if let Some(clip) = self.clip.as_mut() {
            clip.notes.comments.retain(|c| c.id != id);
        }
        if self.focused_comment.as_deref() == Some(id) {
            self.focused_comment = None;
        }
        self.save_notes();
    }

    pub fn toggle_done(&mut self, id: &str) {
        if let Some(clip) = self.clip.as_mut() {
            if let Some(c) = clip.notes.comments.iter_mut().find(|c| c.id == id) {
                c.done = !c.done;
            }
        }
        self.save_notes();
    }

    fn save_notes(&mut self) {
        let Some(clip) = &self.clip else { return };
        match clip.notes.save(&clip.info) {
            Ok(p) if p != clip.info.notes_path && !clip.notes.comments.is_empty() => {
                self.show_toast(format!("Media folder is read-only; notes saved to {}", p.display()));
            }
            Ok(_) => {}
            Err(e) => self.show_toast(format!("Couldn't save notes: {e:#}")),
        }
    }

    pub fn jump_comment(&mut self, delta: i32) {
        let Some(clip) = &self.clip else { return };
        let mut frames: Vec<u64> = clip.notes.comments.iter().map(|c| c.frame).collect();
        frames.dedup();
        let target = if delta > 0 {
            frames.iter().find(|f| **f > self.frame).or(frames.first()).copied()
        } else {
            frames.iter().rev().find(|f| **f < self.frame).or(frames.last()).copied()
        };
        if let Some(f) = target {
            let focused = clip.notes.comments.iter().find(|c| c.frame == f).map(|c| c.id.clone());
            self.stop();
            self.seek(f);
            self.focused_comment = focused;
        }
    }

    /// Shapes visible on the current frame: saved comments plus the draft.
    pub fn visible_shapes(&self) -> Vec<(Shape, bool)> {
        let mut out = Vec::new();
        if let Some(clip) = &self.clip {
            for c in clip.notes.on_frame(self.frame) {
                let focused = self.focused_comment.as_deref() == Some(c.id.as_str());
                out.extend(c.shapes.iter().cloned().map(|s| (s, focused || self.focused_comment.is_none())));
            }
        }
        if self.draft_frame.is_none_or(|f| f == self.frame) {
            out.extend(self.draft.iter().cloned().map(|s| (s, true)));
            if let Some(s) = &self.in_progress {
                out.push((s.clone(), true));
            }
        }
        out
    }

    // -------------------------------------------------------------- stills

    pub fn save_frame(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = &self.clip else { return };
        let Some((_, img)) = self.presenter.shown[0].clone() else {
            self.show_toast("Nothing displayed yet".into());
            return;
        };
        let shapes: Vec<Shape> = self.visible_shapes().into_iter().map(|(s, _)| s).collect();
        let dir = clip.info.path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let stem = clip.info.name.split(" [").next().unwrap_or("frame").replace(['#', ' '], "").trim_end_matches(['.', '_']).to_string();
        let stem = Path::new(&stem).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(stem);
        let name = format!("{}_{}.png", stem, clip.frame_label(self.frame));
        let fallback = dir.join(&name);
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let path = match rx.await {
                Ok(Ok(Some(path))) => path,
                // No portal: save next to the media rather than not at all.
                Ok(Err(e)) => {
                    log::warn!("save dialog unavailable: {e:#}");
                    fallback
                }
                _ => return,
            };
            let path = if path.extension().is_none() { path.with_extension("png") } else { path };
            let (w, h) = (img.size(0).width.0 as u32, img.size(0).height.0 as u32);
            let bytes = img.as_bytes(0).map(|b| b.to_vec()).unwrap_or_default();
            let result = cx
                .background_spawn(async move {
                    let image = display::DisplayImage { width: w, height: h, bgra: bytes };
                    plate_core::bake::save_still(&path, &image, &shapes, Some(crate::assets::BAKE_FONT)).map(|_| path)
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                match result {
                    Ok(p) => app.show_toast(format!("Saved {}", p.display())),
                    Err(e) => app.show_toast(format!("Save failed: {e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ------------------------------------------------------------------- misc

    /// Sets and persists the UI scale (Ctrl +/−).
    pub fn set_ui_scale(&mut self, v: f32, cx: &mut Context<Self>) {
        let v = crate::theme::set_ui_scale(v);
        self.prefs.ui_scale = v;
        self.prefs.save();
        self.show_toast(format!("UI scale {:.0}%", v * 100.0));
        cx.notify();
    }

    pub fn show_toast(&mut self, text: String) {
        log::info!("{text}");
        self.toast = Some(Toast { text, until: Instant::now() + Duration::from_secs(4) });
    }

    pub fn focus_viewer(&self, window: &mut Window) {
        window.focus(&self.viewer_focus);
    }

    fn on_comment_input(&mut self, input: &Entity<TextInput>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Submit => {
                let text = input.read(cx).text().to_string();
                if self.submit_comment(text) {
                    input.update(cx, |i, cx| i.set_text("", cx));
                    self.focus_viewer(window);
                }
                cx.notify();
            }
            InputEvent::Cancel => {
                self.focus_viewer(window);
                cx.notify();
            }
            InputEvent::Changed => {
                // Typing a comment pins it to the current frame, like Frame.io.
                if self.draft_frame.is_none() {
                    self.stop();
                }
                cx.notify();
            }
        }
    }

    fn on_search_input(&mut self, _: &Entity<TextInput>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let InputEvent::Cancel = ev {
            self.modal = None;
            self.focus_viewer(window);
        }
        cx.notify();
    }

    fn on_annotation_input(&mut self, input: &Entity<TextInput>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Submit => {
                let text = input.read(cx).text().to_string();
                self.add_text_annotation(text);
                input.update(cx, |i, cx| i.set_text("", cx));
                self.modal = None;
                self.focus_viewer(window);
            }
            InputEvent::Cancel => {
                self.text_anchor = None;
                self.modal = None;
                self.focus_viewer(window);
            }
            InputEvent::Changed => {}
        }
        cx.notify();
    }

    pub fn open_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = Some(modal);
        match modal {
            Modal::Colorspace => {
                self.search_input.update(cx, |i, cx| i.set_text("", cx));
                window.focus(&self.search_input.focus_handle(cx));
            }
            Modal::TextAnnotation => window.focus(&self.annotation_input.focus_handle(cx)),
            _ => {}
        }
        cx.notify();
    }

    pub fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = None;
        self.text_anchor = None;
        self.focus_viewer(window);
        cx.notify();
    }

    pub fn frame_text(&self, index: u64) -> String {
        let Some(clip) = &self.clip else { return "–".into() };
        match self.counter {
            CounterMode::Frames => clip.frame_label(index).to_string(),
            CounterMode::Index => (index + 1).to_string(),
            CounterMode::Timecode => timecode(index, clip.info.fps, clip.info.timecode.as_deref()),
        }
    }
}

pub enum SlotView {
    Empty,
    Loading,
    Image(Arc<RenderImage>),
    Missing(String),
}

fn to_render_image(img: display::DisplayImage) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(img.width, img.height, img.bgra).expect("display image size");
    let frame = image::Frame::new(buffer);
    Arc::new(RenderImage::new(SmallVec::from_elem(frame, 1)))
}

fn snap_45(from: [f32; 2], p: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (p[0] - from[0], p[1] - from[1]);
    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    let len = (dx * dx + dy * dy).sqrt();
    [from[0] + len * snapped.cos(), from[1] + len * snapped.sin()]
}

pub fn fit_scale(area: Bounds<Pixels>, width: u32, height: u32, aspect: f32) -> f32 {
    let w = width as f32 * aspect.max(0.01);
    let h = height as f32;
    let margin = 24.0;
    let aw = (f32::from(area.size.width) - margin * 2.0).max(10.0);
    let ah = (f32::from(area.size.height) - margin * 2.0).max(10.0);
    (aw / w).min(ah / h)
}

/// Logical pixels per image pixel. `zoom` is image pixels per device pixel.
pub fn view_scale(area: Bounds<Pixels>, width: u32, height: u32, aspect: f32, zoom: Option<f32>, scale_factor: f32) -> f32 {
    match zoom {
        None => fit_scale(area, width, height, aspect),
        Some(z) => z / scale_factor.max(0.5),
    }
}

pub fn image_rect(
    area: Bounds<Pixels>,
    width: u32,
    height: u32,
    aspect: f32,
    zoom: Option<f32>,
    pan: Point<Pixels>,
    scale_factor: f32,
) -> Bounds<Pixels> {
    let scale = view_scale(area, width, height, aspect, zoom, scale_factor);
    let size = gpui::size(px(width as f32 * aspect.max(0.01) * scale), px(height as f32 * scale));
    let center = area.center() + pan;
    Bounds { origin: point(center.x - size.width / 2., center.y - size.height / 2.), size }
}

/// Watches one folder (not recursively) and asks the app to rescan, at most
/// a few times a second however busy the folder is.
fn watch_dir(dir: &Path, cx: &mut Context<Plate>) -> Option<notify::RecommendedWatcher> {
    use notify::Watcher;
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if !matches!(ev.kind, notify::EventKind::Access(_)) {
                let _ = tx.unbounded_send(());
            }
        }
    })
    .map_err(|e| log::warn!("can't watch {}: {e}", dir.display()))
    .ok()?;
    if let Err(e) = watcher.watch(dir, notify::RecursiveMode::NonRecursive) {
        log::warn!("can't watch {}: {e}", dir.display());
        return None;
    }
    cx.spawn(async move |this, cx| {
        while rx.next().await.is_some() {
            // Let a burst (a render writing frames, a copy) settle first.
            cx.background_executor().timer(Duration::from_millis(300)).await;
            while let Ok(()) = rx.try_recv() {}
            if this.update(cx, |app, cx| app.refresh_bin(cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    Some(watcher)
}

pub fn dir_title(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.to_string_lossy().to_string())
}

fn entry_matches(e: &BinEntry, path: &Path) -> bool {
    match &e.kind {
        EntryKind::Sequence(seq) => path.parent() == Some(seq.dir.as_path()) && {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            name.starts_with(&seq.prefix) && name.ends_with(&seq.suffix)
        },
        _ => e.path == path,
    }
}

/// SMPTE-style timecode, offset by the file's start timecode when present.
pub fn timecode(index: u64, fps: f64, start: Option<&str>) -> String {
    let nominal = fps.round().max(1.0) as u64;
    let mut total = index;
    if let Some(tc) = start {
        let parts: Vec<u64> = tc.split([':', ';', '.']).filter_map(|p| p.parse().ok()).collect();
        if parts.len() == 4 {
            total += ((parts[0] * 60 + parts[1]) * 60 + parts[2]) * nominal + parts[3];
        }
    }
    let f = total % nominal;
    let s = total / nominal;
    format!("{:02}:{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60, f)
}
