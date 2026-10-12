use super::widgets::{button, icon, icon_button, primary_button, row, tag};
use crate::actions::SHORTCUTS;
use crate::app::{CompareMode, Modal, Plate};
use crate::theme::{self, s};
use gpui::{div, prelude::*, AnyElement, Context, Div, FontWeight, MouseButton, PathPromptOptions, SharedString, Window};
use plate_core::display::Channel;
use plate_core::media;
use plate_core::prefs::{ColorChoice, OcioChoice};
use plate_core::scan::EntryKind;

impl Plate {
    pub(super) fn render_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (title, width, body): (&str, f32, AnyElement) = match modal {
            Modal::Colorspace => ("Input colorspace", 520., self.modal_colorspace(window, cx).into_any_element()),
            Modal::Display => ("Display & view", 560., self.modal_display(cx).into_any_element()),
            Modal::Channel => ("Channels", 340., self.modal_channel(cx).into_any_element()),
            Modal::Compare => ("Compare A/B", 480., self.modal_compare(cx).into_any_element()),
            Modal::Settings => ("Settings", 560., self.modal_settings(cx).into_any_element()),
            Modal::Help => ("Shortcuts", 720., self.modal_help().into_any_element()),
            Modal::Layers => ("Layers", 460., self.modal_layers(cx).into_any_element()),
            Modal::Versions => ("Versions", 520., self.modal_versions(cx).into_any_element()),
            Modal::Guides => ("Guides", 520., self.modal_guides(cx).into_any_element()),
            Modal::Captions => ("Captions", 480., self.modal_captions(cx).into_any_element()),
            Modal::TextAnnotation => ("Text annotation", 420., self.modal_text().into_any_element()),
        };
        div()
            .id("modal-backdrop")
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000080))
            .flex()
            .flex_col()
            .items_center()
            .pt(s(64.))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_modal(window, cx)))
            .child(
                div()
                    .id("modal")
                    .w(s(width))
                    // Already in window pixels; not UI-scaled.
                    .max_h(window.viewport_size().height * 0.8)
                    .flex()
                    .flex_col()
                    .bg(theme::paper())
                    .border_1()
                    .border_color(theme::hairline_strong())
                    .rounded(s(theme::RADIUS_LG))
                    .overflow_hidden()
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                            .child(div().font_weight(FontWeight::MEDIUM).child(title))
                            .child(icon_button("modal-close", "x", false).on_click(cx.listener(|this, _, window, cx| this.close_modal(window, cx)))),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn modal_colorspace(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let Some(clip) = &self.clip else { return div() };
        let query = self.search_input.read(cx).text().to_lowercase();
        let current = clip.choice.colorspace.clone();
        let format_label = clip.info.format.label();
        let hint = clip.info.colorspace_hint.clone();
        let remembered = clip.remembered;

        let mut list = div().id("cs-list").flex_1().min_h_0().overflow_y_scroll().p(s(6.)).flex().flex_col();
        let mut shown = 0;
        for (family, spaces) in self.color.grouped() {
            let matches: Vec<_> = spaces
                .into_iter()
                .filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query) || family.to_lowercase().contains(&query))
                .collect();
            if matches.is_empty() {
                continue;
            }
            list = list.child(div().px(s(10.)).pt(s(10.)).pb(s(4.)).child(tag(family.to_uppercase())));
            for cs in matches {
                shown += 1;
                let name = cs.name.clone();
                let selected = name == current;
                let enc = cs.encoding.clone();
                list = list.child(
                    row(SharedString::from(format!("cs-{name}")), selected)
                        .child(div().flex_1().min_w_0().text_size(s(13.)).child(name.clone()))
                        .when(!enc.is_empty(), |d| d.child(tag(enc.to_uppercase())))
                        .when(selected, |d| d.child(icon("check").text_color(theme::blue())))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(c) = &this.clip {
                                let choice = ColorChoice { colorspace: name.clone(), ..c.choice.clone() };
                                this.set_choice(choice);
                            }
                            this.close_modal(window, cx);
                        })),
                );
            }
        }
        if shown == 0 {
            list = list.child(div().p(s(14.)).text_color(theme::ink_faint()).child("No colorspace matches"));
        }

        div()
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .p(s(10.))
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(
                        div()
                            .px(s(10.))
                            .py(s(7.))
                            .border_1()
                            .border_color(theme::hairline_strong())
                            .rounded(s(theme::RADIUS))
                            .bg(theme::raised())
                            .child(self.search_input.clone()),
                    ),
            )
            .child(list)
            .child(
                div()
                    .flex_none()
                    .px(s(14.))
                    .py(s(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(s(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(s(2.))
                            .child(div().text_size(s(12.)).child(format!("Remembered for every {format_label} file")))
                            .child(tag(match (remembered, hint) {
                                (true, _) => "SET BY YOU".to_string(),
                                (false, Some(h)) => format!("DEFAULT · FILE SAYS {}", h.to_uppercase()),
                                (false, None) => "DEFAULT FOR THIS FORMAT".to_string(),
                            })),
                    )
                    .child(button("cs-reset", "Reset to default").on_click(cx.listener(|this, _, window, cx| {
                        this.reset_choice();
                        this.close_modal(window, cx);
                    }))),
            )
    }

    fn modal_display(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(clip) = &self.clip else { return div() };
        let choice = clip.choice.clone();
        let views = self.color.views(&choice.display);
        let mut displays = div().id("displays").flex_1().overflow_y_scroll().p(s(6.)).flex().flex_col().border_r_1().border_color(theme::hairline());
        for d in self.color.displays.clone() {
            let selected = d == choice.display;
            let c = choice.clone();
            let dd = d.clone();
            displays = displays.child(
                row(SharedString::from(format!("d-{d}")), selected)
                    .child(div().text_size(s(13.)).child(d.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let views = this.color.views(&dd);
                        let view = if views.contains(&c.view) { c.view.clone() } else { this.color.config.default_view(&dd) };
                        this.set_choice(ColorChoice { display: dd.clone(), view, ..c.clone() });
                        cx.notify();
                    })),
            );
        }
        let mut view_list = div().id("views").flex_1().overflow_y_scroll().p(s(6.)).flex().flex_col();
        for v in views {
            let selected = v == choice.view;
            let c = choice.clone();
            let vv = v.clone();
            view_list = view_list.child(
                row(SharedString::from(format!("v-{v}")), selected)
                    .child(div().flex_1().text_size(s(13.)).child(v.clone()))
                    .when(selected, |d| d.child(icon("check").text_color(theme::blue())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_choice(ColorChoice { view: vv.clone(), ..c.clone() });
                        this.close_modal(window, cx);
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .h(s(360.))
                    .child(div().flex().flex_col().flex_1().child(div().px(s(16.)).pt(s(10.)).child(tag("DISPLAY"))).child(displays))
                    .child(div().flex().flex_col().flex_1().child(div().px(s(16.)).pt(s(10.)).child(tag("VIEW"))).child(view_list)),
            )
            .child(
                div()
                    .px(s(14.))
                    .py(s(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .text_size(s(12.))
                    .text_color(theme::ink_muted())
                    .child(format!("Remembered with the input colorspace for every {} file", clip.info.format.label())),
            )
    }

    fn modal_channel(&mut self, cx: &mut Context<Self>) -> Div {
        let mut list = div().p(s(6.)).flex().flex_col();
        for ch in Channel::ALL {
            let selected = ch == self.channel;
            let key = match ch {
                Channel::Rgb => "C",
                Channel::Red => "R",
                Channel::Green => "G",
                Channel::Blue => "B",
                Channel::Alpha => "A",
                Channel::Luma => "Y",
            };
            let swatch = match ch {
                Channel::Red => theme::marker([0xff, 0x4d, 0x4d]),
                Channel::Green => theme::marker([0x3d, 0xdc, 0x84]),
                Channel::Blue => theme::marker([0x5a, 0x8c, 0xff]),
                Channel::Alpha => theme::marker([0x88, 0x88, 0x88]),
                _ => theme::ink().into(),
            };
            list = list.child(
                row(SharedString::from(format!("ch-{}", ch.short())), selected)
                    .child(div().size(s(10.)).bg(swatch))
                    .child(div().flex_1().text_size(s(13.)).child(ch.label()))
                    .child(div().font_family(theme::font_mono()).text_size(s(11.)).text_color(theme::ink_faint()).child(key))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.channel = ch;
                        this.close_modal(window, cx);
                    })),
            );
        }
        list
    }

    fn modal_compare(&mut self, cx: &mut Context<Self>) -> Div {
        let current_a = self.clip.as_ref().map(|c| c.info.path.clone());
        let mut body = div().flex().flex_col();
        let mode_row = div()
            .px(s(14.))
            .py(s(12.))
            .flex()
            .items_center()
            .gap(s(8.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(tag("MODE"))
            .child(mode_button("mode-wipe", "Wipe", self.compare_mode == CompareMode::Wipe).on_click(cx.listener(|this, _, _, cx| {
                this.compare_mode = CompareMode::Wipe;
                cx.notify();
            })))
            .child(mode_button("mode-side", "Side by side", self.compare_mode == CompareMode::SideBySide).on_click(cx.listener(|this, _, _, cx| {
                this.compare_mode = CompareMode::SideBySide;
                cx.notify();
            })));
        body = body.child(mode_row);

        if let Some(b) = &self.compare {
            body = body.child(
                div()
                    .px(s(14.))
                    .py(s(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(div().flex().flex_col().child(tag("B")).child(div().text_size(s(13.)).child(b.info.name.clone())))
                    .child(button("compare-off", "Turn off").on_click(cx.listener(|this, _, window, cx| {
                        this.set_compare(None, cx);
                        this.close_modal(window, cx);
                    }))),
            );
        }

        let mut list = div().id("compare-list").max_h(s(320.)).overflow_y_scroll().p(s(6.)).flex().flex_col();
        {
            let rows = self.visible_rows();
            for (i, e) in rows.iter().map(|r| &r.entry).enumerate() {
                if matches!(e.kind, EntryKind::Directory) || current_a.as_ref() == Some(&e.path) {
                    continue;
                }
                let entry = e.clone();
                let name = match &e.kind {
                    EntryKind::Sequence(s) => s.pattern(),
                    _ => e.name.clone(),
                };
                let selected = self.compare.as_ref().is_some_and(|c| c.info.path == e.path);
                list = list.child(row(("cmp", i), selected).child(div().text_size(s(13.)).child(name)).on_click(cx.listener(move |this, _, window, cx| {
                    match media::open_entry(&entry) {
                        Ok(s) => this.set_compare(Some(s), cx),
                        Err(e) => this.show_toast(format!("{e:#}")),
                    }
                    this.close_modal(window, cx);
                })));
            }
        }
        body.child(div().px(s(14.)).pt(s(10.)).child(tag("PICK B FROM THE BIN")))
            .child(list)
            .child(
                div()
                    .px(s(14.))
                    .py(s(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(tag("W TOGGLES  ·  SHIFT W SWITCHES MODE  ·  DRAG THE WIPE"))
                    .child(primary_button("compare-browse", "Choose file…").on_click(cx.listener(|this, _, window, cx| {
                        this.close_modal(window, cx);
                        this.prompt_compare(cx);
                    }))),
            )
    }

    fn modal_settings(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let env = std::env::var("OCIO").ok().filter(|v| !v.is_empty());
        let current = self.prefs.ocio.clone();
        let options: Vec<(OcioChoice, String, String)> = vec![
            (OcioChoice::Auto, "Automatic".into(), match &env {
                Some(p) => format!("$OCIO is set: {p}"),
                None => "$OCIO isn't set, so the built-in ACES studio config".into(),
            }),
            (OcioChoice::Builtin, "Built-in ACES studio config".into(), "ocio://studio-config-latest".into()),
            (OcioChoice::Env, "$OCIO".into(), env.clone().unwrap_or_else(|| "not set in this session".into())),
        ];
        let mut configs = div().p(s(6.)).flex().flex_col();
        for (choice, label, detail) in options {
            let selected = current == choice;
            configs = configs.child(
                row(SharedString::from(format!("ocio-{label}")), selected)
                    .child(div().flex().flex_col().flex_1().child(div().text_size(s(13.)).child(label)).child(tag(detail)))
                    .when(selected, |d| d.child(icon("check").text_color(theme::blue())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prefs.ocio = choice.clone();
                        this.prefs.save();
                        this.reload_color(cx);
                    })),
            );
        }
        let custom = match &current {
            OcioChoice::File(p) => Some(p.clone()),
            _ => None,
        };
        configs = configs.child(
            row("ocio-file", custom.is_some())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .child(div().text_size(s(13.)).child("Config file…"))
                        .child(tag(custom.unwrap_or_else(|| "pick any config.ocio".into()))),
                )
                .on_click(cx.listener(|_, _, _, cx| {
                    let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("OCIO config".into()) });
                    cx.spawn(async move |this, cx| {
                        if let Ok(Ok(Some(paths))) = rx.await {
                            if let Some(p) = paths.first() {
                                let p = p.to_string_lossy().to_string();
                                let _ = this.update(cx, |app, cx| {
                                    app.prefs.ocio = OcioChoice::File(p);
                                    app.prefs.save();
                                    app.reload_color(cx);
                                });
                            }
                        }
                    })
                    .detach();
                })),
        );

        let key = self.color.memory_key();
        let memory: Vec<(String, ColorChoice)> = self.prefs.color_memory.get(&key).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        let mut mem_list = div().p(s(6.)).flex().flex_col();
        if memory.is_empty() {
            mem_list = mem_list.child(div().px(s(10.)).py(s(6.)).text_size(s(12.)).text_color(theme::ink_faint()).child("Nothing yet. Pick an input colorspace and it's remembered for that format."));
        }
        for (fmt, choice) in memory {
            let f = fmt.clone();
            let k = key.clone();
            mem_list = mem_list.child(
                div()
                    .px(s(10.))
                    .py(s(5.))
                    .flex()
                    .items_center()
                    .gap(s(10.))
                    .child(div().w(s(110.)).font_family(theme::font_mono()).text_size(s(11.)).child(fmt.to_uppercase()))
                    .child(div().flex_1().min_w_0().text_size(s(12.)).whitespace_nowrap().overflow_hidden().text_ellipsis().child(format!("{}  →  {}", choice.colorspace, choice.view)))
                    .child(icon_button(SharedString::from(format!("forget-{fmt}")), "x", false).size(s(24.)).on_click(cx.listener(move |this, _, _, cx| {
                        this.prefs.forget(&k, &f);
                        this.prefs.save();
                        this.reload_color(cx);
                    }))),
            );
        }

        let disk_on = self.prefs.disk_cache;
        let disk_location = self.prefs.disk_cache_location().to_string_lossy().to_string();
        let disk_detail = match self.cache.disk() {
            Some(d) => format!("{:.1} GB in use", d.used() as f64 / 1024.0 / 1024.0 / 1024.0),
            None => "OFF".to_string(),
        };
        let gb = self.prefs.cache_gb;
        let used = self.cache.used() as f64 / 1024.0 / 1024.0 / 1024.0;
        div()
            .id("settings")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(section("INTERFACE FONT", None))
            .child(
                div()
                    .p(s(6.))
                    .flex()
                    .flex_col()
                    .child(
                        toggle_row("font-sf", "SF Pro", if theme::SF_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed) { "AS IN YOUR NUKE" } else { "NOT INSTALLED" }, theme::font_choice() == theme::FontChoice::SfPro)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if theme::SF_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
                                    this.prefs.ui_font = "sf".into();
                                    this.prefs.save();
                                    theme::apply_font_pref("sf");
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        toggle_row("font-source", "Source Sans", "AS IN HOUDINI · BUNDLED", theme::font_choice() == theme::FontChoice::SourceSans).on_click(cx.listener(|this, _, _, cx| {
                            this.prefs.ui_font = "source".into();
                            this.prefs.save();
                            theme::apply_font_pref("source");
                            cx.notify();
                        })),
                    ),
            )
            .child(section("OCIO CONFIG", Some(self.color.label.clone())))
            .child(configs)
            .child(section("REMEMBERED COLOR BY FORMAT", None))
            .child(mem_list)
            .child(section("RAM CACHE", Some(format!("{used:.1} GB in use"))))
            .child(
                div()
                    .px(s(16.))
                    .py(s(10.))
                    .flex()
                    .items_center()
                    .gap(s(8.))
                    .child(button("cache-dec", "−").w(s(36.)).on_click(cx.listener(|this, _, _, cx| set_cache(this, this.prefs.cache_gb - 2.0, cx))))
                    .child(div().w(s(80.)).flex().justify_center().font_family(theme::font_mono()).text_size(s(16.)).child(format!("{gb:.0} GB")))
                    .child(button("cache-inc", "+").w(s(36.)).on_click(cx.listener(|this, _, _, cx| set_cache(this, this.prefs.cache_gb + 2.0, cx)))),
            )
            .child(section("DISK CACHE", Some(disk_detail)))
            .child(
                div()
                    .p(s(6.))
                    .flex()
                    .flex_col()
                    .child(
                        row("disk-toggle", disk_on)
                            .child(div().flex().flex_col().flex_1().child(div().text_size(s(13.)).child(if disk_on { "On" } else { "Off" })).child(tag("FRAMES EVICTED FROM RAM SPILL HERE · CLEARED WHEN PLATE CLOSES")))
                            .when(disk_on, |d| d.child(icon("check").text_color(theme::blue())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prefs.disk_cache = !this.prefs.disk_cache;
                                this.prefs.save();
                                this.apply_disk_cache();
                                cx.notify();
                            })),
                    )
                    .child(
                        row("disk-location", false)
                            .child(div().flex().flex_col().flex_1().min_w_0().child(div().text_size(s(13.)).child("Location…")).child(tag(disk_location.clone()).whitespace_nowrap().overflow_hidden().text_ellipsis()))
                            .on_click(cx.listener(|_, _, _, cx| {
                                let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Disk cache location".into()) });
                                cx.spawn(async move |this, cx| {
                                    if let Ok(Ok(Some(paths))) = rx.await {
                                        if let Some(p) = paths.first() {
                                            let p = p.to_string_lossy().to_string();
                                            let _ = this.update(cx, |app, cx| {
                                                app.prefs.disk_cache_dir = Some(p);
                                                app.prefs.disk_cache = true;
                                                app.prefs.save();
                                                app.apply_disk_cache();
                                                cx.notify();
                                            });
                                        }
                                    }
                                })
                                .detach();
                            })),
                    ),
            )
            .child(
                div()
                    .px(s(16.))
                    .pb(s(10.))
                    .flex()
                    .items_center()
                    .gap(s(8.))
                    .child(button("disk-dec", "−").w(s(36.)).on_click(cx.listener(|this, _, _, cx| set_disk_gb(this, this.prefs.disk_cache_gb - 16.0, cx))))
                    .child(div().w(s(80.)).flex().justify_center().font_family(theme::font_mono()).text_size(s(16.)).child(format!("{:.0} GB", self.prefs.disk_cache_gb)))
                    .child(button("disk-inc", "+").w(s(36.)).on_click(cx.listener(|this, _, _, cx| set_disk_gb(this, this.prefs.disk_cache_gb + 16.0, cx)))),
            )
            .child(
                div()
                    .px(s(16.))
                    .py(s(12.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .text_size(s(11.))
                    .text_color(theme::ink_faint())
                    .child(format!("Preferences: {}", plate_core::prefs::config_dir().join("prefs.json").display())),
            )
    }

    fn modal_help(&self) -> Div {
        let mut grid = div().p(s(16.)).flex().flex_wrap().gap(s(18.));
        for (group, keys) in SHORTCUTS {
            let mut col = div().w(s(320.)).flex().flex_col().gap(s(4.)).child(div().pb(s(4.)).child(tag(group.to_uppercase())));
            for (k, what) in keys.iter() {
                col = col.child(
                    div()
                        .flex()
                        .gap(s(10.))
                        .child(div().w(s(110.)).font_family(theme::font_mono()).text_size(s(11.)).text_color(theme::blue()).child(crate::actions::shortcut_label(k)))
                        .child(div().flex_1().text_size(s(12.)).text_color(theme::ink_muted()).child(*what)),
                );
            }
            grid = grid.child(col);
        }
        grid
    }

    fn modal_text(&self) -> Div {
        div()
            .p(s(14.))
            .flex()
            .flex_col()
            .gap(s(10.))
            .child(
                div()
                    .px(s(10.))
                    .py(s(8.))
                    .border_1()
                    .border_color(theme::hairline_strong())
                    .rounded(s(theme::RADIUS))
                    .bg(theme::raised())
                    .text_color(theme::marker(self.current_marker()))
                    .child(self.annotation_input.clone()),
            )
            .child(tag("ENTER PLACES IT  ·  ESC CANCELS"))
    }
}

/// A row that toggles one setting, with a check when on.
fn toggle_row(id: impl Into<gpui::ElementId>, label: &str, detail: &str, on: bool) -> gpui::Stateful<Div> {
    row(id, false)
        .child(
            div()
                .size(s(16.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(s(3.))
                .border_1()
                .border_color(if on { theme::blue() } else { theme::hairline_strong() })
                .when(on, |d| d.bg(theme::blue()))
                .when(on, |d| d.child(super::widgets::icon_sized("check", 12.).text_color(theme::paper()))),
        )
        .child(div().flex_1().text_size(s(13.)).child(label.to_string()))
        .child(tag(detail.to_string()))
}

/// A row of mutually exclusive choices.
fn choices<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(String, T)>,
    current: &T,
    cx: &mut Context<Plate>,
    apply: impl Fn(&mut Plate, T) + Clone + 'static,
) -> Div {
    let mut row = div().flex().flex_wrap().gap(s(4.));
    for (i, (label, value)) in options.into_iter().enumerate() {
        let selected = &value == current;
        let apply = apply.clone();
        row = row.child(mode_button_owned(SharedString::from(format!("{id}-{i}")), label, selected).on_click(cx.listener(move |this, _, _, cx| {
            apply(this, value.clone());
            this.prefs.save();
            cx.notify();
        })));
    }
    row
}

fn mode_button_owned(id: SharedString, label: String, selected: bool) -> gpui::Stateful<Div> {
    button(id, label).when(selected, |d| d.bg(theme::structure()).border_color(theme::blue()))
}

impl Plate {
    fn modal_versions(&mut self, cx: &mut Context<Self>) -> Div {
        let (list, at) = self.versions();
        if list.is_empty() {
            return div().p(s(16.)).text_size(s(12.)).text_color(theme::ink_faint()).child("No other versions of this clip in its folder. Versions are found by a v001-style token in the name.");
        }
        let now = std::time::SystemTime::now();
        let mut rows = div().id("version-list").max_h(s(420.)).overflow_y_scroll().p(s(6.)).flex().flex_col();
        for (i, (v, entry)) in list.iter().enumerate().rev() {
            let selected = Some(i) == at;
            let age = std::fs::metadata(&entry.path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .map(|d| match d.as_secs() {
                    0..=59 => "just now".to_string(),
                    60..=3599 => format!("{}m ago", d.as_secs() / 60),
                    3600..=86_399 => format!("{}h ago", d.as_secs() / 3600),
                    s => format!("{}d ago", s / 86_400),
                })
                .unwrap_or_default();
            let e = entry.clone();
            rows = rows.child(
                row(("version", i), selected)
                    .child(div().w(s(44.)).font_family(theme::font_mono()).text_size(s(12.)).text_color(if selected { theme::blue() } else { theme::ink() }).child(format!("v{v:03}")))
                    .child(div().flex_1().min_w_0().text_size(s(12.)).text_color(theme::ink_muted()).whitespace_nowrap().overflow_hidden().text_ellipsis().child(scan_name(entry)))
                    .child(tag(age.to_uppercase()))
                    .when(selected, |d| d.child(icon("check").text_color(theme::blue())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_version(&e, cx);
                        this.close_modal(window, cx);
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(rows)
            .child(
                div()
                    .px(s(14.))
                    .py(s(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(tag("ALT ↑ ↓ STEPS VERSIONS ON THE SAME FRAME"))
                    .child(primary_button("compare-prev", "Compare with previous").on_click(cx.listener(|this, _, window, cx| {
                        this.close_modal(window, cx);
                        this.compare_previous(cx);
                    }))),
            )
    }

    fn modal_layers(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let (layers, current) = self
            .clip
            .as_ref()
            .map(|c| (c.info.layers.clone(), c.info.layer.clone()))
            .unwrap_or_default();
        let mut list = div().id("layer-list").max_h(s(460.)).overflow_y_scroll().p(s(6.)).flex().flex_col();
        for (i, name) in layers.into_iter().enumerate() {
            let selected = current.as_ref() == Some(&name);
            let n = name.clone();
            list = list.child(
                row(("layer", i), selected)
                    .child(div().flex_1().min_w_0().text_size(s(13.)).whitespace_nowrap().overflow_hidden().text_ellipsis().child(name.clone()))
                    .when(i == 0, |d| d.child(tag("BEAUTY")))
                    .when(selected, |d| d.child(icon("check").text_color(theme::blue())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.switch_layer(&n);
                        this.close_modal(window, cx);
                    })),
            );
        }
        list.child(div().px(s(10.)).pt(s(8.)).pb(s(4.)).child(tag("ALT ← →  STEPS LAYERS · THE CHOICE CARRIES TO OTHER FILES")))
    }

    fn modal_guides(&mut self, cx: &mut Context<Self>) -> Div {
        let g = self.prefs.guides.clone();
        let toggle = |id: &'static str, label: &str, detail: &str, on: bool, cx: &mut Context<Plate>, f: fn(&mut plate_core::prefs::Guides)| {
            toggle_row(id, label, detail, on).on_click(cx.listener(move |this, _, _, cx| {
                f(&mut this.prefs.guides);
                this.prefs.guides.visible = true;
                this.prefs.save();
                cx.notify();
            }))
        };
        let aspects: Vec<(String, Option<f32>)> = vec![
            ("Off".into(), None),
            ("2.39".into(), Some(2.39)),
            ("2.35".into(), Some(2.35)),
            ("2.00".into(), Some(2.0)),
            ("1.85".into(), Some(1.85)),
            ("16:9".into(), Some(16.0 / 9.0)),
            ("4:3".into(), Some(4.0 / 3.0)),
            ("1:1".into(), Some(1.0)),
            ("4:5".into(), Some(0.8)),
            ("9:16".into(), Some(9.0 / 16.0)),
        ];
        let grids: Vec<(String, Option<(u32, u32)>)> = vec![
            ("Off".into(), None),
            ("2×2".into(), Some((2, 2))),
            ("4×4".into(), Some((4, 4))),
            ("6×4".into(), Some((6, 4))),
            ("8×8".into(), Some((8, 8))),
            ("12×12".into(), Some((12, 12))),
        ];
        let opacities: Vec<(String, u32)> = vec![("50%".into(), 50), ("75%".into(), 75), ("100%".into(), 100)];
        let current_opacity = (g.mask_opacity * 100.0).round() as u32;
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(s(6.))
                    .flex()
                    .flex_col()
                    .child(toggle_row("g-visible", "Show guides", "'", g.visible).on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.guides.visible = !this.prefs.guides.visible;
                        this.prefs.save();
                        cx.notify();
                    })))
                    .child(toggle("g-thirds", "Rule of thirds", "", g.thirds, cx, |g| g.thirds = !g.thirds))
                    .child(toggle("g-center", "Center cross", "", g.center, cx, |g| g.center = !g.center))
                    .child(toggle("g-action", "Action safe", "93%", g.action_safe, cx, |g| g.action_safe = !g.action_safe))
                    .child(toggle("g-title", "Title safe", "90% · DASHED", g.title_safe, cx, |g| g.title_safe = !g.title_safe)),
            )
            .child(section("ASPECT MASK", Some("GUIDES SIT INSIDE IT".into())))
            .child(div().px(s(16.)).py(s(8.)).child(choices("g-aspect", aspects, &g.aspect, cx, |p, v| {
                p.prefs.guides.aspect = v;
                p.prefs.guides.visible = true;
            })))
            .child(
                div()
                    .px(s(16.))
                    .pb(s(10.))
                    .flex()
                    .items_center()
                    .gap(s(10.))
                    .child(tag("MASK"))
                    .child(choices("g-opacity", opacities, &current_opacity, cx, |p, v| p.prefs.guides.mask_opacity = v as f32 / 100.0)),
            )
            .child(section("GRID", Some("COLUMNS × ROWS".into())))
            .child(div().px(s(16.)).py(s(8.)).pb(s(14.)).child(choices("g-grid", grids, &g.grid, cx, |p, v| {
                p.prefs.guides.grid = v;
                p.prefs.guides.visible = true;
            })))
    }

    fn modal_captions(&mut self, cx: &mut Context<Self>) -> Div {
        let style = self.prefs.captions.clone();
        let current = self.captions.as_ref().map(|c| (c.path.clone(), c.cues.len()));
        let sidecars = self
            .clip
            .as_ref()
            .map(|c| plate_core::captions::find_sidecars(&c.info.path, None))
            .unwrap_or_default();
        let fps = self.fps();
        let offset = self.caption_offset;

        let mut files = div().p(s(6.)).flex().flex_col();
        match &current {
            Some((path, n)) => {
                files = files.child(
                    row("cap-current", true)
                        .child(div().flex().flex_col().flex_1().min_w_0().child(div().text_size(s(13.)).child(path.file_name().unwrap_or_default().to_string_lossy().to_string())).child(tag(format!("{n} CUES"))))
                        .child(icon_button("cap-clear", "x", false).size(s(24.)).on_click(cx.listener(|this, _, _, cx| {
                            this.captions = None;
                            cx.notify();
                        }))),
                );
            }
            None => {
                files = files.child(div().px(s(10.)).py(s(6.)).text_size(s(12.)).text_color(theme::ink_faint()).child("No captions loaded. A clip.srt or clip.vtt next to the media loads automatically."));
            }
        }
        for (i, p) in sidecars.into_iter().enumerate() {
            if current.as_ref().is_some_and(|(c, _)| *c == p) {
                continue;
            }
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            files = files.child(row(("cap-side", i), false).child(div().text_size(s(13.)).child(name)).on_click(cx.listener(move |this, _, _, cx| {
                this.load_captions(&p);
                cx.notify();
            })));
        }

        div()
            .flex()
            .flex_col()
            .child(files)
            .child(
                div()
                    .px(s(16.))
                    .pb(s(10.))
                    .flex()
                    .gap(s(8.))
                    .child(primary_button("cap-load", "Load file…").on_click(cx.listener(|_, _, _, cx| {
                        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Captions (SRT or VTT)".into()) });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = rx.await {
                                if let Some(p) = paths.first().cloned() {
                                    let _ = this.update(cx, |app, cx| {
                                        app.load_captions(&p);
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .detach();
                    })))
                    .child(button("cap-toggle", if style.visible { "Hide  (S)" } else { "Show  (S)" }).on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.captions.visible = !this.prefs.captions.visible;
                        this.prefs.save();
                        cx.notify();
                    }))),
            )
            .child(section("STYLE", None))
            .child(
                div()
                    .px(s(16.))
                    .py(s(10.))
                    .flex()
                    .flex_col()
                    .gap(s(10.))
                    .child(div().flex().items_center().gap(s(10.)).child(div().w(s(70.)).child(tag("SIZE"))).child(choices(
                        "cap-size",
                        vec![("S".into(), 35u32), ("M".into(), 45), ("L".into(), 60)],
                        &((style.size * 1000.0).round() as u32),
                        cx,
                        |p, v| p.prefs.captions.size = v as f32 / 1000.0,
                    )))
                    .child(div().flex().items_center().gap(s(10.)).child(div().w(s(70.)).child(tag("POSITION"))).child(choices(
                        "cap-pos",
                        vec![("Bottom".into(), false), ("Top".into(), true)],
                        &style.top,
                        cx,
                        |p, v| p.prefs.captions.top = v,
                    )))
                    .child(div().flex().items_center().gap(s(10.)).child(div().w(s(70.)).child(tag("BOX"))).child(choices(
                        "cap-box",
                        vec![("On".into(), true), ("Off".into(), false)],
                        &style.boxed,
                        cx,
                        |p, v| p.prefs.captions.boxed = v,
                    ))),
            )
            .child(section("SYNC", Some(format!("{:+.3} S", offset as f64 / fps))))
            .child(
                div()
                    .px(s(16.))
                    .py(s(10.))
                    .pb(s(14.))
                    .flex()
                    .items_center()
                    .gap(s(8.))
                    .child(button("cap-off-dec", "−").w(s(36.)).on_click(cx.listener(|this, _, _, cx| {
                        this.caption_offset -= 1;
                        cx.notify();
                    })))
                    .child(div().w(s(110.)).flex().justify_center().font_family(theme::font_mono()).text_size(s(14.)).child(format!("{offset:+} frames")))
                    .child(button("cap-off-inc", "+").w(s(36.)).on_click(cx.listener(|this, _, _, cx| {
                        this.caption_offset += 1;
                        cx.notify();
                    })))
                    .child(button("cap-off-zero", "Reset").on_click(cx.listener(|this, _, _, cx| {
                        this.caption_offset = 0;
                        cx.notify();
                    }))),
            )
    }
}

fn section(title: &str, detail: Option<String>) -> Div {
    div()
        .px(s(16.))
        .pt(s(14.))
        .pb(s(4.))
        .flex()
        .justify_between()
        .border_t_1()
        .border_color(theme::hairline())
        .child(tag(title.to_string()))
        .children(detail.map(|d| tag(d)))
}

fn mode_button(id: &'static str, label: &'static str, selected: bool) -> gpui::Stateful<Div> {
    button(id, label).when(selected, |d| d.bg(theme::structure()).border_color(theme::blue()))
}

fn set_cache(this: &mut Plate, gb: f32, cx: &mut Context<Plate>) {
    this.prefs.cache_gb = gb.clamp(2.0, 256.0);
    this.prefs.save();
    this.cache.set_budget((this.prefs.cache_gb as f64 * 1024.0 * 1024.0 * 1024.0) as usize);
    this.reconfigure_loaders();
    cx.notify();
}

fn set_disk_gb(this: &mut Plate, gb: f32, cx: &mut Context<Plate>) {
    this.prefs.disk_cache_gb = gb.clamp(8.0, 2048.0);
    this.prefs.save();
    if this.prefs.disk_cache {
        this.apply_disk_cache();
    }
    cx.notify();
}

fn scan_name(e: &plate_core::scan::BinEntry) -> String {
    plate_core::scan::version_name(e)
}
