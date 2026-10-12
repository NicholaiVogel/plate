//! Sequential read throughput of one movie reader.
use plate_core::media::FrameReader;
use plate_core::source::video::VideoReader;
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("path");
    let n: u64 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(341);
    let mut r = VideoReader::open(std::path::Path::new(&path), 0, n)?;
    let t = Instant::now();
    for i in 0..n {
        r.read(i)?;
    }
    println!("{n} frames in {:.0} ms ({:.0} fps)", t.elapsed().as_secs_f64() * 1e3, n as f64 / t.elapsed().as_secs_f64());
    Ok(())
}
