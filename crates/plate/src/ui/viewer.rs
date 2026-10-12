use super::widgets::{icon_button, mono, tag, TipExt};
use crate::app::{image_rect, CompareMode, Drag, Modal, Plate, Probe, SlotView, Tool};
use crate::theme::{self, s, FONT_MONO};
use gpui::{
    canvas, div, fill, point, prelude::*, px, Bounds, ContentMask, Context, Corners, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, Point, ScrollWheelEvent, Window,
};
use plate_core::loader::FrameStatus;
use plate_core::notes::{arrow_head, Shape};

struct SlotPaint {
    view: SlotView,
    width: u32,
    height: u32,
    aspect: f32,
}

impl Plate {
    pub(super) fn render_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale_factor = window.scale_factor();
        let slot_a = self.clip.as_ref().map(|c| (c.info.width, c.info.height, c.info.pixel_aspect));
        let slot_b = self.compare.as_ref().map(|c| (c.info.width, c.info.height, c.info.pixel_aspect));
        let a = slot_a.map(|(w, h, aspect)| SlotPaint { view: self.present(0, cx), width: w, height: h, aspect });
        let b = slot_b.map(|(w, h, aspect)| SlotPaint { view: self.present(1, cx), width: w, height: h, aspect });

        let zoom = self.zoom;
        let pan = self.pan;
        let compare_mode = self.compare_mode;
        let wipe = self.wipe;
        let shapes = self.visible_shapes();
        let guides = self.prefs.guides.clone();
        let probe_pos = self.probe.as_ref().map(|p| p.pos);
        let bounds_cell = self.viewer_bounds.clone();

        let painter = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _| {
                bounds_cell.set(bounds);
                window.paint_quad(fill(bounds, theme::viewer_bg()));
                let side = b.is_some() && compare_mode == CompareMode::SideBySide;
                let area = |slot: usize| -> Bounds<Pixels> {
                    if side {
                        let half = bounds.size.width / 2.;
                        let origin = if slot == 0 { bounds.origin } else { point(bounds.origin.x + half, bounds.origin.y) };
                        Bounds { origin, size: gpui::size(half, bounds.size.height) }
                    } else {
                        bounds
                    }
                };
                let Some(a) = a else { return };
                let rect_a = image_rect(area(0), a.width, a.height, a.aspect, zoom, pan, scale_factor);

                match (&b, compare_mode) {
                    (Some(b), CompareMode::Wipe) => {
                        let split = rect_a.origin.x + rect_a.size.width * wipe;
                        let left = Bounds::from_corners(bounds.origin, point(split, bounds.bottom_right().y));
                        let right = Bounds::from_corners(point(split, bounds.origin.y), bounds.bottom_right());
                        window.with_content_mask(Some(ContentMask { bounds: left }), |w| paint_slot(w, &a.view, rect_a));
                        // B is stretched onto A's frame so the wipe compares like for like.
                        window.with_content_mask(Some(ContentMask { bounds: right }), |w| paint_slot(w, &b.view, rect_a));
                        let line = Bounds { origin: point(split, bounds.origin.y), size: gpui::size(s(1.), bounds.size.height) };
                        window.paint_quad(fill(line, theme::ink()));
                        let handle = Bounds::centered_at(point(split, bounds.center().y), gpui::size(s(12.), s(28.)));
                        window.paint_quad(fill(handle, theme::ink()));
                    }
                    (Some(b), CompareMode::SideBySide) => {
                        window.with_content_mask(Some(ContentMask { bounds: area(0) }), |w| paint_slot(w, &a.view, rect_a));
                        let rect_b = image_rect(area(1), b.width, b.height, b.aspect, zoom, pan, scale_factor);
                        window.with_content_mask(Some(ContentMask { bounds: area(1) }), |w| paint_slot(w, &b.view, rect_b));
                        let line = Bounds { origin: point(area(1).origin.x, bounds.origin.y), size: gpui::size(s(1.), bounds.size.height) };
                        window.paint_quad(fill(line, theme::hairline_strong()));
                    }
                    _ => paint_slot(window, &a.view, rect_a),
                }

                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    if guides.visible && guides.any() {
                        paint_guides(window, &guides, rect_a, a.width as f32 * a.aspect / a.height.max(1) as f32);
                    }
                    for (shape, strong) in &shapes {
                        paint_shape(window, shape, rect_a, *strong);
                    }
                    if let Some(p) = probe_pos {
                        let c = theme::ink();
                        window.paint_quad(fill(Bounds { origin: point(p.x - s(9.), p.y), size: gpui::size(s(6.), s(1.)) }, c));
                        window.paint_quad(fill(Bounds { origin: point(p.x + s(4.), p.y), size: gpui::size(s(6.), s(1.)) }, c));
                        window.paint_quad(fill(Bounds { origin: point(p.x, p.y - s(9.)), size: gpui::size(s(1.), s(6.)) }, c));
                        window.paint_quad(fill(Bounds { origin: point(p.x, p.y + s(4.)), size: gpui::size(s(1.), s(6.)) }, c));
                    }
                });
            },
        )
        .size_full();

        let cursor = if self.drawing {
            gpui::CursorStyle::Crosshair
        } else if self.probe_on {
            gpui::CursorStyle::Crosshair
        } else if matches!(self.drag, Drag::Pan { .. }) {
            gpui::CursorStyle::ClosedHand
        } else {
            gpui::CursorStyle::Arrow
        };

        div()
            .id("viewer")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::viewer_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                this.focus_viewer(window);
                this.drag = Drag::Pan { start: ev.position, pan: this.pan };
                cx.notify();
            }))
            .on_mouse_move(cx.listener(Self::viewer_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                match this.drag {
                    Drag::Draw => this.finish_shape(),
                    Drag::Click { .. } => this.toggle_play(),
                    _ => {}
                }
                this.drag = Drag::None;
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Middle, cx.listener(|this, _, _, cx| {
                this.drag = Drag::None;
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                if dy.abs() > 0.0 {
                    this.zoom_at(1.0015f32.powf(dy * 2.0), Some(ev.position), window.scale_factor());
                    cx.notify();
                }
            }))
            .child(painter)
            .child(self.render_viewer_overlay(window, cx))
    }

    fn viewer_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_viewer(window);
        let sf = window.scale_factor();
        if self.clip.is_none() {
            self.prompt_open(false, cx);
            return;
        }
        if ev.click_count == 2 && !self.drawing {
            self.fit();
            self.drag = Drag::None;
            cx.notify();
            return;
        }
        if self.compare.is_some() && self.compare_mode == CompareMode::Wipe {
            if let Some(c) = &self.clip {
                let r = self.image_rect(self.viewer_bounds.get(), c.info.width, c.info.height, c.info.pixel_aspect, sf);
                let split = r.origin.x + r.size.width * self.wipe;
                if (f32::from(ev.position.x) - f32::from(split)).abs() < 8.0 {
                    self.drag = Drag::Wipe;
                    cx.notify();
                    return;
                }
            }
        }
        if self.drawing {
            if let Some(p) = self.to_image(ev.position, sf) {
                if self.tool == Tool::Text {
                    self.stop();
                    self.text_anchor = Some(p);
                    self.open_modal(Modal::TextAnnotation, window, cx);
                    return;
                }
                self.begin_shape(p);
                self.drag = Drag::Draw;
            }
        } else if self.probe_on {
            self.update_probe(ev.position, sf);
        } else {
            self.drag = Drag::Click { start: ev.position, pan: self.pan };
        }
        cx.notify();
    }

    fn viewer_mouse_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let sf = window.scale_factor();
        match self.drag {
            Drag::Pan { start, pan } => {
                self.pan = pan + (ev.position - start);
                cx.notify();
            }
            Drag::Click { start, pan } => {
                let d = ev.position - start;
                if f32::from(d.x).abs() + f32::from(d.y).abs() > 4.0 {
                    self.drag = Drag::Pan { start, pan };
                    self.pan = pan + d;
                    cx.notify();
                }
            }
            Drag::Draw => {
                if let Some(p) = self.to_image(ev.position, sf) {
                    self.extend_shape(p, ev.modifiers.shift);
                    cx.notify();
                }
            }
            Drag::Wipe => {
                if let Some(c) = &self.clip {
                    let r = self.image_rect(self.viewer_bounds.get(), c.info.width, c.info.height, c.info.pixel_aspect, sf);
                    self.wipe = (f32::from(ev.position.x - r.origin.x) / f32::from(r.size.width)).clamp(0.0, 1.0);
                    cx.notify();
                }
            }
            _ => {
                if self.probe_on && ev.pressed_button.is_none() {
                    self.update_probe(ev.position, sf);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn demo_probe(&mut self, pos: Point<Pixels>, sf: f32) {
        self.update_probe(pos, sf);
    }

    fn update_probe(&mut self, pos: Point<Pixels>, sf: f32) {
        let Some(clip) = &self.clip else { return };
        let Some(n) = self.to_image(pos, sf) else { return };
        let x = (n[0] * clip.info.width as f32).floor() as i64;
        let y = (n[1] * clip.info.height as f32).floor() as i64;
        let raw = match clip.loader.get(self.frame) {
            FrameStatus::Ready(f) => f.sample(x, y),
            _ => None,
        };
        let shown = self.presenter.shown[0].as_ref().and_then(|(_, img)| {
            let size = img.size(0);
            let (w, h) = (size.width.0 as i64, size.height.0 as i64);
            if x < 0 || y < 0 || x >= w || y >= h {
                return None;
            }
            let bytes = img.as_bytes(0)?;
            let i = ((y * w + x) * 4) as usize;
            Some([bytes[i + 2] as f32 / 255.0, bytes[i + 1] as f32 / 255.0, bytes[i] as f32 / 255.0])
        });
        self.probe = Some(Probe { pos, pixel: (x, y), raw, shown });
    }

    fn render_viewer_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sf = window.scale_factor();
        let bounds = self.viewer_bounds.get();
        let mut layer = div().absolute().inset_0().size_full();

        let Some(clip) = &self.clip else {
            return layer.child(self.render_empty_state(cx));
        };

        // Text annotations are laid out as real text on top of the canvas.
        let rect = self.image_rect(self.slot_area(0), clip.info.width, clip.info.height, clip.info.pixel_aspect, sf);
        let origin = bounds.origin;
        for (shape, strong) in self.visible_shapes() {
            if let Shape::Text { at, text, color, size } = shape {
                let x = rect.origin.x + rect.size.width * at[0] - origin.x;
                let y = rect.origin.y + rect.size.height * at[1] - origin.y;
                let font = (f32::from(rect.size.height) * size).max(8.0);
                layer = layer.child(
                    div()
                        .absolute()
                        .left(x)
                        .top(y)
                        .text_size(px(font))
                        .line_height(px(font * 1.2))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme::marker(color))
                        .opacity(if strong { 1.0 } else { 0.45 })
                        .whitespace_nowrap()
                        .child(text),
                );
            }
        }

        // Aspect label and captions sit inside the guide frame.
        let image_aspect = clip.info.width as f32 * clip.info.pixel_aspect / clip.info.height.max(1) as f32;
        let guides = &self.prefs.guides;
        let frame = guide_frame(rect, image_aspect, if guides.visible { guides.aspect } else { None });
        let local = |b: Bounds<Pixels>| Bounds { origin: point(b.origin.x - origin.x, b.origin.y - origin.y), size: b.size };
        if guides.visible {
            if let Some(a) = guides.aspect {
                let f = local(frame);
                layer = layer.child(
                    div()
                        .absolute()
                        .left(f.origin.x + s(6.))
                        .top(f.origin.y + s(4.))
                        .child(tag(format!("{}", aspect_label(a))).text_color(theme::ink_muted())),
                );
            }
        }
        let lines = self.caption_lines();
        if !lines.is_empty() {
            let style = self.prefs.captions.clone();
            let f = local(frame);
            let h = f32::from(f.size.height);
            let font = (h * style.size).max(10.0);
            let margin = px(h * 0.07);
            let mut block = div()
                .absolute()
                .left(f.origin.x)
                .top(f.origin.y)
                .w(f.size.width)
                .h(f.size.height)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(font * 0.12))
                .when(style.top, |d| d.justify_start().pt(margin))
                .when(!style.top, |d| d.justify_end().pb(margin));
            for line in lines {
                block = block.child(
                    div()
                        .px(px(font * 0.35))
                        .py(px(font * 0.08))
                        .rounded(px(font * 0.12))
                        .when(style.boxed, |d| d.bg(gpui::rgba(0x000000c0)))
                        .text_size(px(font))
                        .line_height(px(font * 1.25))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme::ink())
                        .whitespace_nowrap()
                        .child(line),
                );
            }
            layer = layer.child(block);
        }

        // The name lives in the top bar; the viewer only reports status.
        if let Some(status) = self.present_status() {
            layer = layer.child(
                div()
                    .absolute()
                    .right(s(14.))
                    .bottom(s(12.))
                    .px(s(8.))
                    .py(s(3.))
                    .rounded(s(theme::RADIUS))
                    .bg(theme::scrim())
                    .child(tag(status)),
            );
        }

        if let Some(b) = &self.compare {
            let label = |text: String, right: bool| {
                div()
                    .absolute()
                    .top(s(12.))
                    .when(right, |d| d.right(s(14.)))
                    .when(!right, |d| d.left(s(14.)))
                    .px(s(8.))
                    .py(s(3.))
                    .rounded(s(theme::RADIUS))
                    .bg(theme::scrim())
                    .border_1()
                    .border_color(theme::hairline())
                    .font_family(FONT_MONO)
                    .text_size(s(11.))
                    .child(text)
            };
            layer = layer
                .child(label(format!("A  {}", clip.info.name), false))
                .child(label(format!("B  {}", b.info.name), true));
        }

        if let SlotView::Missing(err) = self.peek_slot(0) {
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(s(6.))
                            .px(s(20.))
                            .py(s(14.))
                            .border_1()
                            .border_color(theme::warn())
                            .rounded(s(theme::RADIUS_LG))
                            .bg(theme::scrim())
                            .child(mono(format!("MISSING FRAME {}", self.frame_text(self.frame))).text_color(theme::warn()))
                            .child(div().text_size(s(12.)).text_color(theme::ink_muted()).max_w(s(420.)).child(err)),
                    ),
            );
        }

        if let Some(p) = &self.probe {
            let fmt = |v: f32| format!("{v:.4}");
            let mut lines = vec![format!("x {}  y {}", p.pixel.0, p.pixel.1)];
            if let Some(r) = p.raw {
                lines.push(format!("raw  {}  {}  {}  {}", fmt(r[0]), fmt(r[1]), fmt(r[2]), fmt(r[3])));
            }
            if let Some(s) = p.shown {
                lines.push(format!("disp {}  {}  {}", fmt(s[0]), fmt(s[1]), fmt(s[2])));
                lines.push(format!("8bit {:>4} {:>4} {:>4}", (s[0] * 255.0).round(), (s[1] * 255.0).round(), (s[2] * 255.0).round()));
            }
            let x = p.pos.x - origin.x + s(16.);
            let y = p.pos.y - origin.y + s(16.);
            let swatch = p.shown.map(|s| gpui::Rgba { r: s[0], g: s[1], b: s[2], a: 1.0 });
            layer = layer.child(
                div()
                    .absolute()
                    .left(x)
                    .top(y)
                    .flex()
                    .gap(s(8.))
                    .px(s(10.))
                    .py(s(8.))
                    .bg(theme::scrim())
                    .rounded(s(theme::RADIUS))
                    .border_1()
                    .border_color(theme::hairline_strong())
                    .children(swatch.map(|c| div().size(s(28.)).bg(c).border_1().border_color(theme::hairline_strong())))
                    .child(div().flex().flex_col().font_family(FONT_MONO).text_size(s(11.)).children(lines.into_iter().map(|l| div().child(l)))),
            );
        }

        if self.drawing {
            layer = layer.child(self.render_draw_toolbar(cx));
        }
        layer
    }

    fn peek_slot(&self, slot: usize) -> SlotView {
        let clip = if slot == 0 { self.clip.as_ref() } else { self.compare.as_ref() };
        let Some(clip) = clip else { return SlotView::Empty };
        match clip.loader.get(self.frame.min(clip.info.frame_count.saturating_sub(1))) {
            FrameStatus::Failed(e) => SlotView::Missing(e),
            _ => SlotView::Empty,
        }
    }

    /// A short note when the viewer is behind: decoding, or showing a held frame.
    fn present_status(&self) -> Option<String> {
        let clip = self.clip.as_ref()?;
        match clip.loader.get(self.frame) {
            FrameStatus::Loading => Some("LOADING".into()),
            _ => None,
        }
    }

    fn render_draw_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current_marker();
        let mut bar = div()
            .id("draw-toolbar")
            .absolute()
            .top(s(12.))
            .left(s(0.))
            .right(s(0.))
            .flex()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let mut island = div()
            .flex()
            .items_center()
            .gap(s(2.))
            .p(s(4.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::hairline_strong())
            .rounded(s(theme::RADIUS_LG))
            .shadow_md();
        for tool in Tool::ALL {
            island = island.child(
                div()
                    .relative()
                    .child(
                        icon_button(gpui::SharedString::from(format!("tool-{}", tool.key())), tool.icon(), self.tool == tool)
                            .tip(format!("{}  ({})", tool.label(), tool.key()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tool = tool;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(s(2.))
                            .bottom(s(0.))
                            .font_family(FONT_MONO)
                            .text_size(s(8.))
                            .text_color(theme::ink_faint())
                            .child(tool.key()),
                    ),
            );
        }
        island = island.child(div().w(s(1.)).h(s(20.)).mx(s(4.)).bg(theme::hairline()));
        for (i, c) in theme::MARKERS.iter().enumerate() {
            let selected = *c == current;
            island = island.child(
                div()
                    .id(("marker", i))
                    .size(s(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(s(theme::RADIUS))
                    .cursor_pointer()
                    .when(selected, |d| d.border_1().border_color(theme::ink()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(div().size(s(12.)).rounded(s(6.)).bg(theme::marker(*c)).border_1().border_color(theme::hairline_strong()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prefs.annotation_color = i;
                        this.prefs.save();
                        cx.notify();
                    })),
            );
        }
        island = island.child(div().w(s(1.)).h(s(20.)).mx(s(4.)).bg(theme::hairline()));
        for (i, (w, label)) in [(2.5f32, "S"), (4.0, "M"), (7.0, "L")].into_iter().enumerate() {
            let selected = (self.prefs.stroke_width - w).abs() < 0.1;
            island = island.child(
                div()
                    .id(("stroke", i))
                    .size(s(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(s(theme::RADIUS))
                    .cursor_pointer()
                    .font_family(FONT_MONO)
                    .text_size(s(11.))
                    .text_color(if selected { theme::blue() } else { theme::ink_muted() })
                    .when(selected, |d| d.bg(theme::blue_soft()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prefs.stroke_width = w;
                        this.prefs.save();
                        cx.notify();
                    })),
            );
        }
        island = island
            .child(div().w(s(1.)).h(s(20.)).mx(s(4.)).bg(theme::hairline()))
            .child(icon_button("undo-shape", "undo", false).tip("Undo  (Ctrl Z)").on_click(cx.listener(|this, _, _, cx| {
                this.undo_shape();
                cx.notify();
            })))
            .child(icon_button("clear-draft", "trash", false).tip("Clear marks").on_click(cx.listener(|this, _, _, cx| {
                this.clear_draft();
                cx.notify();
            })))
            .child(icon_button("close-draw", "x", false).tip("Done  (Esc)").on_click(cx.listener(|this, _, window, cx| {
                this.drawing = false;
                this.focus_viewer(window);
                cx.notify();
            })));
        bar = bar.child(island);
        bar
    }

    fn render_empty_state(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(s(18.))
            .child(icon_sized_muted("film"))
            .child(div().text_size(s(20.)).child("Drop media here"))
            .child(
                div()
                    .text_size(s(13.))
                    .text_color(theme::ink_muted())
                    .child("Movies, image sequences, EXR, DPX, QuickTime, DCP folders"),
            )
            .child(
                div()
                    .flex()
                    .gap(s(8.))
                    .child(super::widgets::primary_button("empty-open", "Open files").on_click(cx.listener(|this, _, _, cx| this.prompt_open(false, cx))))
                    .child(super::widgets::button("empty-folder", "Open folder").on_click(cx.listener(|this, _, _, cx| this.prompt_open(true, cx)))),
            )
            .child(tag("CTRL O  ·  CTRL SHIFT O  ·  ? FOR SHORTCUTS"))
    }
}

fn icon_sized_muted(name: &str) -> impl IntoElement {
    super::widgets::icon_sized(name, 40.).text_color(theme::ink_faint())
}

fn paint_slot(window: &mut Window, view: &SlotView, rect: Bounds<Pixels>) {
    match view {
        SlotView::Image(img) => {
            let _ = window.paint_image(rect, Corners::default(), img.clone(), 0, false);
        }
        SlotView::Loading | SlotView::Missing(_) => {
            window.paint_quad(fill(rect, theme::raised()));
        }
        SlotView::Empty => {}
    }
}

fn paint_shape(window: &mut Window, shape: &Shape, rect: Bounds<Pixels>, strong: bool) {
    let to = |p: [f32; 2]| point(rect.origin.x + rect.size.width * p[0], rect.origin.y + rect.size.height * p[1]);
    let h = f32::from(rect.size.height);
    let mut color: Hsla = theme::marker(shape.color());
    if !strong {
        color.a = 0.45;
    }
    let stroke = |w: f32| PathBuilder::stroke(px((w * h).max(1.5)));
    let mut builder = match shape {
        Shape::Pen { points, width, .. } => {
            let mut b = stroke(*width);
            if let Some(first) = points.first() {
                b.move_to(to(*first));
                if points.len() == 1 {
                    b.line_to(to(*first) + point(px(0.5), px(0.)));
                }
                for p in &points[1..] {
                    b.line_to(to(*p));
                }
            }
            b
        }
        Shape::Line { from, to: t, width, .. } => {
            let mut b = stroke(*width);
            b.move_to(to(*from));
            b.line_to(to(*t));
            b
        }
        Shape::Arrow { from, to: t, width, .. } => {
            let mut b = stroke(*width);
            let (a, e) = (to(*from), to(*t));
            b.move_to(a);
            b.line_to(e);
            let head = (width * h * 4.0).max(10.0);
            let (l, r) = arrow_head([f32::from(a.x), f32::from(a.y)], [f32::from(e.x), f32::from(e.y)], head);
            b.move_to(point(px(l[0]), px(l[1])));
            b.line_to(e);
            b.line_to(point(px(r[0]), px(r[1])));
            b
        }
        Shape::Rect { from, to: t, width, .. } => {
            let mut b = stroke(*width);
            let (a, e) = (to(*from), to(*t));
            b.add_polygon(&[a, point(e.x, a.y), e, point(a.x, e.y)], true);
            b
        }
        Shape::Ellipse { from, to: t, width, .. } => {
            let mut b = stroke(*width);
            let (a, e) = (to(*from), to(*t));
            let (cx, cy) = ((f32::from(a.x) + f32::from(e.x)) / 2., (f32::from(a.y) + f32::from(e.y)) / 2.);
            let (rx, ry) = ((f32::from(e.x) - f32::from(a.x)).abs() / 2., (f32::from(e.y) - f32::from(a.y)).abs() / 2.);
            let pts: Vec<Point<Pixels>> = (0..72)
                .map(|i| {
                    let t = i as f32 / 72.0 * std::f32::consts::TAU;
                    point(px(cx + rx * t.cos()), px(cy + ry * t.sin()))
                })
                .collect();
            b.add_polygon(&pts, true);
            b
        }
        Shape::Text { .. } => return,
    };
    let _ = &mut builder;
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// The area guides are measured in: the image, or the centered extraction of
/// the masked aspect ratio inside it.
pub(crate) fn guide_frame(rect: Bounds<Pixels>, image_aspect: f32, aspect: Option<f32>) -> Bounds<Pixels> {
    let Some(target) = aspect.filter(|a| *a > 0.0) else { return rect };
    let (w, h) = (f32::from(rect.size.width), f32::from(rect.size.height));
    let (fw, fh) = if target >= image_aspect { (w, w / target) } else { (h * target, h) };
    Bounds::centered_at(rect.center(), gpui::size(px(fw), px(fh)))
}

pub(crate) fn aspect_label(a: f32) -> String {
    match a {
        a if (a - 0.8).abs() < 1e-3 => "4:5".into(),
        a if (a - 0.5625).abs() < 1e-3 => "9:16".into(),
        a if (a - 1.3333).abs() < 1e-3 => "4:3".into(),
        a if (a - 1.7778).abs() < 1e-3 => "16:9".into(),
        a => format!("{a:.2}:1"),
    }
}

fn paint_guides(window: &mut Window, g: &plate_core::prefs::Guides, rect: Bounds<Pixels>, image_aspect: f32) {
    let frame = guide_frame(rect, image_aspect, g.aspect);
    let line = Hsla::from(theme::ink()).alpha(0.45);
    let faint = Hsla::from(theme::ink()).alpha(0.28);

    if g.aspect.is_some() {
        let shade = gpui::black().alpha(g.mask_opacity);
        // The bars outside the extraction, then a hairline at its edge.
        for bar in [
            Bounds::from_corners(rect.origin, point(rect.right(), frame.top())),
            Bounds::from_corners(point(rect.left(), frame.bottom()), rect.bottom_right()),
            Bounds::from_corners(point(rect.left(), frame.top()), point(frame.left(), frame.bottom())),
            Bounds::from_corners(point(frame.right(), frame.top()), point(rect.right(), frame.bottom())),
        ] {
            if bar.size.width > px(0.) && bar.size.height > px(0.) {
                window.paint_quad(fill(bar, shade));
            }
        }
        outline(window, frame, line, false);
    }

    let at = |fx: f32, fy: f32| point(frame.origin.x + frame.size.width * fx, frame.origin.y + frame.size.height * fy);
    let mut segments: Vec<(Point<Pixels>, Point<Pixels>, Hsla)> = Vec::new();
    if let Some((cols, rows)) = g.grid {
        for c in 1..cols {
            let x = c as f32 / cols as f32;
            segments.push((at(x, 0.0), at(x, 1.0), faint));
        }
        for r in 1..rows {
            let y = r as f32 / rows as f32;
            segments.push((at(0.0, y), at(1.0, y), faint));
        }
    }
    if g.thirds {
        for t in [1.0 / 3.0, 2.0 / 3.0] {
            segments.push((at(t, 0.0), at(t, 1.0), line));
            segments.push((at(0.0, t), at(1.0, t), line));
        }
    }
    if g.center {
        let arm = frame.size.height * 0.03;
        let c = frame.center();
        segments.push((point(c.x - arm, c.y), point(c.x + arm, c.y), line));
        segments.push((point(c.x, c.y - arm), point(c.x, c.y + arm), line));
    }
    for (a, b, color) in segments {
        let mut pb = PathBuilder::stroke(px(1.));
        pb.move_to(a);
        pb.line_to(b);
        if let Ok(p) = pb.build() {
            window.paint_path(p, color);
        }
    }
    // Action safe 93%, title safe 90% (SMPTE ST 2046-1 / EBU R95 style).
    let inset = |pct: f32| {
        let dx = frame.size.width * ((1.0 - pct) / 2.0);
        let dy = frame.size.height * ((1.0 - pct) / 2.0);
        Bounds::from_corners(point(frame.left() + dx, frame.top() + dy), point(frame.right() - dx, frame.bottom() - dy))
    };
    if g.action_safe {
        outline(window, inset(0.93), line, false);
    }
    if g.title_safe {
        outline(window, inset(0.90), line, true);
    }
}

fn outline(window: &mut Window, b: Bounds<Pixels>, color: Hsla, dashed: bool) {
    let mut pb = PathBuilder::stroke(px(1.));
    if dashed {
        pb = pb.dash_array(&[px(6.), px(4.)]);
    }
    pb.add_polygon(&[b.origin, point(b.right(), b.top()), b.bottom_right(), point(b.left(), b.bottom())], true);
    if let Ok(p) = pb.build() {
        window.paint_path(p, color);
    }
}
