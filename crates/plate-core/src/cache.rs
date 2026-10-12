//! A byte-budgeted frame cache shared by every open clip.

use crate::disk::DiskTier;
use crate::frame::Frame;
use parking_lot::{Mutex, RwLock};
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
    /// Where RAM evictions go, when a disk cache is configured.
    disk: RwLock<Option<Arc<DiskTier>>>,
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
            disk: RwLock::new(None),
        }
    }

    pub fn budget(&self) -> usize {
        self.inner.lock().budget
    }

    pub fn set_budget(&self, bytes: usize) {
        let mut g = self.inner.lock();
        g.budget = bytes;
        let victims = evict(&mut g);
        drop(g);
        self.spill(victims);
    }

    /// Replaces the disk tier. The previous one deletes its folder.
    pub fn set_disk(&self, tier: Option<DiskTier>) {
        *self.disk.write() = tier.map(Arc::new);
    }

    pub fn disk(&self) -> Option<Arc<DiskTier>> {
        self.disk.read().clone()
    }

    /// Moves a frame from the disk tier back into RAM.
    pub fn load_from_disk(&self, key: CacheKey) -> Option<Arc<Frame>> {
        let disk = self.disk()?;
        if !disk.contains(key) {
            return None;
        }
        let frame = Arc::new(disk.load(key)?);
        self.insert(key, frame.clone());
        Some(frame)
    }

    fn spill(&self, victims: Vec<(CacheKey, Arc<Frame>)>) {
        if victims.is_empty() {
            return;
        }
        if let Some(disk) = self.disk() {
            for (k, f) in victims {
                disk.spill(k, f);
            }
        }
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
        let victims = evict(&mut g);
        drop(g);
        self.spill(victims);
    }

    pub fn set_focus(&self, source: u64, focus: Option<Focus>) {
        let mut g = self.inner.lock();
        match focus {
            Some(f) => g.focus.insert(source, f),
            None => g.focus.remove(&source),
        };
    }

    /// Indices of cached frames (RAM or disk) for one source, for the
    /// timeline cache bar.
    pub fn cached(&self, source: u64, frame_count: u64) -> Vec<bool> {
        let mut out = vec![false; frame_count as usize];
        {
            let g = self.inner.lock();
            for &(s, i) in g.map.keys() {
                if s == source && (i as usize) < out.len() {
                    out[i as usize] = true;
                }
            }
        }
        if let Some(disk) = self.disk() {
            for i in disk.keys_for(source) {
                if (i as usize) < out.len() {
                    out[i as usize] = true;
                }
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
        drop(g);
        if let Some(disk) = self.disk() {
            disk.remove_source(source);
        }
    }

    /// Runs `f` with a membership test that holds the lock once, for scanning
    /// many candidates cheaply.
    pub fn with_contains<R>(&self, f: impl FnOnce(&dyn Fn(CacheKey) -> bool) -> R) -> R {
        let g = self.inner.lock();
        f(&|k| g.map.contains_key(&k))
    }
}

fn evict(g: &mut Inner) -> Vec<(CacheKey, Arc<Frame>)> {
    let mut victims = Vec::new();
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
            victims.push((k, e.frame));
        }
    }
    victims
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicted_frames_come_back_from_disk() {
        let loc = std::env::temp_dir().join(format!("plate-cache-test-{}", std::process::id()));
        let frame_bytes = Frame::new_u8(64, 64, false).byte_size();
        let cache = FrameCache::new(frame_bytes * 2);
        cache.set_disk(Some(DiskTier::new(&loc, 1 << 26).unwrap()));
        for i in 0..6u64 {
            let mut f = Frame::new_u8(64, 64, false);
            f.byte_mut()[0] = i as u8 * 10;
            cache.insert((9, i), Arc::new(f));
        }
        assert!(!cache.contains((9, 0)), "frame 0 left RAM");
        let disk = cache.disk().unwrap();
        for _ in 0..200 {
            if disk.contains((9, 0)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let back = cache.load_from_disk((9, 0)).expect("frame 0 on disk");
        assert_eq!(back.sample(0, 0).unwrap()[0], 0.0);
        let back3 = cache.load_from_disk((9, 1)).expect("frame 1 on disk");
        assert_eq!(back3.sample(0, 0).unwrap()[0], 10.0 / 255.0);
        // Reloading evicted other frames; their writes are asynchronous.
        let mut all = false;
        for _ in 0..200 {
            all = cache.cached(9, 6).iter().all(|c| *c);
            if all {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(all, "timeline shows RAM and disk frames");
        let dir = disk.dir().to_path_buf();
        drop(disk);
        cache.set_disk(None);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&loc);
    }
}
