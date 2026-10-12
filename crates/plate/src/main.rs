//! plate: a color-managed player for movies, image sequences and DCPs.
//!
//! Usage: plate [FILE | FOLDER | DCP]...

mod actions;
mod app;
mod assets;
mod demo;
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
fn parse_arg(arg: &str) -> Option<PathBuf> {
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

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,plate=info,plate_core=info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("plate {}\n\nUsage: plate [FILE | FOLDER | DCP]...\n\nPress ? in the app for shortcuts.", env!("CARGO_PKG_VERSION"));
        return;
    }
    let paths: Vec<PathBuf> = args.iter().filter_map(|a| parse_arg(a)).collect();

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
                        app.open_paths(paths, cx);
                        demo::schedule(window, cx);
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
        cx.activate(true);
    });
}
