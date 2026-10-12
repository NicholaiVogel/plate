//! Headless subcommands: `plate preview`, `plate info`, `plate still`.
//! Each returns a process exit code.

use crate::ipc::{self, OpenRequest};
use crate::parse_arg;
use plate_core::{
    color::ColorManager,
    display,
    media::{self, MediaKind},
    notes::Notes,
    prefs::{ColorChoice, Prefs},
};
use std::path::{Path, PathBuf};

fn absolute(arg: &str) -> Option<PathBuf> {
    let p = parse_arg(arg)?;
    Some(p.canonicalize().unwrap_or(p))
}

/// Hands media to the running plate, or starts one detached from this shell.
pub fn preview(paths: &[String], frame: Option<i64>, compare: Option<&str>, new: bool) -> i32 {
    let req = OpenRequest {
        paths: paths.iter().filter_map(|a| absolute(a)).collect(),
        frame,
        compare: compare.and_then(absolute),
    };
    for p in req.paths.iter().chain(req.compare.iter()) {
        if !p.exists() {
            eprintln!("plate: {} not found", p.display());
            return 1;
        }
    }
    if !new {
        match ipc::send(&req) {
            Ok(true) => {
                println!("plate: showing {} in the open window", names(&req.paths));
                return 0;
            }
            Ok(false) => {}
            Err(e) => {
                eprintln!("plate: {e}");
                return 1;
            }
        }
    }
    match spawn_detached(&req, new) {
        Ok(pid) => {
            println!("plate: opened {} (pid {pid})", names(&req.paths));
            0
        }
        Err(e) => {
            eprintln!("plate: couldn't start: {e}");
            1
        }
    }
}

fn names(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>().join(", ")
}

/// Starts the GUI in its own session with output going to a log file, so the
/// caller (a terminal, an agent's shell) returns at once and can exit freely.
fn spawn_detached(req: &OpenRequest, new: bool) -> std::io::Result<u32> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    let log_dir = dirs_state().join("plate");
    std::fs::create_dir_all(&log_dir)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("plate.log"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(&req.paths);
    if let Some(f) = req.frame {
        cmd.arg("--frame").arg(f.to_string());
    }
    if let Some(c) = &req.compare {
        cmd.arg("--compare").arg(c);
    }
    if new {
        cmd.arg("--new");
    }
    cmd.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    Ok(cmd.spawn()?.id())
}

fn dirs_state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
}

/// The colorspace/display/view plate would use for this media.
fn color_for(cm: &ColorManager, prefs: &Prefs, info: &media::MediaInfo) -> (ColorChoice, bool) {
    match prefs.remembered(&cm.memory_key(), &info.format.id()) {
        Some(c) if cm.validate(c) => (c.clone(), true),
        _ => (cm.default_choice(info), false),
    }
}

pub fn info(path: &str, json: bool) -> i32 {
    let Some(p) = absolute(path) else { return 1 };
    let source = match media::open(&p) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("plate: {e:#}");
            return 1;
        }
    };
    let info = source.info().clone();
    let prefs = Prefs::load();
    let (cm, _) = ColorManager::load(&prefs.ocio);
    let (choice, remembered) = color_for(&cm, &prefs, &info);
    let notes = Notes::load(&info);
    let open_notes = notes.comments.iter().filter(|c| !c.done).count();
    let last = info.first_frame + info.frame_count as i64 - 1;
    let kind = match info.kind {
        MediaKind::Video => "movie",
        MediaKind::Sequence => "sequence",
        MediaKind::Still => "still",
        MediaKind::Dcp => "dcp",
    };
    if json {
        let v = serde_json::json!({
            "path": info.path,
            "name": info.name,
            "kind": kind,
            "width": info.width,
            "height": info.height,
            "pixel_aspect": info.pixel_aspect,
            "fps": info.fps,
            "frames": info.frame_count,
            "first_frame": info.first_frame,
            "last_frame": last,
            "format": info.format.id(),
            "detail": info.detail,
            "alpha": info.has_alpha,
            "audio": info.has_audio,
            "timecode": info.timecode,
            "color": {
                "colorspace": choice.colorspace,
                "display": choice.display,
                "view": choice.view,
                "remembered": remembered,
                "config": cm.label,
            },
            "comments": { "total": notes.comments.len(), "open": open_notes, "sidecar": info.notes_path },
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        println!("{}", info.name);
        println!("  {kind}  {}×{}  {:.3} fps  frames {}–{} ({})", info.width, info.height, info.fps, info.first_frame, last, info.frame_count);
        println!("  {}  {}{}{}", info.format.label(), info.detail, if info.has_alpha { "  alpha" } else { "" }, if info.has_audio { "  audio" } else { "" });
        if let Some(tc) = &info.timecode {
            println!("  timecode {tc}");
        }
        println!(
            "  color  {} → {} / {}  ({})",
            choice.colorspace,
            choice.display,
            choice.view,
            if remembered { "remembered for this format" } else { "default" }
        );
        println!("  comments  {} open, {} total", open_notes, notes.comments.len());
    }
    0
}

#[allow(clippy::too_many_arguments)]
pub fn still(
    path: &str,
    frame: Option<i64>,
    out: Option<PathBuf>,
    colorspace: Option<String>,
    display_name: Option<String>,
    view: Option<String>,
    exposure: f32,
    annotations: bool,
) -> i32 {
    let run = || -> anyhow::Result<PathBuf> {
        let p = absolute(path).ok_or_else(|| anyhow::anyhow!("bad path"))?;
        let source = media::open(&p)?;
        let info = source.info().clone();
        let label = frame.unwrap_or(info.first_frame);
        let last = info.first_frame + info.frame_count as i64 - 1;
        if label < info.first_frame || label > last {
            anyhow::bail!("frame {label} is outside {}-{last}", info.first_frame);
        }
        let index = (label - info.first_frame) as u64;
        let prefs = Prefs::load();
        let (cm, _) = ColorManager::load(&prefs.ocio);
        let (mut choice, _) = color_for(&cm, &prefs, &info);
        if let Some(cs) = colorspace {
            choice.colorspace = cs;
        }
        if let Some(d) = display_name {
            choice.view = cm.config.default_view(&d);
            choice.display = d;
        }
        if let Some(v) = view {
            choice.view = v;
        }
        if !cm.validate(&choice) {
            anyhow::bail!("{} / {} / {} isn't valid in {}", choice.colorspace, choice.display, choice.view, cm.label);
        }
        let frame = source.reader()?.read(index)?;
        let lut = cm.display_lut(&choice, exposure, 1.0)?;
        let image = display::render(&frame, &lut, display::Channel::Rgb);
        let shapes = if annotations {
            Notes::load(&info).comments.iter().filter(|c| c.frame == index).flat_map(|c| c.shapes.clone()).collect()
        } else {
            Vec::new()
        };
        let out = out.unwrap_or_else(|| {
            let stem = info.name.split([' ', '#', '[']).next().unwrap_or("frame").trim_end_matches(['.', '_']).to_string();
            let stem = Path::new(&stem).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(stem);
            PathBuf::from(format!("{stem}_{label}.png"))
        });
        plate_core::bake::save_still(&out, &image, &shapes, Some(crate::assets::BAKE_FONT))?;
        Ok(out.canonicalize().unwrap_or(out))
    };
    match run() {
        Ok(p) => {
            println!("{}", p.display());
            0
        }
        Err(e) => {
            eprintln!("plate: {e:#}");
            1
        }
    }
}
