//! Opens media, decodes a few frames through the full display pipeline, and
//! writes PNGs. Usage: cargo run --release -p plate-core --example probe -- <out_dir> <paths...>

use plate_core::{color::ColorManager, display, media, prefs::OcioChoice};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    std::fs::create_dir_all(&out)?;
    let (cm, warn) = ColorManager::load(&OcioChoice::Builtin);
    if let Some(w) = warn {
        eprintln!("warning: {w}");
    }
    for path in args {
        let t = Instant::now();
        let src = match media::open(std::path::Path::new(&path)) {
            Ok(s) => s,
            Err(e) => {
                println!("{path}: OPEN FAILED: {e:#}");
                continue;
            }
        };
        let info = src.info().clone();
        let choice = cm.default_choice(&info);
        println!(
            "{}\n  kind={:?} {}x{} frames={} first={} fps={:.3} key={} ({}) alpha={} audio={} tc={:?}\n  detail={}\n  color: {} -> {} / {}\n  notes={}\n  open {:.1}ms",
            info.name, info.kind, info.width, info.height, info.frame_count, info.first_frame, info.fps,
            info.format.id(), info.format.label(), info.has_alpha, info.has_audio, info.timecode, info.detail,
            choice.colorspace, choice.display, choice.view, info.notes_path.display(), t.elapsed().as_secs_f64() * 1e3
        );
        let proc = cm.display_lut(&choice, 0.0, 1.0)?;
        let mut reader = src.reader()?;
        let last = info.frame_count.saturating_sub(1);
        for idx in [0, last / 2, last, 1] {
            let t = Instant::now();
            let frame = match reader.read(idx) {
                Ok(f) => f,
                Err(e) => {
                    println!("  frame {idx}: FAILED {e:#}");
                    continue;
                }
            };
            let dt = t.elapsed();
            let t = Instant::now();
            let img = display::render(&frame, &proc, display::Channel::Rgb);
            let rt = t.elapsed();
            let mid = frame.sample(frame.width as i64 / 2, frame.height as i64 / 2).unwrap();
            println!(
                "  frame {idx}: decode {:.1}ms display {:.1}ms center raw {:.3?}",
                dt.as_secs_f64() * 1e3, rt.as_secs_f64() * 1e3, mid
            );
            if idx == last / 2 {
                let stem = info.name.replace(['/', ' ', '#', '[', ']'], "_");
                plate_core::bake::save_still(&out.join(format!("{stem}.png")), &img, &[], None)?;
            }
        }
        if !src.audio().is_empty() {
            println!("  audio segments: {:?}", src.audio().iter().map(|s| (s.path.as_ref().map(|p| p.file_name().unwrap().to_string_lossy().to_string()), s.source_offset, s.duration)).collect::<Vec<_>>());
        }
    }
    Ok(())
}
