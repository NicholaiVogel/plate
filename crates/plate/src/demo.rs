//! `PLATE_DEMO=<state>` puts the app into a given state shortly after start,
//! so UI states can be screenshotted without synthesizing input.
//! `PLATE_STATS=1` logs presented frames per second during playback.

use crate::app::{CompareMode, Modal, Plate, Tool};
use gpui::{point, px, Context, Window};
use plate_core::notes::{Comment, Shape};
use std::time::Duration;

pub fn schedule(window: &mut Window, cx: &mut Context<Plate>) {
    #[cfg(target_os = "macos")]
    if std::env::var_os("PLATE_CHECK_SCRUB").is_some() {
        // Exercise the painted timeline's actual GPUI event handlers. This
        // runs in a real window without requiring OS input permissions.
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let (bounds, count) = this.update_in(cx, |app, _, _| {
                (app.timeline_bounds.get(), app.clip.as_ref().expect("test clip").info.frame_count)
            }).unwrap();
            assert!(count > 1, "scrubbing check requires a movie or image sequence");
            cx.update(|window, _| crate::macos::check_header(window)).unwrap();
            assert!(f32::from(bounds.size.width) > 100.0, "timeline must be painted");
            let start = point(bounds.origin.x + bounds.size.width * 0.1, bounds.center().y);
            let end = point(bounds.origin.x + bounds.size.width * 0.75, bounds.center().y);
            let outside = cx.update(|window, _| point(window.viewport_size().width + px(100.), start.y)).unwrap();
            let before = point(px(-100.), start.y);
            for (end, expected) in [(end, (count as f64 * 0.75).floor() as u64), (outside, count - 1), (before, 0)] {
                // NSEventType: moved, left down, left dragged, left up.
                for (event_type, position) in [(5, start), (1, start), (6, end), (2, end)] {
                    cx.update(|window, _| crate::macos::check_mouse_event(window, event_type, position)).unwrap();
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                }
                this.update_in(cx, |app, _, _| {
                    assert_eq!(app.frame, expected, "drag must seek to the cursor's frame, including outside the window");
                    assert!(matches!(app.drag, crate::app::Drag::None), "release must end the scrub");
                    assert!(!app.is_playing(), "scrubbing must pause playback");
                    println!("PASS: timeline drag seeks to frame {} of {count}", app.frame);
                }).unwrap();
            }
            cx.update(|_, cx| cx.quit()).unwrap();
        }).detach();
    }
    let Ok(state) = std::env::var("PLATE_DEMO") else { return };
    // Steps run in order; "wait:<ms>" pauses between them.
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(Duration::from_millis(1500)).await;
        for step in state.split(',').map(str::to_string).collect::<Vec<_>>() {
            if let Some(ms) = step.strip_prefix("wait:").and_then(|v| v.parse::<u64>().ok()) {
                cx.background_executor().timer(Duration::from_millis(ms)).await;
                continue;
            }
            let ok = this.update_in(cx, |app, window, cx| {
                apply(app, &step, window, cx);
                cx.notify();
            });
            if ok.is_err() {
                break;
            }
        }
    })
    .detach();
}

fn apply(app: &mut Plate, state: &str, window: &mut Window, cx: &mut Context<Plate>) {
    for step in state.split(',') {
        let (name, arg) = step.split_once(':').unwrap_or((step, ""));
        match name {
            "play" => app.play(1, 1.0),
            "seek" => app.seek(arg.parse().unwrap_or(0)),
            "modal" => {
                let m = match arg {
                    "colorspace" => Modal::Colorspace,
                    "display" => Modal::Display,
                    "channel" => Modal::Channel,
                    "compare" => Modal::Compare,
                    "settings" => Modal::Settings,
                    "help" => Modal::Help,
                    "guides" => Modal::Guides,
                    "layers" => Modal::Layers,
                    "versions" => Modal::Versions,
                    "captions" => Modal::Captions,
                    _ => continue,
                };
                app.open_modal(m, window, cx);
            }
            "draw" => {
                app.drawing = true;
                app.tool = Tool::Arrow;
                app.draft_frame = Some(app.frame);
                app.draft.push(Shape::Arrow { from: [0.2, 0.25], to: [0.42, 0.45], color: [0xff, 0x4d, 0x4d], width: 0.004 });
                app.draft.push(Shape::Ellipse { from: [0.45, 0.4], to: [0.62, 0.65], color: [0xff, 0xd2, 0x3f], width: 0.004 });
                app.draft.push(Shape::Pen {
                    points: (0..40).map(|i| [0.65 + i as f32 * 0.005, 0.3 + (i as f32 * 0.3).sin() * 0.03]).collect(),
                    color: [0x3d, 0xdc, 0x84],
                    width: 0.004,
                });
                app.draft.push(Shape::Text { at: [0.22, 0.2], text: "roto edge chatter".into(), color: [0xff, 0x4d, 0x4d], size: 0.03 });
            }
            "comments" => {
                if let Some(clip) = app.clip.as_mut() {
                    clip.notes.comments.push(Comment::new(3, "Edge of the matte is chattering on the left".into(), "director".into(), vec![
                        Shape::Rect { from: [0.1, 0.1], to: [0.3, 0.4], color: [0xff, 0x4d, 0x4d], width: 0.004 },
                    ]));
                    clip.notes.comments.push(Comment::new(20, "Grade looks good here".into(), "reviewer".into(), vec![]));
                    let mut done = Comment::new(30, "Remove the boom mic".into(), "director".into(), vec![]);
                    done.done = true;
                    clip.notes.comments.push(done);
                }
                app.seek(3);
                app.focused_comment = app.clip.as_ref().and_then(|c| c.notes.comments.first().map(|c| c.id.clone()));
            }
            "compare" => {
                if let Ok(s) = plate_core::media::open(std::path::Path::new(arg)) {
                    app.set_compare(Some(s), cx);
                }
            }
            "side" => app.compare_mode = CompareMode::SideBySide,
            "probe" => {
                app.probe_on = true;
                let b = app.viewer_bounds.get();
                let pos = point(b.origin.x + b.size.width * 0.4, b.origin.y + b.size.height * 0.5);
                app.demo_probe(pos, window.scale_factor());
            }
            "channel" => app.set_channel(match arg {
                "r" => plate_core::display::Channel::Red,
                "a" => plate_core::display::Channel::Alpha,
                _ => plate_core::display::Channel::Luma,
            }),
            "exposure" => app.set_exposure(arg.parse().unwrap_or(1.0)),
            "gamma" => app.set_gamma(arg.parse().unwrap_or(1.0)),
            "zoom" => app.zoom_at(arg.parse().unwrap_or(2.0), None, window.scale_factor()),
            "pan" => app.pan = point(px(120.), px(40.)),
            "inout" => {
                app.seek(10);
                app.set_in();
                app.seek(30);
                app.set_out();
                app.seek(14);
            }
            "choose" => {
                if let Some(c) = &app.clip {
                    let choice = plate_core::prefs::ColorChoice { colorspace: arg.to_string(), ..c.choice.clone() };
                    app.set_choice(choice);
                }
            }
            "post" => {
                app.draft_frame = Some(app.frame);
                app.draft.push(Shape::Rect { from: [0.3, 0.3], to: [0.5, 0.5], color: [0xff, 0x4d, 0x4d], width: 0.004 });
                app.submit_comment(arg.to_string());
            }
            "scale" => app.set_ui_scale(arg.parse().unwrap_or(1.4), cx),
            "bounds" => log::info!(
                "viewer {:?} timeline {:?} window {:?}",
                app.viewer_bounds.get(),
                app.timeline_bounds.get(),
                window.bounds().size
            ),
            "quit" => cx.quit(),
            "root" => {
                let p = std::path::Path::new(arg).canonicalize().unwrap_or_else(|_| arg.into());
                app.set_root(&p, cx);
            }
            "expand" => {
                let p = std::path::Path::new(arg).canonicalize().unwrap_or_else(|_| arg.into());
                app.toggle_folder(&p);
            }
            "older" => app.step_version(-1, cx),
            "newer" => app.step_version(1, cx),
            "compare_prev" => app.compare_previous(cx),
            "expand_all" => {
                if let Some(b) = &app.bin {
                    for e in &b.entries {
                        if let Some((k, _)) = plate_core::scan::version_of(&plate_core::scan::version_name(e)) {
                            app.expanded_stacks.insert(k);
                        }
                    }
                }
            }
            "layer" => app.switch_layer(arg),
            "wait" => {}
            "toast" => app.show_toast("Saved /tmp/frame_1012.png".into()),
            _ => log::warn!("unknown demo step {name}"),
        }
    }
}
