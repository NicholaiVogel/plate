//! Times how long the loader takes to cache a whole clip from a given playhead.
//! Usage: fill <path> [playhead]
use plate_core::{cache::FrameCache, loader::Loader, media};
use std::sync::Arc;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("path");
    let playhead: u64 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let src = media::open(std::path::Path::new(&path))?;
    let info = src.info().clone();
    let cache = Arc::new(FrameCache::new(16 << 30));
    let t = Instant::now();
    let loader = Loader::new(src, cache.clone(), |_| {});
    loader.set_window(info.frame_count);
    loader.set_playhead(playhead, 0);
    let mut first = None;
    loop {
        let done = cache.cached(info.id, info.frame_count).iter().filter(|c| **c).count();
        if first.is_none() && done > 0 {
            first = Some(t.elapsed());
        }
        if done as u64 == info.frame_count {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    println!(
        "{} frames {}x{}: first frame {:.0} ms, all cached {:.0} ms, {:.2} GB",
        info.frame_count, info.width, info.height,
        first.unwrap().as_secs_f64() * 1e3, t.elapsed().as_secs_f64() * 1e3,
        cache.used() as f64 / (1u64 << 30) as f64
    );
    Ok(())
}
