//! Optional second cache tier on disk.
//!
//! Frames evicted from RAM are written to a per-session folder under a
//! user-chosen location and read back instead of being decoded again. The
//! folder is deleted when the session ends; folders left behind by a crash are
//! swept the next time any session starts.

use crate::cache::CacheKey;
use crate::frame::{Frame, Pixels};
use crossbeam_channel::{Receiver, Sender};
use half::f16;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAGIC: &[u8; 4] = b"PLF1";
const SESSION_PREFIX: &str = "plate-session-";

struct Entry {
    bytes: usize,
    last_used: u64,
}

#[derive(Default)]
struct Index {
    entries: HashMap<CacheKey, Entry>,
    bytes: usize,
    tick: u64,
}

enum Job {
    Write(CacheKey, Arc<Frame>),
    RemoveSource(u64),
}

pub struct DiskTier {
    dir: PathBuf,
    budget: usize,
    index: Arc<Mutex<Index>>,
    tx: Option<Sender<Job>>,
    writer: Option<std::thread::JoinHandle<()>>,
}

impl DiskTier {
    /// Creates this session's folder under `location`, sweeping folders
    /// left by sessions that are no longer running.
    pub fn new(location: &Path, budget: usize) -> std::io::Result<Self> {
        std::fs::create_dir_all(location)?;
        sweep_stale(location);
        let dir = location.join(format!("{SESSION_PREFIX}{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let index = Arc::new(Mutex::new(Index::default()));
        let (tx, rx) = crossbeam_channel::unbounded();
        let writer = {
            let dir = dir.clone();
            let index = index.clone();
            std::thread::Builder::new().name("plate-disk-cache".into()).spawn(move || writer(dir, budget, index, rx))?
        };
        Ok(Self { dir, budget, index, tx: Some(tx), writer: Some(writer) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn used(&self) -> usize {
        self.index.lock().bytes
    }

    pub fn contains(&self, key: CacheKey) -> bool {
        self.index.lock().entries.contains_key(&key)
    }

    pub fn keys_for(&self, source: u64) -> Vec<u64> {
        self.index.lock().entries.keys().filter(|(s, _)| *s == source).map(|(_, i)| *i).collect()
    }

    /// Queues a frame for writing. Never blocks on I/O.
    pub fn spill(&self, key: CacheKey, frame: Arc<Frame>) {
        if self.index.lock().entries.contains_key(&key) {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Job::Write(key, frame));
        }
    }

    pub fn remove_source(&self, source: u64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Job::RemoveSource(source));
        }
    }

    /// Reads a frame back, or None if it isn't on disk (or the file is bad).
    pub fn load(&self, key: CacheKey) -> Option<Frame> {
        {
            let mut idx = self.index.lock();
            idx.tick += 1;
            let tick = idx.tick;
            idx.entries.get_mut(&key)?.last_used = tick;
        }
        match read_frame(&path_for(&self.dir, key)) {
            Ok(f) => Some(f),
            Err(e) => {
                log::warn!("disk cache read {key:?}: {e}");
                let mut idx = self.index.lock();
                if let Some(e) = idx.entries.remove(&key) {
                    idx.bytes -= e.bytes;
                }
                None
            }
        }
    }
}

impl Drop for DiskTier {
    fn drop(&mut self) {
        // Closing the channel lets the writer finish and exit.
        self.tx.take();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        if let Err(e) = std::fs::remove_dir_all(&self.dir) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("clearing disk cache {}: {e}", self.dir.display());
            }
        }
    }
}

fn path_for(dir: &Path, (source, frame): CacheKey) -> PathBuf {
    dir.join(format!("{source}_{frame}.plf"))
}

fn writer(dir: PathBuf, budget: usize, index: Arc<Mutex<Index>>, rx: Receiver<Job>) {
    for job in rx {
        match job {
            Job::Write(key, frame) => {
                let path = path_for(&dir, key);
                match write_frame(&path, &frame) {
                    Ok(bytes) => {
                        let mut idx = index.lock();
                        idx.tick += 1;
                        let tick = idx.tick;
                        if let Some(old) = idx.entries.insert(key, Entry { bytes, last_used: tick }) {
                            idx.bytes -= old.bytes;
                        }
                        idx.bytes += bytes;
                        while idx.bytes > budget {
                            let Some(victim) = idx.entries.iter().min_by_key(|(_, e)| e.last_used).map(|(k, _)| *k) else { break };
                            if let Some(e) = idx.entries.remove(&victim) {
                                idx.bytes -= e.bytes;
                            }
                            let _ = std::fs::remove_file(path_for(&dir, victim));
                        }
                    }
                    Err(e) => {
                        log::warn!("disk cache write {}: {e}", path.display());
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            Job::RemoveSource(source) => {
                let victims: Vec<CacheKey> = {
                    let mut idx = index.lock();
                    let keys: Vec<CacheKey> = idx.entries.keys().filter(|(s, _)| *s == source).copied().collect();
                    for k in &keys {
                        if let Some(e) = idx.entries.remove(k) {
                            idx.bytes -= e.bytes;
                        }
                    }
                    keys
                };
                for k in victims {
                    let _ = std::fs::remove_file(path_for(&dir, k));
                }
            }
        }
    }
}

fn write_frame(path: &Path, f: &Frame) -> std::io::Result<usize> {
    let mut w = BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?);
    let kind: u8 = match f.pixels {
        Pixels::Half(_) => 0,
        Pixels::Byte(_) => 1,
    };
    w.write_all(MAGIC)?;
    w.write_all(&f.width.to_le_bytes())?;
    w.write_all(&f.height.to_le_bytes())?;
    w.write_all(&[kind, f.has_alpha as u8])?;
    w.write_all(&f.pixel_aspect.to_le_bytes())?;
    match &f.pixels {
        Pixels::Half(p) => w.write_all(bytemuck_f16(p))?,
        Pixels::Byte(p) => w.write_all(p)?,
    }
    w.flush()?;
    Ok(18 + f.byte_size())
}

fn read_frame(path: &Path) -> std::io::Result<Frame> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let mut file = std::fs::File::open(path)?;
    let mut head = [0u8; 18];
    file.read_exact(&mut head)?;
    if &head[0..4] != MAGIC {
        return Err(bad("not a plate cache file"));
    }
    let width = u32::from_le_bytes(head[4..8].try_into().unwrap());
    let height = u32::from_le_bytes(head[8..12].try_into().unwrap());
    let (kind, has_alpha) = (head[12], head[13] != 0);
    let pixel_aspect = f32::from_le_bytes(head[14..18].try_into().unwrap());
    let n = width as usize * height as usize * 4;
    let pixels = match kind {
        0 => {
            let mut v = vec![f16::ZERO; n];
            file.read_exact(bytemuck_f16_mut(&mut v))?;
            Pixels::Half(v)
        }
        1 => {
            let mut v = vec![0u8; n];
            file.read_exact(&mut v)?;
            Pixels::Byte(v)
        }
        _ => return Err(bad("unknown pixel kind")),
    };
    Ok(Frame { width, height, pixel_aspect, has_alpha, pixels })
}

fn bytemuck_f16(v: &[f16]) -> &[u8] {
    // SAFETY: f16 is a plain 2-byte value with no padding.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), v.len() * 2) }
}

fn bytemuck_f16_mut(v: &mut [f16]) -> &mut [u8] {
    // SAFETY: every bit pattern is a valid f16.
    unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr().cast(), v.len() * 2) }
}

/// Removes session folders whose process is gone (crash, kill -9).
fn sweep_stale(location: &Path) {
    let Ok(rd) = std::fs::read_dir(location) else { return };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(pid) = name.strip_prefix(SESSION_PREFIX).and_then(|p| p.parse::<u32>().ok()) else { continue };
        let alive = Path::new(&format!("/proc/{pid}")).exists() && pid != std::process::id();
        if !alive {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_cleans_up() {
        let loc = std::env::temp_dir().join(format!("plate-disk-test-{}", std::process::id()));
        let mut f = Frame::new(4, 2, true);
        f.half_mut()[5] = f16::from_f32(0.25);
        let mut b = Frame::new_u8(3, 1, false);
        b.byte_mut()[2] = 200;
        let dir;
        {
            let tier = DiskTier::new(&loc, 1 << 20).unwrap();
            dir = tier.dir().to_path_buf();
            tier.spill((1, 7), Arc::new(f));
            tier.spill((1, 8), Arc::new(b));
            // Writes are asynchronous.
            for _ in 0..200 {
                if tier.contains((1, 7)) && tier.contains((1, 8)) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let back = tier.load((1, 7)).unwrap();
            assert_eq!(back.sample(1, 0).unwrap()[1], 0.25);
            assert!(back.has_alpha);
            assert_eq!(tier.load((1, 8)).unwrap().sample(0, 0).unwrap()[2], 200.0 / 255.0);
            assert!(tier.load((2, 0)).is_none());
        }
        assert!(!dir.exists(), "session folder removed on drop");
        let _ = std::fs::remove_dir_all(&loc);
    }
}
