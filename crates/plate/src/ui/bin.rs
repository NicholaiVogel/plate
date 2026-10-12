use super::widgets::{icon_button, icon_sized, row, tag, TipExt};
use crate::app::{dir_title, Bin, Plate};
use crate::theme::{self, s};
use gpui::{div, prelude::*, Context, FontWeight, Window};
use plate_core::scan::{self, EntryKind};

impl Plate {
    pub(super) fn render_bin(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(bin) = &self.bin else { return div() };
        let current = self.clip.as_ref().map(|c| c.info.path.clone());
        let parent = bin.dir.as_ref().and_then(|d| d.parent().map(|p| p.to_path_buf()));

        let mut list = div().id("bin-list").flex_1().min_h_0().overflow_y_scroll().p(s(6.)).flex().flex_col();
        for (i, e) in bin.entries.iter().enumerate() {
            let selected = current.as_ref().is_some_and(|c| match &e.kind {
                EntryKind::Sequence(seq) => {
                    c.parent() == Some(seq.dir.as_path())
                        && c.file_name().map(|n| n.to_string_lossy().to_string()).is_some_and(|n| n.starts_with(&seq.prefix) && n.ends_with(&seq.suffix))
                }
                _ => *c == e.path,
            });
            // The name already carries the extension; only sequences and
            // DCPs get a quiet right-hand note.
            let (glyph, sub, warn) = match &e.kind {
                EntryKind::Directory => ("folder", None, false),
                EntryKind::Dcp => ("projector", Some("DCP".to_string()), false),
                EntryKind::Video => ("film", None, false),
                EntryKind::Still => ("image", None, false),
                EntryKind::Sequence(seq) => {
                    let missing = seq.missing();
                    if missing > 0 {
                        ("layers", Some(format!("{} MISSING", missing)), true)
                    } else {
                        ("layers", Some(format!("{}–{}", seq.first, seq.last)), false)
                    }
                }
            };
            let name = match &e.kind {
                EntryKind::Sequence(seq) => seq.pattern(),
                _ => e.name.clone(),
            };
            let entry = e.clone();
            list = list.child(
                row(("bin", i), selected)
                    .py(s(5.))
                    .child(icon_sized(glyph, 14.).text_color(if selected { theme::blue() } else { theme::ink_faint() }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(s(12.))
                            .text_color(if selected { theme::ink() } else { theme::ink_muted() })
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(name),
                    )
                    .children(sub.map(|t| tag(t).flex_none().when(warn, |d| d.text_color(theme::warn()))))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_entry(&entry, cx);
                        this.focus_viewer(window);
                    })),
            );
        }
        if bin.entries.is_empty() {
            list = list.child(div().p(s(12.)).text_color(theme::ink_faint()).text_size(s(12.)).child("No media in this folder"));
        }

        div()
            .w(s(270.))
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .h(s(44.))
                    .flex_none()
                    .px(s(10.))
                    .flex()
                    .items_center()
                    .gap(s(6.))
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(div().font_weight(FontWeight::MEDIUM).whitespace_nowrap().overflow_hidden().text_ellipsis().child(bin.title.clone()))
                            .child(tag(format!("{} ITEMS", bin.entries.len()))),
                    )
                    .when_some(parent, |d, parent| {
                        d.child(icon_button("bin-up", "up", false).tip("Parent folder").on_click(cx.listener(move |this, _, _, cx| {
                            if let Ok(entries) = scan::scan_dir(&parent) {
                                this.set_bin(Bin { dir: Some(parent.clone()), title: dir_title(&parent), entries }, cx);
                                cx.notify();
                            }
                        })))
                    })
                    .child(icon_button("bin-open", "folder", false).tip("Open folder  (Ctrl Shift O)").on_click(cx.listener(|this, _, _, cx| this.prompt_open(true, cx))))
                    .child(icon_button("bin-hide", "sidebar", true).tip("Hide bin  (Shift Tab)").on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.show_bin = false;
                        cx.notify();
                    }))),
            )
            .child(list)
    }
}

