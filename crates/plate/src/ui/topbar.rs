use super::widgets::{chip, icon_button, icon_sized, segmented, TipExt};
use crate::app::{CompareMode, Modal, Plate, SliderKind};
use crate::theme::{self, s, FONT_MONO};
use gpui::{div, prelude::*, Context, FontWeight, Window};
use plate_core::display::Channel;

impl Plate {
    pub(super) fn render_topbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clip = self.clip.as_ref();
        // A short spec beside the name: what you'd want to check at a glance.
        // The container is in the extension and the length is in the counter.
        let spec: Vec<String> = clip
            .map(|c| {
                let i = &c.info;
                let fps = if (i.fps - i.fps.round()).abs() < 0.01 { format!("{:.0} fps", i.fps) } else { format!("{:.3} fps", i.fps) };
                let mut parts = vec![format!("{} × {}", i.width, i.height), fps, i.format.codec_label()];
                if i.has_alpha {
                    parts.push("alpha".into());
                }
                if !i.layers.is_empty() {
                    parts.push(format!("{} layers", i.layers.len()));
                }
                parts
            })
            .unwrap_or_default();
        // Name with the extension dimmed; sequences show their pattern.
        let (stem, ext) = clip
            .map(|c| {
                let name = c.info.name.split(" [").next().unwrap_or(&c.info.name).to_string();
                match name.rfind('.') {
                    Some(dot) if dot > 0 && name.len() - dot <= 6 => (name[..dot].to_string(), name[dot..].to_string()),
                    _ => (name, String::new()),
                }
            })
            .unwrap_or_default();

        let version_pill = {
            let (list, at) = self.versions();
            at.filter(|_| list.len() > 1).map(|i| (format!("v{:03}", list[i].0), list.len()))
        };
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
                .when_some(c.info.layer.clone().filter(|_| !c.info.layers.is_empty()), |d, layer| {
                    d.child(
                        chip("chip-layer", "Layer", layer, self.modal == Some(Modal::Layers))
                            .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Layers, window, cx))),
                    )
                })
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
                        icon_button("bin-toggle", "sidebar", self.prefs.show_bin && self.bin.is_some())
                            .tip("Sidebar  (Shift Tab)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prefs.show_bin = !this.prefs.show_bin;
                                this.prefs.save();
                                cx.notify();
                            })),
                    )
                    .child(match clip {
                        Some(_) => div()
                            .flex()
                            .items_baseline()
                            .min_w_0()
                            .flex_shrink()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(s(14.))
                            .child(div().font_weight(FontWeight::MEDIUM).text_color(theme::ink()).child(stem))
                            .child(div().text_color(theme::ink_faint()).child(ext)),
                        None => div().text_color(theme::ink_faint()).text_size(s(14.)).child("No media"),
                    })
                    .children(version_pill.map(|(label, count)| {
                        div()
                            .id("version-pill")
                            .flex_none()
                            .h(s(24.))
                            .pl(s(8.))
                            .pr(s(6.))
                            .flex()
                            .items_center()
                            .gap(s(4.))
                            .rounded(s(theme::RADIUS))
                            .bg(theme::raised())
                            .cursor_pointer()
                            .hover(|st| st.bg(theme::structure()))
                            .child(div().text_size(s(13.)).text_color(theme::ink()).child(label))
                            .child(icon_sized("chevron-down", 12.).text_color(theme::ink_muted()))
                            .tip(format!("{count} versions · V lists them · Alt ↑ ↓ steps · Shift V compares"))
                            .on_click(cx.listener(|this, _, window, cx| this.open_modal(Modal::Versions, window, cx)))
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(s(14.))
                            .ml(s(6.))
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(s(12.))
                            .text_color(theme::ink_muted())
                            .children(spec.into_iter().map(|p| div().flex_none().child(p))),
                    ),
            )
            .child(controls)
    }
}

/// "sRGB - Display" -> "sRGB".
fn short_display(d: &str) -> String {
    d.trim_end_matches(" - Display").to_string()
}
