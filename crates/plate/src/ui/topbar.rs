use super::widgets::{chip, icon_button, segmented, tag};
use crate::app::{CompareMode, Modal, Plate, SliderKind};
use crate::theme::{self, s, FONT_MONO};
use gpui::{div, prelude::*, Context, FontWeight, Window};
use plate_core::display::Channel;

impl Plate {
    pub(super) fn render_topbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clip = self.clip.as_ref();
        let meta = clip.map(|c| {
            let i = &c.info;
            let fps = if (i.fps - i.fps.round()).abs() < 0.01 { format!("{:.0}", i.fps) } else { format!("{:.3}", i.fps) };
            let mut parts = vec![format!("{}×{}", i.width, i.height), format!("{fps} fps"), i.format.label()];
            if i.frame_count > 1 {
                parts.push(format!("{} fr", i.frame_count));
            }
            if i.has_alpha {
                parts.push("alpha".into());
            }
            parts.join("  ·  ")
        });

        let mut controls = div().flex().flex_none().items_center().gap(s(6.));
        if let Some(c) = clip {
            let view = format!("{} · {}", short_display(&c.choice.display), c.choice.view);
            let compare = match (&self.compare, self.compare_mode) {
                (None, _) => "Off".to_string(),
                (Some(_), CompareMode::Wipe) => "Wipe".to_string(),
                (Some(_), CompareMode::SideBySide) => "Side by side".to_string(),
            };
            let strip = segmented()
                .child(
                    chip("chip-input", "Input", c.choice.colorspace.clone(), self.modal == Some(Modal::Colorspace))
                        .border_l_0()
                        .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Colorspace, window, cx))),
                )
                .child(
                    chip("chip-view", "View", view, self.modal == Some(Modal::Display))
                        .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Display, window, cx))),
                )
                .child(
                    chip("chip-channel", "Ch", self.channel.label(), self.channel != Channel::Rgb || self.modal == Some(Modal::Channel))
                        .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Channel, window, cx))),
                )
                .child(self.render_slider(SliderKind::Exposure, cx))
                .child(self.render_slider(SliderKind::Gamma, cx))
                .child(
                    chip("chip-compare", "A/B", compare, self.compare.is_some() || self.modal == Some(Modal::Compare))
                        .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Compare, window, cx))),
                );
            controls = controls
                .child(strip)
                .child(div().w(s(1.)).h(s(20.)).mx(s(4.)).bg(theme::hairline()))
                .child(icon_button("save-frame", "save", false).on_click(cx.listener(|this, _, _, cx| this.save_frame(cx))));
        }
        controls = controls
            .child(icon_button("open-folder", "folder", false).on_click(cx.listener(|this, _, _, cx| this.prompt_open(true, cx))))
            .child(
                icon_button("settings", "sliders", self.modal == Some(Modal::Settings))
                    .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Settings, window, cx))),
            )
            .child(
                div()
                    .id("help")
                    .size(s(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(s(theme::RADIUS))
                    .cursor_pointer()
                    .font_family(FONT_MONO)
                    .text_size(s(13.))
                    .text_color(theme::ink_muted())
                    .hover(|s| s.bg(theme::hover()).text_color(theme::ink()))
                    .child("?")
                    .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Help, window, cx))),
            );

        div()
            .h(s(48.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(s(16.))
            .px(s(12.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap(s(12.))
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("bin-toggle")
                            .flex()
                            .items_center()
                            .gap(s(8.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prefs.show_bin = !this.prefs.show_bin;
                                cx.notify();
                            }))
                            .flex_none()
                            .child(div().size(s(8.)).bg(theme::blue()))
                            .child(div().font_family(FONT_MONO).font_weight(FontWeight::MEDIUM).text_size(s(12.)).child("PLATE")),
                    )
                    .child(div().flex_none().w(s(1.)).h(s(20.)).bg(theme::hairline()))
                    .child(match clip {
                        Some(c) => div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_size(s(13.))
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(c.info.name.clone()),
                            )
                            .child(tag(meta.unwrap_or_default()).whitespace_nowrap().overflow_hidden().text_ellipsis()),
                        None => div().text_color(theme::ink_faint()).text_size(s(13.)).child("No media"),
                    }),
            )
            .child(controls)
    }
}

/// "sRGB - Display" -> "sRGB".
fn short_display(d: &str) -> String {
    d.trim_end_matches(" - Display").to_string()
}
