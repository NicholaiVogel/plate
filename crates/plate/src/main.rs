//! plate: a color-managed player for movies, image sequences and DCPs.
//!
//! Usage: plate [FILE | FOLDER | DCP]...

mod actions;
mod app;
mod assets;
mod cli;
mod demo;
mod ipc;
mod text_input;
mod theme;
mod ui;

use app::Plate;
use gpui::{
    px, size, AppContext, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions,
};
use std::borrow::Cow;
use std::path::PathBuf;

/// Accepts plain paths and `file://` URIs (desktop launchers pass `%U`).
pub(crate) fn parse_arg(arg: &str) -> Option<PathBuf> {
    if let Some(rest) = arg.strip_prefix("file://") {
        let path = rest.trim_start_matches("localhost");
        return Some(PathBuf::from(percent_decode(path)));
    }
    if arg.starts_with('-') {
        return None;
    }
    Some(PathBuf::from(arg))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(clap::Parser)]
#[command(
    name = "plate",
    version,
    about = "Color-managed media player for VFX review",
    args_conflicts_with_subcommands = true,
    after_help = "Paths can be movies, any frame of an image sequence, folders (opened as a bin) or DCP folders.\n\
                  With a plate window already open, files are handed to it instead of opening a new one.\n\
                  Press ? in the app for keyboard shortcuts."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Media to open.
    paths: Vec<String>,
    /// Frame to show, as numbered in plate (e.g. 1012 for a comp sequence).
    #[arg(long)]
    frame: Option<i64>,
    /// Second clip for A/B compare.
    #[arg(long)]
    compare: Option<String>,
    /// Open a separate window instead of reusing the running plate.
    #[arg(long)]
    new: bool,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Show media in plate and return immediately: hands it to the running
    /// window (raising it), or starts plate in the background.
    Preview {
        #[arg(required = true)]
        paths: Vec<String>,
        /// Frame to show, as numbered in plate.
        #[arg(long)]
        frame: Option<i64>,
        /// Second clip for A/B compare.
        #[arg(long)]
        compare: Option<String>,
        /// Open a separate window instead of reusing the running plate.
        #[arg(long)]
        new: bool,
    },
    /// Print what plate sees in a file: format, range, rate, color, notes.
    Info {
        path: String,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Render one frame through plate's color pipeline to an image file.
    Still {
        path: String,
        /// Frame as numbered in plate; defaults to the first.
        #[arg(long)]
        frame: Option<i64>,
        /// Output .png, .jpg or .tif (default: <name>_<frame>.png here).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Input colorspace (default: what plate remembers for the format).
        #[arg(long)]
        colorspace: Option<String>,
        /// Display (default: remembered/config default).
        #[arg(long)]
        display: Option<String>,
        /// View (default: remembered/config default).
        #[arg(long)]
        view: Option<String>,
        /// Exposure in stops.
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        exposure: f32,
        /// Burn in the annotations saved on that frame.
        #[arg(long)]
        annotations: bool,
        /// EXR layer to render (see `plate info`).
        #[arg(long)]
        layer: Option<String>,
    },
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,plate=info,plate_core=info")).init();
    use clap::Parser;
    let cli = Cli::parse();
    let code = match cli.command {
        Some(Command::Preview { paths, frame, compare, new }) => cli::preview(&paths, frame, compare.as_deref(), new),
        Some(Command::Info { path, json }) => cli::info(&path, json),
        Some(Command::Still { path, frame, out, colorspace, display, view, exposure, annotations, layer }) => {
            cli::still(&path, frame, out, colorspace, display, view, exposure, annotations, layer)
        }
        None => {
            let req = ipc::OpenRequest {
                paths: cli.paths.iter().filter_map(|a| parse_arg(a)).collect(),
                frame: cli.frame,
                compare: cli.compare.as_deref().and_then(parse_arg),
            };
            // A double-clicked file goes to the plate that's already open.
            if !cli.new && !req.paths.is_empty() {
                match ipc::send(&req) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(e) => log::warn!("running plate refused: {e}"),
                }
            }
            run_gui(req);
            0
        }
    };
    std::process::exit(code);
}

fn run_gui(req: ipc::OpenRequest) {
    let requests = ipc::listen();
    let owns_socket = requests.is_some();
    Application::new().with_assets(assets::Assets).run(move |cx| {
        let fonts = assets::FONTS.iter().map(|f| Cow::Borrowed(*f)).collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            log::warn!("loading fonts: {e}");
        }
        text_input::bind_keys(cx);
        actions::bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1600.), px(960.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions { title: Some("plate".into()), ..Default::default() }),
                    // PLATE_APP_ID lets test harnesses target the window with
                    // compositor rules without touching normal sessions.
                    app_id: Some(std::env::var("PLATE_APP_ID").unwrap_or_else(|_| "plate".into())),
                    window_min_size: Some(size(px(760.), px(480.))),
                    window_background: WindowBackgroundAppearance::Opaque,
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| Plate::new(window, cx));
                    view.update(cx, |app, cx| {
                        if let Err(e) = app.apply_request(req, cx) {
                            app.show_toast(e);
                        }
                        demo::schedule(window, cx);
                        if let Some(mut rx) = requests {
                            use futures::StreamExt;
                            cx.spawn_in(window, async move |this, cx| {
                                while let Some((req, done)) = rx.next().await {
                                    let result = this
                                        .update_in(cx, |app, window, cx| {
                                            let r = app.apply_request(req, cx);
                                            window.activate_window();
                                            cx.notify();
                                            r
                                        })
                                        .unwrap_or_else(|_| Err("plate is closing".into()));
                                    let _ = done.send(result);
                                }
                            })
                            .detach();
                        }
                    });
                    view
                },
            )
            .expect("open window");
        let _ = window;
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.on_app_quit(move |_| {
            if owns_socket {
                ipc::cleanup();
            }
            async {}
        })
        .detach();
        cx.activate(true);
    });
}
