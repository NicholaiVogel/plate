//! A byte-budgeted frame cache shared by every open clip.

use crate::frame::Frame;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub type CacheKey = (u64, u64);

struct Entry {
    frame: Arc<Frame>,
    last_used: u64,
}

/// The range a loader is actively working on. Frames inside a focus survive
/// eviction longer than frames outside it.
#[derive(Clone, Copy, Debug)]
pub struct Focus {
    pub start: u64,
    pub end: u64,
}

struct Inner {
    map: HashMap<CacheKey, Entry>,
    focus: HashMap<u64, Focus>,
    bytes: usize,
    budget: usize,
    tick: u64,
}

pub struct FrameCache {
    inner: Mutex<Inner>,
}

impl FrameCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                focus: HashMap::new(),
                bytes: 0,
                budget: budget_bytes,
                tick: 0,
            }),
        }
    }

    pub fn budget(&self) -> usize {
        self.inner.lock().budget
    }

    pub fn set_budget(&self, bytes: usize) {
        let mut g = self.inner.lock();
        g.budget = bytes;
        evict(&mut g);
    }

    pub fn used(&self) -> usize {
        self.inner.lock().bytes
    }

    pub fn get(&self, key: CacheKey) -> Option<Arc<Frame>> {
        let mut g = self.inner.lock();
        g.tick += 1;
        let tick = g.tick;
        g.map.get_mut(&key).map(|e| {
            e.last_used = tick;
            e.frame.clone()
        })
    }

    pub fn contains(&self, key: CacheKey) -> bool {
        self.inner.lock().map.contains_key(&key)
    }

    pub fn insert(&self, key: CacheKey, frame: Arc<Frame>) {
        let mut g = self.inner.lock();
        g.tick += 1;
        let tick = g.tick;
        let size = frame.byte_size();
        if let Some(old) = g.map.insert(key, Entry { frame, last_used: tick }) {
            g.bytes -= old.frame.byte_size();
        }
        g.bytes += size;
        evict(&mut g);
    }

    pub fn set_focus(&self, source: u64, focus: Option<Focus>) {
        let mut g = self.inner.lock();
        match focus {
            Some(f) => g.focus.insert(source, f),
            None => g.focus.remove(&source),
        };
    }

    /// Indices of cached frames for one source, for the timeline cache bar.
    pub fn cached(&self, source: u64, frame_count: u64) -> Vec<bool> {
        let g = self.inner.lock();
        let mut out = vec![false; frame_count as usize];
        for &(s, i) in g.map.keys() {
            if s == source && (i as usize) < out.len() {
                out[i as usize] = true;
            }
        }
        out
    }

    pub fn remove_source(&self, source: u64) {
        let mut g = self.inner.lock();
        let before = g.bytes;
        let mut freed = 0;
        g.map.retain(|&(s, _), e| {
            let keep = s != source;
            if !keep {
                freed += e.frame.byte_size();
            }
            keep
        });
        g.bytes = before - freed;
        g.focus.remove(&source);
    }

    /// Runs `f` with a membership test that holds the lock once, for scanning
    /// many candidates cheaply.
    pub fn with_contains<R>(&self, f: impl FnOnce(&dyn Fn(CacheKey) -> bool) -> R) -> R {
        let g = self.inner.lock();
        f(&|k| g.map.contains_key(&k))
    }
}

fn evict(g: &mut Inner) {
    while g.bytes > g.budget && !g.map.is_empty() {
        // Least recently used outside every focus range, else least recently used.
        let in_focus = |(s, i): &CacheKey| g.focus.get(s).is_some_and(|f| *i >= f.start && *i <= f.end);
        let victim = g
            .map
            .iter()
            .filter(|(k, _)| !in_focus(k))
            .min_by_key(|(_, e)| e.last_used)
            .or_else(|| g.map.iter().min_by_key(|(_, e)| e.last_used))
            .map(|(k, _)| *k);
        let Some(k) = victim else { break };
        if let Some(e) = g.map.remove(&k) {
            g.bytes -= e.frame.byte_size();
        }
    }
}
