//! Background decoding ahead of the playhead.

use crate::cache::{Focus, FrameCache};
use crate::frame::Frame;
use crate::media::Source;
use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread::JoinHandle;

pub enum FrameStatus {
    Ready(Arc<Frame>),
    Loading,
    Failed(String),
}

struct State {
    playhead: u64,
    /// 1 forward, -1 reverse, 0 paused.
    direction: i32,
    /// How many frames ahead of the playhead to keep decoded.
    window: u64,
    /// Inclusive loop range; prefetch wraps inside it.
    range: Option<(u64, u64)>,
    inflight: HashSet<u64>,
    failed: HashMap<u64, String>,
    shutdown: bool,
}

struct Shared {
    source: Source,
    cache: Arc<FrameCache>,
    state: Mutex<State>,
    cv: Condvar,
    notify: Box<dyn Fn(u64) + Send + Sync>,
}

pub struct Loader {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl Loader {
    /// `notify` is called from worker threads each time a frame lands.
    pub fn new(source: Source, cache: Arc<FrameCache>, notify: impl Fn(u64) + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            source: source.clone(),
            cache,
            state: Mutex::new(State {
                playhead: 0,
                direction: 0,
                window: 48,
                range: None,
                inflight: HashSet::new(),
                failed: HashMap::new(),
                shutdown: false,
            }),
            cv: Condvar::new(),
            notify: Box::new(notify),
        });
        let workers = (0..source.parallelism().max(1))
            .map(|i| {
                let shared = shared.clone();
                std::thread::Builder::new()
                    .name(format!("plate-decode-{i}"))
                    .spawn(move || worker(shared))
                    .expect("spawn decode worker")
            })
            .collect();
        Self { shared, workers }
    }

    pub fn source(&self) -> &Source {
        &self.shared.source
    }

    pub fn set_playhead(&self, frame: u64, direction: i32) {
        let mut s = self.shared.state.lock();
        if s.playhead == frame && s.direction == direction {
            return;
        }
        s.playhead = frame;
        s.direction = direction;
        self.update_focus(&s);
        drop(s);
        self.shared.cv.notify_all();
    }

    pub fn set_window(&self, frames: u64) {
        let mut s = self.shared.state.lock();
        s.window = frames.max(1);
        self.update_focus(&s);
        drop(s);
        self.shared.cv.notify_all();
    }

    pub fn set_range(&self, range: Option<(u64, u64)>) {
        let mut s = self.shared.state.lock();
        s.range = range;
        drop(s);
        self.shared.cv.notify_all();
    }

    fn update_focus(&self, s: &State) {
        let id = self.shared.source.info().id;
        let back = if s.direction < 0 { s.window } else { s.window / 8 };
        let fwd = if s.direction < 0 { s.window / 8 } else { s.window };
        self.shared.cache.set_focus(
            id,
            Some(Focus { start: s.playhead.saturating_sub(back), end: s.playhead + fwd }),
        );
    }

    pub fn get(&self, frame: u64) -> FrameStatus {
        let id = self.shared.source.info().id;
        if let Some(f) = self.shared.cache.get((id, frame)) {
            return FrameStatus::Ready(f);
        }
        match self.shared.state.lock().failed.get(&frame) {
            Some(e) => FrameStatus::Failed(e.clone()),
            None => FrameStatus::Loading,
        }
    }

    /// Forget failures, e.g. after files changed on disk.
    pub fn retry_failed(&self) {
        self.shared.state.lock().failed.clear();
        self.shared.cv.notify_all();
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.state.lock().shutdown = true;
        self.shared.cv.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
        self.shared.cache.set_focus(self.shared.source.info().id, None);
    }
}

/// Picks the most urgent frame that is neither cached, in flight nor failed.
///
/// Order: the playhead, then ahead in the play direction, then behind. A
/// single stateful decoder (long-GOP video) gets the frames behind in
/// ascending order after everything ahead, so it seeks once instead of once
/// per frame; parallel sources also keep a few frames behind ready early for
/// stepping back.
fn next_job(shared: &Shared, s: &State) -> Option<u64> {
    let info = shared.source.info();
    let count = info.frame_count;
    if count == 0 {
        return None;
    }
    let sequential = shared.source.parallelism() <= 1;
    let id = info.id;
    let playhead = s.playhead.min(count - 1);
    let window = s.window.min(count);
    let dir: i64 = if s.direction < 0 { -1 } else { 1 };

    // Inside a loop range, prefetch wraps around the range.
    let (lo, hi) = s.range.unwrap_or((0, count - 1));
    let wrapping = s.range.is_some() && playhead >= lo && playhead <= hi;
    let (lo, hi) = if wrapping { (lo, hi) } else { (0, count - 1) };
    let span = hi - lo + 1;

    let ahead = |k: u64| -> Option<u64> {
        if wrapping {
            let off = playhead - lo;
            let v = if dir > 0 { (off + k) % span } else { (off + span - (k % span)) % span };
            Some(lo + v)
        } else if dir > 0 {
            (playhead + k <= hi).then_some(playhead + k)
        } else {
            playhead.checked_sub(k).filter(|v| *v >= lo)
        }
    };
    let behind = |k: u64| -> Option<u64> {
        if wrapping {
            return None;
        }
        if dir > 0 { playhead.checked_sub(k).filter(|v| *v >= lo) } else { (playhead + k <= hi).then_some(playhead + k) }
    };
    let n_ahead = if wrapping {
        span.min(window)
    } else if dir > 0 {
        (hi - playhead + 1).min(window)
    } else {
        (playhead - lo + 1).min(window)
    };
    let n_behind = window.saturating_sub(n_ahead);
    let early_behind = if s.direction == 0 && !sequential { 8.min(n_behind) } else { 0 };

    shared.cache.with_contains(|cached| {
        let free = |i: u64| !cached((id, i)) && !s.inflight.contains(&i) && !s.failed.contains_key(&i);
        if free(playhead) {
            return Some(playhead);
        }
        for k in 1..n_ahead.max(1) {
            if k <= early_behind {
                if let Some(i) = behind(k).filter(|i| free(*i)) {
                    return Some(i);
                }
            }
            if let Some(i) = ahead(k).filter(|i| free(*i)) {
                return Some(i);
            }
        }
        if sequential {
            // Farthest first, so one seek covers the whole stretch behind.
            (1..=n_behind).rev().filter_map(behind).find(|i| free(*i))
        } else {
            (1..=n_behind).filter_map(behind).find(|i| free(*i))
        }
    })
}

fn worker(shared: Arc<Shared>) {
    let mut reader = None;
    loop {
        let job = {
            let mut s = shared.state.lock();
            loop {
                if s.shutdown {
                    return;
                }
                if let Some(i) = next_job(&shared, &s) {
                    s.inflight.insert(i);
                    break i;
                }
                shared.cv.wait(&mut s);
            }
        };

        if reader.is_none() {
            match shared.source.reader() {
                Ok(r) => reader = Some(r),
                Err(e) => {
                    let mut s = shared.state.lock();
                    s.inflight.remove(&job);
                    s.failed.insert(job, e.to_string());
                    drop(s);
                    (shared.notify)(job);
                    continue;
                }
            }
        }
        let result = reader.as_mut().unwrap().read(job);
        let id = shared.source.info().id;
        match result {
            Ok(frame) => {
                shared.cache.insert((id, job), Arc::new(frame));
                shared.state.lock().inflight.remove(&job);
            }
            Err(e) => {
                // Decoder state may be wedged; start fresh next time.
                reader = None;
                let mut s = shared.state.lock();
                s.inflight.remove(&job);
                s.failed.insert(job, format!("{e:#}"));
            }
        }
        (shared.notify)(job);
    }
}
