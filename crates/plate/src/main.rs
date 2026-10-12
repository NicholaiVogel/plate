//! plate: a color-managed player for movies, image sequences and DCPs.
//!
//! Usage: plate [FILE | FOLDER | DCP]...

mod actions;
mod app;
mod assets;
mod cli;
mod demo;
mod ipc;
#[cfg(target_os = "macos")]
mod macos;
mod text_input;
mod theme;
mod ui;

use app::Plate;
use futures::StreamExt;
use gpui::{
    point, px, size, AppContext, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions,
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
    let application = Application::new().with_assets(assets::Assets);
    let (open_tx, mut open_rx) = futures::channel::mpsc::unbounded::<Vec<String>>();
    // Finder events can arrive before the first window is ready.
    application.on_open_urls(move |urls| { let _ = open_tx.unbounded_send(urls); });
    application.on_reopen(|cx| {
        cx.activate(true);
        for window in cx.windows() {
            let _ = window.update(cx, |_, window, _| window.activate_window());
        }
    });
    application.run(move |cx| {
        let fonts = assets::FONTS.iter().map(|f| Cow::Borrowed(*f)).collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            log::warn!("loading fonts: {e}");
        }
        text_input::bind_keys(cx);
        actions::bind_keys(cx);
        let has_sf = {
            let names = cx.text_system().all_font_names();
            names.iter().any(|n| n == theme::SF_PRO) && names.iter().any(|n| n == theme::SF_MONO)
        };
        theme::SF_AVAILABLE.store(has_sf, std::sync::atomic::Ordering::Relaxed);
        theme::apply_font_pref(&plate_core::prefs::Prefs::load().ui_font);
        #[cfg(target_os = "macos")]
        {
            use gpui::{Menu, MenuItem, SystemMenuType};
            cx.on_action(|_: &actions::Quit, cx| cx.quit());
            cx.set_menus(vec![
                Menu { name: "Plate".into(), items: vec![
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Quit Plate", actions::Quit),
                ]},
                Menu { name: "File".into(), items: vec![
                    MenuItem::action("Open…", actions::OpenFile),
                    MenuItem::action("Open Folder…", actions::OpenFolder),
                    MenuItem::action("Save Frame…", actions::SaveFrame),
                ]},
                Menu { name: "Edit".into(), items: vec![
                    MenuItem::action("Undo Mark", actions::UndoShape),
                    MenuItem::separator(),
                    MenuItem::action("Cut", text_input::Cut),
                    MenuItem::action("Copy", text_input::Copy),
                    MenuItem::action("Paste", text_input::Paste),
                    MenuItem::action("Select All", text_input::SelectAll),
                ]},
            ]);
        }

        let bounds = Bounds::centered(None, size(px(1600.), px(960.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("plate".into()),
                        appears_transparent: cfg!(target_os = "macos"),
                        traffic_light_position: cfg!(target_os = "macos").then(|| point(px(12.), px(17.))),
                    }),
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
        cx.spawn(async move |cx| {
            while let Some(urls) = open_rx.next().await {
                let paths = urls.iter().filter_map(|url| parse_arg(url)).collect();
                let _ = window.update(cx, |app, window, cx| {
                    app.open_paths(paths, cx);
                    window.activate_window();
                    cx.notify();
                });
            }
        }).detach();
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
