//! Small shared building blocks.

use crate::theme::{self, s};
use gpui::{div, prelude::*, svg, AppContext, Div, ElementId, SharedString, Stateful, Svg};

/// SVG icons don't inherit text color in gpui, so every icon carries its own.
pub fn icon(name: &str) -> Svg {
    icon_sized(name, 16.)
}

pub fn icon_sized(name: &str, size: f32) -> Svg {
    svg().path(SharedString::from(format!("icons/{name}.svg"))).size(s(size)).flex_none().text_color(theme::ink_muted())
}

/// A square, borderless icon button.
pub fn icon_button(id: impl Into<ElementId>, name: &str, active: bool) -> Stateful<Div> {
    icon_button_colored(id, name, active, if active { theme::blue() } else { theme::ink_muted() })
}

pub fn icon_button_colored(id: impl Into<ElementId>, name: &str, active: bool, color: impl Into<gpui::Hsla>) -> Stateful<Div> {
    div()
        .id(id)
        .group("icon-button")
        .size(s(30.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(s(theme::RADIUS))
        .cursor_pointer()
        .when(active, |d| d.bg(theme::blue_soft()))
        .hover(|s| s.bg(theme::hover()))
        .child(icon(name).text_color(color.into()).group_hover("icon-button", |s| s.text_color(theme::ink())))
}

/// Uppercase monospace label, the site's technical-tag voice.
pub fn tag(text: impl Into<SharedString>) -> Div {
    div().font_family(theme::font_mono()).text_size(s(10.)).text_color(theme::ink_faint()).child(text.into())
}

pub fn mono(text: impl Into<SharedString>) -> Div {
    div().font_family(theme::font_mono()).text_size(s(12.)).child(text.into())
}

/// The joined strip that holds the top-bar controls.
pub fn segmented() -> Div {
    div()
        .h(s(30.))
        .flex()
        .flex_none()
        .items_center()
        .border_1()
        .border_color(theme::hairline())
        .rounded(s(theme::RADIUS))
        .overflow_hidden()
}

/// One segment of the strip: a small tag and a value, opening a modal on click.
pub fn chip(id: impl Into<ElementId>, label: &str, value: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .px(s(10.))
        .flex()
        .flex_none()
        .items_center()
        .gap(s(8.))
        .border_l_1()
        .border_color(theme::hairline())
        .when(active, |d| d.bg(theme::blue_soft()))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(tag(label.to_uppercase()))
        .child(div().text_size(s(12.)).text_color(theme::ink()).max_w(s(220.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(value.into()))
        .child(icon_sized("chevron-down", 12.).text_color(theme::ink_faint()))
}

/// A selectable row in a list.
pub fn row(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(s(10.))
        .py(s(6.))
        .flex()
        .items_center()
        .gap(s(8.))
        .rounded(s(theme::RADIUS))
        .cursor_pointer()
        .when(selected, |d| d.bg(theme::structure()))
        .hover(|s| s.bg(theme::hover()))
}

fn button_base(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(s(28.))
        .px(s(12.))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .rounded(s(theme::RADIUS))
        .text_size(s(12.))
        .cursor_pointer()
        .child(label.into())
}

/// Plain text button with a hairline border.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    button_base(id, label)
        .border_color(theme::hairline_strong())
        .hover(|s| s.bg(theme::hover()).border_color(theme::ink_muted()))
}

pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    button_base(id, label)
        .bg(theme::blue())
        .border_color(theme::blue())
        .text_color(theme::paper())
        .hover(|s| s.bg(theme::ink()))
}

struct Tip(SharedString);

impl gpui::Render for Tip {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(s(8.))
            .py(s(4.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::hairline_strong())
            .rounded(s(theme::RADIUS))
            .text_size(s(11.))
            .text_color(theme::ink())
            .child(self.0.clone())
    }
}

pub trait TipExt {
    fn tip(self, text: impl Into<SharedString>) -> Self;
}

impl TipExt for Stateful<Div> {
    fn tip(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        self.tooltip(move |_, cx| cx.new(|_| Tip(text.clone())).into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn primary_button_can_be_constructed() {
        let _ = super::primary_button("primary-test", "Open files");
    }
}
