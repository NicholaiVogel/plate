//! Inline exposure and gamma sliders for the top-bar strip.

use super::widgets::tag;
use crate::app::{Drag, Plate, SliderKind};
use crate::theme::{self, s, FONT_MONO};
use gpui::{canvas, div, fill, point, prelude::*, px, Bounds, Context, MouseButton, MouseDownEvent, Pixels, ScrollWheelEvent};

/// Gamma runs on a log scale so 1.0 sits in the middle of the track.
const GAMMA_MIN: f32 = 0.2;
const GAMMA_MAX: f32 = 5.0;
const EXPOSURE_RANGE: f32 = 8.0;

impl SliderKind {
    fn to_t(self, v: f32) -> f32 {
        match self {
            SliderKind::Exposure => (v + EXPOSURE_RANGE) / (2.0 * EXPOSURE_RANGE),
            SliderKind::Gamma => (v / GAMMA_MIN).ln() / (GAMMA_MAX / GAMMA_MIN).ln(),
        }
        .clamp(0.0, 1.0)
    }

    fn from_t(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            SliderKind::Exposure => t * 2.0 * EXPOSURE_RANGE - EXPOSURE_RANGE,
            SliderKind::Gamma => GAMMA_MIN * (GAMMA_MAX / GAMMA_MIN).powf(t),
        }
    }

    fn neutral(self) -> f32 {
        match self {
            SliderKind::Exposure => 0.0,
            SliderKind::Gamma => 1.0,
        }
    }

    /// The increment a drag snaps to, like the f-stop and gamma sliders in
    /// Nuke's viewer. Shift drags freely in fine steps.
    fn step(self, fine: bool) -> f32 {
        match (self, fine) {
            (SliderKind::Exposure, false) => 0.25,
            (SliderKind::Exposure, true) => 0.05,
            (SliderKind::Gamma, false) => 0.1,
            (SliderKind::Gamma, true) => 0.01,
        }
    }

    fn snap(self, v: f32, fine: bool) -> f32 {
        let step = self.step(fine);
        (v / step).round() * step
    }

    /// Tick positions drawn under the track: whole stops, and the gamma
    /// values people actually reach for.
    fn ticks(self) -> &'static [f32] {
        match self {
            SliderKind::Exposure => &[-8., -7., -6., -5., -4., -3., -2., -1., 0., 1., 2., 3., 4., 5., 6., 7., 8.],
            SliderKind::Gamma => &[0.2, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0],
        }
    }

    fn label(self) -> &'static str {
        match self {
            SliderKind::Exposure => "EXP",
            SliderKind::Gamma => "γ",
        }
    }
}

impl Plate {
    pub fn slider_value(&self, kind: SliderKind) -> f32 {
        match kind {
            SliderKind::Exposure => self.exposure,
            SliderKind::Gamma => self.gamma,
        }
    }

    pub fn set_slider(&mut self, kind: SliderKind, v: f32) {
        match kind {
            SliderKind::Exposure => self.set_exposure(v),
            SliderKind::Gamma => self.set_gamma(v),
        }
    }

    /// Sets a slider from a mouse x position, snapped to its increment.
    pub fn slide_to(&mut self, kind: SliderKind, x: Pixels, fine: bool) {
        let b = self.slider_bounds[kind as usize].get();
        let w = f32::from(b.size.width).max(1.0);
        let t = f32::from(x - b.origin.x) / w;
        self.set_slider(kind, kind.snap(kind.from_t(t), fine));
    }

    /// Steps a slider by whole increments (scroll wheel).
    pub fn nudge(&mut self, kind: SliderKind, steps: i32, fine: bool) {
        let v = self.slider_value(kind) + steps as f32 * kind.step(fine);
        self.set_slider(kind, kind.snap(v, fine));
    }

    pub(super) fn render_slider(&self, kind: SliderKind, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.slider_value(kind);
        let t = kind.to_t(value);
        let t0 = kind.to_t(kind.neutral());
        let changed = (value - kind.neutral()).abs() > 1e-4;
        let bounds_cell = self.slider_bounds[kind as usize].clone();
        let ticks: Vec<f32> = kind.ticks().iter().map(|v| kind.to_t(*v)).collect();
        let text = match kind {
            SliderKind::Exposure => format!("{value:+.2}"),
            SliderKind::Gamma => format!("{value:.2}"),
        };

        let track = canvas(
            move |bounds, _, _| bounds,
            move |bounds: Bounds<Pixels>, _, window, _| {
                bounds_cell.set(bounds);
                let w = f32::from(bounds.size.width);
                let cy = bounds.center().y;
                let x_at = |t: f32| bounds.origin.x + px(w * t);
                let line = Bounds { origin: point(bounds.origin.x, cy - s(1.)), size: gpui::size(bounds.size.width, s(2.)) };
                window.paint_quad(fill(line, theme::hairline_strong()).corner_radii(s(1.)));
                let (a, b) = if t >= t0 { (t0, t) } else { (t, t0) };
                let filled = Bounds::from_corners(point(x_at(a), cy - s(1.)), point(x_at(b), cy + s(1.)));
                window.paint_quad(fill(filled, theme::blue()).corner_radii(s(1.)));
                for &tt in &ticks {
                    let major = (tt - t0).abs() < 1e-4;
                    let h = if major { s(10.) } else { s(5.) };
                    let tick = Bounds { origin: point(x_at(tt), cy + s(3.)), size: gpui::size(s(1.), h - s(3.)) };
                    window.paint_quad(fill(tick, if major { theme::ink_muted() } else { theme::ink_faint() }));
                }
                let knob = Bounds::centered_at(point(x_at(t), cy), gpui::size(s(6.), s(14.)));
                window.paint_quad(fill(knob, theme::ink()).corner_radii(s(2.)));
            },
        )
        .size_full();

        div()
            .h_full()
            .px(s(10.))
            .flex()
            .flex_none()
            .items_center()
            .gap(s(8.))
            .border_l_1()
            .border_color(theme::hairline())
            .when(changed, |d| d.bg(theme::blue_soft()))
            .child(tag(kind.label()))
            .child(
                div()
                    .id(("slider", kind as usize))
                    .w(s(84.))
                    .h(s(22.))
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        if ev.click_count >= 2 {
                            this.set_slider(kind, kind.neutral());
                            this.drag = Drag::None;
                        } else {
                            this.drag = Drag::Slider(kind);
                            this.slide_to(kind, ev.position.x, ev.modifiers.shift);
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
                        // One increment per wheel notch; trackpads accumulate.
                        this.slider_scroll -= f32::from(ev.delta.pixel_delta(px(20.)).y);
                        let steps = (this.slider_scroll / 20.0).trunc() as i32;
                        if steps != 0 {
                            this.slider_scroll -= steps as f32 * 20.0;
                            this.nudge(kind, steps, ev.modifiers.shift);
                            cx.notify();
                        }
                    }))
                    .child(track),
            )
            .child(
                div()
                    .w(s(38.))
                    .flex()
                    .justify_end()
                    .font_family(FONT_MONO)
                    .text_size(s(12.))
                    .text_color(if changed { theme::ink() } else { theme::ink_muted() })
                    .child(text),
            )
    }
}
