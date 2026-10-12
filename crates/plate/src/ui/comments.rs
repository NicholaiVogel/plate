use super::widgets::{icon, icon_button, icon_sized, tag, TipExt};
use crate::app::Plate;
use crate::theme::{self, s, FONT_MONO};
use gpui::{div, prelude::*, Context, FontWeight, MouseButton, SharedString, Window};

fn ago(created: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(created);
    let d = now.saturating_sub(created);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86_400),
    }
}

impl Plate {
    pub(super) fn render_comments(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(clip) = &self.clip else { return div() };
        let count = clip.notes.comments.len();
        let open = clip.notes.comments.iter().filter(|c| !c.done).count();

        let mut list = div().id("comment-list").flex_1().min_h_0().overflow_y_scroll().px(s(10.)).py(s(8.)).flex().flex_col().gap(s(6.));
        if count == 0 {
            list = list.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(s(10.))
                    .pt(s(80.))
                    .child(icon_sized("message", 36.).text_color(theme::ink_faint()))
                    .child(div().text_color(theme::ink_muted()).child("No comments yet"))
                    .child(tag("ENTER TO WRITE  ·  D TO DRAW")),
            );
        }
        for c in &clip.notes.comments {
            let focused = self.focused_comment.as_deref() == Some(c.id.as_str());
            let on_frame = c.frame == self.frame;
            let id = c.id.clone();
            let id_done = c.id.clone();
            let id_del = c.id.clone();
            let frame = c.frame;
            let label = self.frame_text(c.frame);
            let marks = c.shapes.len();
            list = list.child(
                div()
                    .id(SharedString::from(format!("c-{}", c.id)))
                    .group("comment")
                    .flex()
                    .flex_col()
                    .gap(s(6.))
                    .p(s(10.))
                    .border_1()
                    .border_color(if focused { theme::blue() } else if on_frame { theme::hairline_strong() } else { theme::hairline() })
                    .rounded(s(theme::RADIUS))
                    .when(focused, |d| d.bg(theme::structure()))
                    .when(c.done, |d| d.opacity(0.55))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.stop();
                        this.seek(frame);
                        this.focused_comment = Some(id.clone());
                        this.focus_viewer(window);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(s(8.))
                            .child(
                                div()
                                    .px(s(6.))
                                    .py(s(1.))
                                    .bg(if focused { theme::blue() } else { theme::structure() })
                                    .text_color(if focused { theme::paper() } else { theme::ink() })
                                    .font_family(FONT_MONO)
                                    .text_size(s(11.))
                                    .child(label),
                            )
                            .child(div().font_weight(FontWeight::MEDIUM).text_size(s(12.)).child(c.author.clone()))
                            .child(div().text_size(s(11.)).text_color(theme::ink_faint()).child(ago(c.created)))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex()
                                    .gap(s(0.))
                                    .invisible()
                                    .group_hover("comment", |s| s.visible())
                                    .child(
                                        icon_button(SharedString::from(format!("done-{}", c.id)), "check", c.done)
                                            .size(s(24.))
                                            .tip(if c.done { "Reopen" } else { "Mark done" })
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.toggle_done(&id_done);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        icon_button(SharedString::from(format!("del-{}", c.id)), "trash", false)
                                            .size(s(24.))
                                            .tip("Delete")
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.delete_comment(&id_del);
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .when(!c.text.is_empty(), |d| d.child(div().text_size(s(13.)).line_height(s(18.)).child(c.text.clone())))
                    .when(marks > 0, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(s(6.))
                                .text_color(theme::ink_faint())
                                .child(icon_sized("pencil", 12.).text_color(theme::ink_faint()))
                                .child(div().text_size(s(11.)).child(format!("{marks} mark{}", if marks == 1 { "" } else { "s" }))),
                        )
                    }),
            );
        }

        let draft_marks = self.draft.len();
        let pinned = self.draft_frame.unwrap_or(self.frame);
        let current = self.current_marker();

        div()
            .w(s(340.))
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .h(s(44.))
                    .flex_none()
                    .px(s(14.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(s(8.))
                            .child(div().font_weight(FontWeight::MEDIUM).child("Comments"))
                            .child(tag(format!("{open} OPEN · {count} TOTAL"))),
                    )
                    .child(
                        div()
                            .flex()
                            .child(icon_button("prev-comment", "step-back", false).tip("Previous comment  ([)").on_click(cx.listener(|this, _, _, cx| {
                                this.jump_comment(-1);
                                cx.notify();
                            })))
                            .child(icon_button("next-comment", "step-forward", false).tip("Next comment  (])").on_click(cx.listener(|this, _, _, cx| {
                                this.jump_comment(1);
                                cx.notify();
                            }))),
                    ),
            )
            .child(list)
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(theme::hairline())
                    .p(s(10.))
                    .flex()
                    .flex_col()
                    .gap(s(8.))
                    .child(
                        div()
                            .id("composer")
                            .flex()
                            .items_center()
                            .gap(s(8.))
                            .px(s(8.))
                            .py(s(7.))
                            .border_1()
                            .border_color(theme::hairline_strong())
                            .rounded(s(theme::RADIUS))
                            .bg(theme::raised())
                            .child(
                                div()
                                    .px(s(5.))
                                    .bg(theme::blue())
                                    .text_color(theme::paper())
                                    .font_family(FONT_MONO)
                                    .text_size(s(11.))
                                    .child(self.frame_text(pinned)),
                            )
                            .child(div().flex_1().min_w_0().text_size(s(13.)).child(self.comment_input.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(s(2.))
                            .child(icon_button("composer-draw", "pencil", self.drawing).tip("Annotate this frame  (D)").on_click(cx.listener(|this, _, window, cx| {
                                this.drawing = !this.drawing;
                                if this.drawing {
                                    this.stop();
                                }
                                this.focus_viewer(window);
                                cx.notify();
                            })))
                            .child(
                                div()
                                    .id("composer-color")
                                    .size(s(30.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(s(theme::RADIUS))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::hover()))
                                    .child(div().size(s(12.)).rounded(s(6.)).bg(theme::marker(current)).border_1().border_color(theme::hairline_strong()))
                                    .tip("Annotation color  (X while drawing)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prefs.annotation_color = (this.prefs.annotation_color + 1) % theme::MARKERS.len();
                                        this.prefs.save();
                                        cx.notify();
                                    })),
                            )
                            .when(draft_marks > 0, |d| {
                                d.child(div().ml(s(4.)).text_size(s(11.)).text_color(theme::ink_muted()).child(format!(
                                    "{draft_marks} mark{} on {}",
                                    if draft_marks == 1 { "" } else { "s" },
                                    self.frame_text(pinned)
                                )))
                                .child(icon_button("composer-undo", "undo", false).size(s(26.)).tip("Undo mark").on_click(cx.listener(|this, _, _, cx| {
                                    this.undo_shape();
                                    cx.notify();
                                })))
                            })
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("send")
                                    .size(s(30.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(s(theme::RADIUS))
                                    .bg(theme::blue())
                                    .text_color(theme::paper())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::ink()))
                                    .child(icon("send").text_color(theme::paper()))
                                    .tip("Post  (Enter)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let text = this.comment_input.read(cx).text().to_string();
                                        if this.submit_comment(text) {
                                            this.comment_input.update(cx, |i, cx| i.set_text("", cx));
                                            this.focus_viewer(window);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }
}
