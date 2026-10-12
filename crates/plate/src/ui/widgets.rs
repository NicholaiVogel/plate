//! Small shared building blocks.

use crate::theme::{self, FONT_MONO};
use gpui::{div, prelude::*, px, svg, AppContext, Div, ElementId, SharedString, Stateful, Svg};

/// SVG icons don't inherit text color in gpui, so every icon carries its own.
pub fn icon(name: &str) -> Svg {
    icon_sized(name, 16.)
}

pub fn icon_sized(name: &str, size: f32) -> Svg {
    svg().path(SharedString::from(format!("icons/{name}.svg"))).size(px(size)).flex_none().text_color(theme::ink_muted())
}

/// A square, borderless icon button.
pub fn icon_button(id: impl Into<ElementId>, name: &str, active: bool) -> Stateful<Div> {
    icon_button_colored(id, name, active, if active { theme::blue() } else { theme::ink_muted() })
}

pub fn icon_button_colored(id: impl Into<ElementId>, name: &str, active: bool, color: impl Into<gpui::Hsla>) -> Stateful<Div> {
    div()
        .id(id)
        .group("icon-button")
        .size(px(30.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS))
        .cursor_pointer()
        .when(active, |d| d.bg(theme::blue_soft()))
        .hover(|s| s.bg(theme::hover()))
        .child(icon(name).text_color(color.into()).group_hover("icon-button", |s| s.text_color(theme::ink())))
}

/// Uppercase monospace label, the site's technical-tag voice.
pub fn tag(text: impl Into<SharedString>) -> Div {
    div().font_family(FONT_MONO).text_size(px(10.)).text_color(theme::ink_faint()).child(text.into())
}

pub fn mono(text: impl Into<SharedString>) -> Div {
    div().font_family(FONT_MONO).text_size(px(12.)).child(text.into())
}

/// A top-bar control: small tag over a value, opening a modal on click.
pub fn chip(id: impl Into<ElementId>, label: &str, value: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(10.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .border_1()
        .border_color(if active { theme::blue() } else { theme::hairline() })
        .rounded(px(theme::RADIUS))
        .cursor_pointer()
        .hover(|s| s.border_color(theme::hairline_strong()).bg(theme::hover()))
        .child(tag(label.to_uppercase()))
        .child(div().text_size(px(12.)).text_color(theme::ink()).max_w(px(220.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(value.into()))
        .child(icon_sized("chevron-down", 12.).text_color(theme::ink_faint()))
}

/// A selectable row in a list.
pub fn row(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(10.))
        .py(px(6.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(theme::RADIUS))
        .cursor_pointer()
        .border_l_2()
        .border_color(if selected { theme::blue() } else { gpui::transparent_black().into() })
        .when(selected, |d| d.bg(theme::structure()))
        .hover(|s| s.bg(theme::hover()))
}

/// Plain text button with a hairline border.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(theme::hairline_strong())
        .rounded(px(theme::RADIUS))
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).border_color(theme::ink_muted()))
        .child(label.into())
}

pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    button(id, label).bg(theme::blue()).border_color(theme::blue()).text_color(theme::paper()).hover(|s| s.bg(theme::ink()))
}

struct Tip(SharedString);

impl gpui::Render for Tip {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::hairline_strong())
            .rounded(px(theme::RADIUS))
            .text_size(px(11.))
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
