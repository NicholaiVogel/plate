use super::widgets::{icon_button, icon_button_colored, icon_sized, TipExt};
use crate::app::{CounterMode, Drag, Plate};
use crate::theme::{self, s, FONT_MONO};
use gpui::{canvas, div, fill, point, prelude::*, px, Bounds, Context, MouseButton, MouseDownEvent, Pixels, Window};

/// Shared with the comment composer so their top edges line up.
pub(super) const TRANSPORT_HEIGHT: f32 = 84.0;

impl Plate {
    pub(super) fn render_transport(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Clamped to the column: if the controls could push it wider, the
        // timeline would run under the comments panel and the playhead would
        // no longer sit under the cursor.
        div()
            .flex_none()
            .w_full()
            .min_w_0()
            .h(s(TRANSPORT_HEIGHT))
            .overflow_hidden()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme::hairline())
            .child(self.render_timeline(window, cx))
            .child(self.render_controls(window, cx))
    }

    pub(super) fn frame_at(&self, x: Pixels) -> Option<u64> {
        let clip = self.clip.as_ref()?;
        let b = self.timeline_bounds.get();
        let w = f32::from(b.size.width).max(1.0);
        let t = (f32::from(x - b.origin.x) / w).clamp(0.0, 1.0);
        let last = clip.info.frame_count.saturating_sub(1);
        Some(((t * clip.info.frame_count as f32).floor() as u64).min(last))
    }

    fn render_timeline(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds_cell = self.timeline_bounds.clone();
        let data = self.clip.as_ref().map(|clip| {
            let count = clip.info.frame_count.max(1);
            let cached = self.cache.cached(clip.info.id, count);
            let range = (clip.in_point.is_some() || clip.out_point.is_some()).then(|| clip.range());
            let mut marks: Vec<(u64, bool)> = clip.notes.comments.iter().map(|c| (c.frame, c.done)).collect();
            marks.dedup_by_key(|m| m.0);
            let draft = self.draft_frame;
            (count, cached, range, marks, draft)
        });
        let frame = self.frame;
        let focused_frame = self.clip.as_ref().and_then(|c| {
            let id = self.focused_comment.as_deref()?;
            c.notes.comments.iter().find(|x| x.id == id).map(|x| x.frame)
        });

        let painter = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _| {
                // The hit area is taller than the track; the track itself is thin.
                bounds_cell.set(bounds);
                let Some((count, cached, range, marks, draft)) = data else {
                    let track = Bounds { origin: point(bounds.origin.x, bounds.center().y - s(1.)), size: gpui::size(bounds.size.width, s(2.)) };
                    window.paint_quad(fill(track, theme::hairline()));
                    return;
                };
                let w = f32::from(bounds.size.width);
                let x_of = |f: u64| bounds.origin.x + px(w * f as f32 / count as f32);
                let frame_w = (w / count as f32).max(1.0);
                let track_y = bounds.origin.y + s(14.);
                let track = Bounds { origin: point(bounds.origin.x, track_y), size: gpui::size(bounds.size.width, s(4.)) };
                window.paint_quad(fill(track, theme::hairline()).corner_radii(s(2.)));

                // Cached frames, merged into runs.
                let mut i = 0usize;
                while i < cached.len() {
                    if cached[i] {
                        let start = i;
                        while i < cached.len() && cached[i] {
                            i += 1;
                        }
                        let r = Bounds {
                            origin: point(x_of(start as u64), track_y),
                            size: gpui::size(px((w * (i - start) as f32 / count as f32).max(1.0)), s(4.)),
                        };
                        window.paint_quad(fill(r, theme::blue_soft()).corner_radii(s(2.)));
                    } else {
                        i += 1;
                    }
                }
                // Played portion.
                let played = Bounds { origin: point(bounds.origin.x, track_y + s(1.)), size: gpui::size(x_of(frame) - bounds.origin.x, s(2.)) };
                window.paint_quad(fill(played, theme::blue()).corner_radii(s(1.)));

                if let Some((a, b)) = range {
                    let r = Bounds::from_corners(point(x_of(a), track_y - s(5.)), point(x_of(b + 1), track_y - s(3.)));
                    window.paint_quad(fill(r, theme::blue()).corner_radii(s(1.)));
                    for (x, left) in [(x_of(a), true), (x_of(b + 1), false)] {
                        let tick = Bounds { origin: point(if left { x } else { x - s(2.) }, track_y - s(7.)), size: gpui::size(s(2.), s(13.)) };
                        window.paint_quad(fill(tick, theme::blue()));
                    }
                }
                for (f, done) in marks {
                    let cx = x_of(f) + px(frame_w / 2.);
                    let size = if Some(f) == focused_frame { 9. } else { 7. };
                    let m = Bounds::centered_at(point(cx, track_y + s(13.)), gpui::size(s(size), s(size)));
                    let color = if done { theme::ink_faint() } else { theme::warn() };
                    window.paint_quad(fill(m, color).corner_radii(s(size / 2.)));
                }
                if let Some(f) = draft {
                    let m = Bounds::centered_at(point(x_of(f) + px(frame_w / 2.), track_y + s(13.)), gpui::size(s(7.), s(7.)));
                    window.paint_quad(fill(m, theme::ink()).corner_radii(s(3.5)));
                }
                // Playhead: the frame's slot, faint, with a crisp line and knob.
                if frame_w >= 4.0 {
                    let slot = Bounds { origin: point(x_of(frame), track_y - s(4.)), size: gpui::size(px(frame_w), s(12.)) };
                    window.paint_quad(fill(slot, theme::hover()));
                }
                let x = x_of(frame) + px((frame_w / 2.0).min(frame_w - 1.0).max(0.0));
                let line = Bounds { origin: point(x - s(1.), bounds.origin.y + s(6.)), size: gpui::size(s(2.), s(20.)) };
                window.paint_quad(fill(line, theme::ink()));
                let knob = Bounds::centered_at(point(x, bounds.origin.y + s(6.)), gpui::size(s(10.), s(6.)));
                window.paint_quad(fill(knob, theme::ink()).corner_radii(s(1.)));
            },
        )
        .size_full();

        div()
            .id("timeline")
            .h(s(32.))
            .mx(s(14.))
            .mt(s(4.))
            .cursor(gpui::CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                this.focus_viewer(window);
                if let Some(f) = this.frame_at(ev.position.x) {
                    this.stop();
                    this.drag = Drag::Scrub;
                    this.seek(f);
                    cx.notify();
                }
            }))
            .child(painter)
    }

    fn render_controls(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playing = self.is_playing();
        let total = self.clip.as_ref().map(|c| c.info.frame_count).unwrap_or(0);
        let counter = if self.clip.is_some() {
            let last = total.saturating_sub(1);
            format!("{}  /  {}", self.frame_text(self.frame), self.frame_text(last))
        } else {
            "–  /  –".into()
        };
        let speed = if self.playback.dir < 0 {
            format!("−{:.1}×", self.playback.speed)
        } else {
            format!("{:.1}×", self.playback.speed)
        };
        let has_audio = self.audio.has_track();
        let muted = self.prefs.muted;
        let volume = self.prefs.volume;
        let range = self.clip.as_ref().and_then(|c| {
            (c.in_point.is_some() || c.out_point.is_some()).then(|| {
                let (a, b) = c.range();
                format!("IN {}  OUT {}", self.frame_text(a), self.frame_text(b))
            })
        });
        let comments_open = self.prefs.show_comments;

        div()
            .h(s(46.))
            .px(s(10.))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(s(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(s(2.))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(icon_button("step-back", "step-back", false).tip("Previous frame  (←)").on_click(cx.listener(|this, _, _, cx| {
                        this.step(-1);
                        cx.notify();
                    })))
                    .child(
                        icon_button_colored("play", if playing { "pause" } else { "play" }, false, theme::ink())
                            .tip("Play / pause  (Space)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_play();
                                cx.notify();
                            })),
                    )
                    .child(icon_button("step-forward", "step-forward", false).tip("Next frame  (→)").on_click(cx.listener(|this, _, _, cx| {
                        this.step(1);
                        cx.notify();
                    })))
                    .child(
                        icon_button("loop", "loop", self.prefs.loop_playback).tip("Loop  (Shift L)").on_click(cx.listener(|this, _, _, cx| {
                            this.prefs.loop_playback = !this.prefs.loop_playback;
                            this.prefs.save();
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .id("speed")
                            .h(s(30.))
                            .px(s(8.))
                            .flex()
                            .items_center()
                            .rounded(s(theme::RADIUS))
                            .cursor_pointer()
                            .font_family(FONT_MONO)
                            .text_size(s(12.))
                            .text_color(theme::ink_muted())
                            .hover(|s| s.bg(theme::hover()).text_color(theme::ink()))
                            .child(speed)
                            .tip("Speed  (J / L to change)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let next = match this.playback.speed {
                                    s if s < 0.3 => 0.5,
                                    s if s < 0.6 => 1.0,
                                    s if s < 1.5 => 2.0,
                                    _ => 0.25,
                                };
                                let dir = if this.playback.dir == 0 { 1 } else { this.playback.dir };
                                if this.is_playing() {
                                    this.play(dir, next);
                                } else {
                                    this.playback.speed = next;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button("mute", if muted || !has_audio { "volume-x" } else { "volume" }, false)
                            .when(!has_audio, |d| d.opacity(0.4))
                            .tip(if has_audio { "Mute  (M) · scroll for volume" } else { "No audio" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prefs.muted = !this.prefs.muted;
                                this.audio.set_muted(this.prefs.muted);
                                this.prefs.save();
                                cx.notify();
                            }))
                            .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _, cx| {
                                let dy = f32::from(ev.delta.pixel_delta(s(20.)).y);
                                this.prefs.volume = (this.prefs.volume - dy / 400.0).clamp(0.0, 1.5);
                                this.audio.set_volume(this.prefs.volume);
                                cx.notify();
                            })),
                    )
                    .when(has_audio && !muted, |d| {
                        d.child(
                            div()
                                .w(s(44.))
                                .h(s(3.))
                                .bg(theme::hairline())
                                .child(div().h_full().w(s(44. * (volume / 1.5).clamp(0.0, 1.0))).bg(theme::ink_muted())),
                        )
                    }),
            )
            .child(
                div()
                    .id("counter")
                    .flex()
                    .items_center()
                    .gap(s(10.))
                    .px(s(10.))
                    .h(s(30.))
                    .rounded(s(theme::RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(div().font_family(FONT_MONO).text_size(s(13.)).child(counter))
                    .child(
                        div().font_family(FONT_MONO).text_size(s(10.)).text_color(theme::ink_faint()).child(match self.counter {
                            CounterMode::Frames => "FRAME",
                            CounterMode::Timecode => "TC",
                            CounterMode::Index => "COUNT",
                        }),
                    )
                    .child(icon_sized("chevron-down", 12.).text_color(theme::ink_faint()))
                    .tip("Frames · timecode · count")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.counter = match this.counter {
                            CounterMode::Frames => CounterMode::Timecode,
                            CounterMode::Timecode => CounterMode::Index,
                            CounterMode::Index => CounterMode::Frames,
                        };
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .justify_end()
                    .items_center()
                    .gap(s(2.))
                    .children(range.map(|r| {
                        div()
                            .id("range")
                            .mr(s(8.))
                            .font_family(FONT_MONO)
                            .text_size(s(11.))
                            .text_color(theme::blue())
                            .cursor_pointer()
                            .child(r)
                            .tip("Clear range  (U)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_in_out();
                                cx.notify();
                            }))
                    }))
                    .child(
                        icon_button("guides", "grid", self.prefs.guides.visible && self.prefs.guides.any())
                            .tip("Guides  (' toggles)")
                            .on_click(cx.listener(|this, _, window, cx| this.open_modal(crate::app::Modal::Guides, window, cx))),
                    )
                    .child(
                        icon_button("captions", "captions", self.captions.is_some() && self.prefs.captions.visible)
                            .tip("Captions  (S toggles)")
                            .on_click(cx.listener(|this, _, window, cx| this.open_modal(crate::app::Modal::Captions, window, cx))),
                    )
                    .child(icon_button("draw", "pencil", self.drawing).tip("Annotate  (D)").on_click(cx.listener(|this, _, window, cx| {
                        this.drawing = !this.drawing;
                        if this.drawing {
                            this.stop();
                            this.probe_on = false;
                        }
                        this.focus_viewer(window);
                        cx.notify();
                    })))
                    .child(icon_button("probe", "crosshair", self.probe_on).tip("Pixel probe  (P)").on_click(cx.listener(|this, _, window, cx| {
                        this.probe_on = !this.probe_on;
                        if !this.probe_on {
                            this.probe = None;
                        } else {
                            this.drawing = false;
                        }
                        this.focus_viewer(window);
                        cx.notify();
                    })))
                    .child(icon_button("fit", "maximize", false).tip("Fit  (F) · double-click the image").on_click(cx.listener(|this, _, _, cx| {
                        this.fit();
                        cx.notify();
                    })))
                    .child(
                        icon_button("comments-toggle", "message", comments_open).tip("Comments  (Tab)").on_click(cx.listener(|this, _, _, cx| {
                            this.prefs.show_comments = !this.prefs.show_comments;
                            this.prefs.save();
                            cx.notify();
                        })),
                    )
                    .child(icon_button("fullscreen", "expand", false).tip("Fullscreen  (F11)").on_click(|_, window, _| window.toggle_fullscreen())),
            )
    }
}
