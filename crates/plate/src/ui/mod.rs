//! The window: top bar, bin, viewer + transport, comments, modals.

mod bin;
mod comments;
mod modals;
mod slider;
mod topbar;
mod transport;
mod viewer;
pub mod widgets;

use crate::actions::*;
use crate::app::{CompareMode, Drag, Modal, Plate, Tool};
use crate::theme::{self, s, FONT_UI};
use gpui::{div, prelude::*, Context, ExternalPaths, Focusable, MouseButton, Window};
use plate_core::display::Channel;
use std::time::Instant;

impl Render for Plate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for img in self.presenter.to_drop.drain(..) {
            let _ = window.drop_image(img);
        }
        self.tick();
        if self.is_playing() {
            window.request_animation_frame();
        }
        if let Some(t) = &self.toast {
            let left = t.until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.toast = None;
            } else {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(left).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
        let title = match &self.clip {
            Some(c) => format!("{} — plate", c.info.name),
            None => "plate".into(),
        };
        if self.window_title != title {
            window.set_window_title(&title);
            self.window_title = title;
        }

        let show_bin = self.prefs.show_bin && self.bin.is_some();
        let show_comments = self.prefs.show_comments && self.clip.is_some();
        let viewer_context = if self.drawing { "Viewer drawing" } else { "Viewer" };

        let center = div()
            .id("center")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .key_context(viewer_context)
            .track_focus(&self.viewer_focus)
            .on_action(cx.listener(|this, _: &TogglePlay, _, cx| {
                this.toggle_play();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PlayForward, _, cx| {
                let speed = if this.playback.dir == 1 { (this.playback.speed * 2.0).min(8.0) } else { 1.0 };
                this.play(1, speed);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PlayReverse, _, cx| {
                let speed = if this.playback.dir == -1 { (this.playback.speed * 2.0).min(8.0) } else { 1.0 };
                this.play(-1, speed);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Pause, _, cx| {
                this.stop();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &StepForward, _, cx| {
                this.step(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &StepBack, _, cx| {
                this.step(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &StepForward10, _, cx| {
                this.step(10);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &StepBack10, _, cx| {
                this.step(-10);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &GoStart, _, cx| {
                this.stop();
                let a = this.clip.as_ref().map(|c| c.range().0).unwrap_or(0);
                this.seek(a);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &GoEnd, _, cx| {
                this.stop();
                let b = this.clip.as_ref().map(|c| c.range().1).unwrap_or(0);
                this.seek(b);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SetIn, _, cx| {
                this.set_in();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SetOut, _, cx| {
                this.set_out();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ClearInOut, _, cx| {
                this.clear_in_out();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ChannelRed, _, cx| {
                this.set_channel(Channel::Red);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ChannelGreen, _, cx| {
                this.set_channel(Channel::Green);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ChannelBlue, _, cx| {
                this.set_channel(Channel::Blue);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ChannelAlpha, _, cx| {
                this.set_channel(Channel::Alpha);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ChannelLuma, _, cx| {
                this.set_channel(Channel::Luma);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ChannelRgb, _, cx| {
                this.set_channel(Channel::Rgb);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomFit, _, cx| {
                this.fit();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Zoom100, _, cx| {
                this.one_to_one();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, window, cx| {
                this.zoom_at(1.25, None, window.scale_factor());
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
                this.zoom_at(0.8, None, window.scale_factor());
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ExposureUp, _, cx| {
                this.set_exposure(this.exposure + 0.25);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ExposureDown, _, cx| {
                this.set_exposure(this.exposure - 0.25);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ResetViewer, _, cx| {
                this.exposure = 0.0;
                this.gamma = 1.0;
                this.channel = Channel::Rgb;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleDraw, _, cx| {
                this.drawing = !this.drawing;
                if this.drawing {
                    this.stop();
                    this.probe_on = false;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &UndoShape, _, cx| {
                this.undo_shape();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.modal.is_some() {
                    this.close_modal(window, cx);
                } else if this.in_progress.is_some() {
                    this.in_progress = None;
                } else if this.drawing {
                    this.drawing = false;
                } else if this.probe_on {
                    this.probe_on = false;
                    this.probe = None;
                } else if this.focused_comment.is_some() {
                    this.focused_comment = None;
                } else if window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| {
                this.prefs.muted = !this.prefs.muted;
                this.audio.set_muted(this.prefs.muted);
                this.prefs.save();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleLoop, _, cx| {
                this.prefs.loop_playback = !this.prefs.loop_playback;
                this.prefs.save();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleCompare, window, cx| {
                if this.compare.is_some() {
                    this.set_compare(None, cx);
                } else {
                    this.open_modal(Modal::Compare, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CycleCompareMode, _, cx| {
                this.compare_mode = match this.compare_mode {
                    CompareMode::Wipe => CompareMode::SideBySide,
                    CompareMode::SideBySide => CompareMode::Wipe,
                };
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextClip, _, cx| this.step_clip(1, cx)))
            .on_action(cx.listener(|this, _: &PrevClip, _, cx| this.step_clip(-1, cx)))
            .on_action(cx.listener(|this, _: &NextComment, _, cx| {
                this.jump_comment(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PrevComment, _, cx| {
                this.jump_comment(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FocusComment, window, cx| {
                if this.clip.is_some() {
                    this.stop();
                    this.prefs.show_comments = true;
                    window.focus(&this.comment_input.focus_handle(cx));
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleProbe, _, cx| {
                this.probe_on = !this.probe_on;
                if !this.probe_on {
                    this.probe = None;
                } else {
                    this.drawing = false;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowHelp, window, cx| this.open_modal(Modal::Help, window, cx)))
            .on_action(cx.listener(|this, _: &PrevLayer, _, cx| {
                this.step_layer(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextLayer, _, cx| {
                this.step_layer(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleGuides, window, cx| {
                if this.prefs.guides.any() {
                    this.prefs.guides.visible = !this.prefs.guides.visible;
                    this.prefs.save();
                    cx.notify();
                } else {
                    this.open_modal(Modal::Guides, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleCaptions, window, cx| {
                if this.captions.is_some() {
                    this.prefs.captions.visible = !this.prefs.captions.visible;
                    this.prefs.save();
                    cx.notify();
                } else {
                    this.open_modal(Modal::Captions, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleComments, _, cx| {
                this.prefs.show_comments = !this.prefs.show_comments;
                this.prefs.save();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToolPen, _, cx| set_tool(this, Tool::Pen, cx)))
            .on_action(cx.listener(|this, _: &ToolArrow, _, cx| set_tool(this, Tool::Arrow, cx)))
            .on_action(cx.listener(|this, _: &ToolLine, _, cx| set_tool(this, Tool::Line, cx)))
            .on_action(cx.listener(|this, _: &ToolRect, _, cx| set_tool(this, Tool::Rect, cx)))
            .on_action(cx.listener(|this, _: &ToolEllipse, _, cx| set_tool(this, Tool::Ellipse, cx)))
            .on_action(cx.listener(|this, _: &ToolText, _, cx| set_tool(this, Tool::Text, cx)))
            .on_action(cx.listener(|this, _: &ToolEraser, _, cx| set_tool(this, Tool::Eraser, cx)))
            .on_action(cx.listener(|this, _: &NextColor, _, cx| {
                this.prefs.annotation_color = (this.prefs.annotation_color + 1) % theme::MARKERS.len();
                cx.notify();
            }))
            .child(self.render_viewer(window, cx))
            .child(self.render_transport(window, cx));

        div()
            .id("plate")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::paper())
            .text_color(theme::ink())
            .font_family(FONT_UI)
            .text_size(s(13.))
            .key_context("Plate")
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| this.prompt_open(false, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.prompt_open(true, cx)))
            .on_action(cx.listener(|this, _: &SaveFrame, _, cx| this.save_frame(cx)))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _: &ToggleBin, _, cx| {
                this.prefs.show_bin = !this.prefs.show_bin;
                this.prefs.save();
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| window.toggle_fullscreen()))
            .on_action(cx.listener(|this, _: &UiScaleUp, _, cx| this.set_ui_scale(theme::ui_scale() + 0.1, cx)))
            .on_action(cx.listener(|this, _: &UiScaleDown, _, cx| this.set_ui_scale(theme::ui_scale() - 0.1, cx)))
            .on_action(cx.listener(|this, _: &UiScaleReset, _, cx| this.set_ui_scale(1.0, cx)))
            .on_action(cx.listener(|this, _: &ColorspaceModal, window, cx| {
                if this.clip.is_some() {
                    this.open_modal(Modal::Colorspace, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &DisplayModal, window, cx| {
                if this.clip.is_some() {
                    this.open_modal(Modal::Display, window, cx)
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.open_paths(paths.paths().to_vec(), cx);
                this.focus_viewer(window);
            }))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(theme::structure()))
            // Scrubbing follows the mouse anywhere in the window, not just
            // while the pointer stays over the thin timeline.
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                if let Drag::Slider(kind) = this.drag {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.drag = Drag::None;
                        return;
                    }
                    this.slide_to(kind, ev.position.x, ev.modifiers.shift);
                    cx.notify();
                    return;
                }
                if matches!(this.drag, Drag::Scrub) {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.drag = Drag::None;
                        return;
                    }
                    if let Some(f) = this.frame_at(ev.position.x) {
                        if f != this.frame {
                            this.seek(f);
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if !matches!(this.drag, Drag::None) {
                    if matches!(this.drag, Drag::Draw) {
                        this.finish_shape();
                    }
                    this.drag = Drag::None;
                    cx.notify();
                }
            }))
            .child(self.render_topbar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(show_bin, |d| d.child(self.render_bin(window, cx)))
                    .child(center)
                    .when(show_comments, |d| d.child(self.render_comments(window, cx))),
            )
            .children(self.modal.map(|m| self.render_modal(m, window, cx)))
            .children(self.toast.as_ref().map(|t| {
                div()
                    .absolute()
                    .bottom(s(92.))
                    .left(s(0.))
                    .right(s(0.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .max_w(s(640.))
                            .px(s(14.))
                            .py(s(8.))
                            .bg(theme::raised())
                            .border_1()
                            .border_color(theme::hairline_strong())
                            .rounded(s(theme::RADIUS))
                            .text_size(s(12.))
                            .child(t.text.clone()),
                    )
            }))
    }
}

fn set_tool(this: &mut Plate, tool: Tool, cx: &mut Context<Plate>) {
    this.tool = tool;
    this.drawing = true;
    cx.notify();
}

impl Focusable for Plate {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.viewer_focus.clone()
    }
}
