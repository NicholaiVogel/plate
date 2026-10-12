//! Steady-state throughput of decode and display transform for one file.
use plate_core::{color::ColorManager, display, media, prefs::OcioChoice};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("path");
    let (cm, _) = ColorManager::load(&OcioChoice::Builtin);
    let src = media::open(std::path::Path::new(&path))?;
    let info = src.info();
    let mut r = src.reader()?;
    let n = info.frame_count.min(24);
    let t = Instant::now();
    let mut frames = Vec::new();
    for i in 0..n {
        frames.push(r.read(i)?);
    }
    let dec = t.elapsed().as_secs_f64() * 1e3 / n as f64;
    let choice = cm.default_choice(info);
    for (label, c) in [("default", choice.clone()), ("srgb-untonemapped", plate_core::prefs::ColorChoice { view: "Un-tone-mapped".into(), ..choice.clone() })] {
        let t0 = Instant::now();
        let p = cm.display_lut(&c, 0.0, 1.0)?;
        println!("{label:>18}: bake {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
        display::render(&frames[0], &p, display::Channel::Rgb);
        let t = Instant::now();
        for f in &frames {
            display::render(f, &p, display::Channel::Rgb);
        }
        let ms = t.elapsed().as_secs_f64() * 1e3 / frames.len() as f64;
        let mpix = (info.width * info.height) as f64 / 1e6;
        println!("{label:>18}: {} / {}  display {ms:.1} ms/frame ({:.0} Mpix/s)", c.colorspace, c.view, mpix / ms * 1e3);
    }
    println!("sequential decode {dec:.1} ms/frame ({}x{})", info.width, info.height);
    Ok(())
}
