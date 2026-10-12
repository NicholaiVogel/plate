//! The sidebar: a media tree in the style of a code editor's explorer.

use super::widgets::{icon_button, icon_sized, row, tag, TipExt};
use crate::app::{Plate, Row, RowKind};
use crate::theme::{self, s};
use gpui::{div, prelude::*, Context, FontWeight, MouseButton, Window};
use plate_core::scan::{self, BinEntry, EntryKind};

const INDENT: f32 = 14.0;
const ROW_HEIGHT: f32 = 26.0;

impl Plate {
    fn is_open(&self, e: &BinEntry) -> bool {
        let Some(c) = self.clip.as_ref().map(|c| &c.info.path) else { return false };
        match &e.kind {
            EntryKind::Sequence(seq) => {
                c.parent() == Some(seq.dir.as_path())
                    && c.file_name().map(|n| n.to_string_lossy().to_string()).is_some_and(|n| n.starts_with(&seq.prefix) && n.ends_with(&seq.suffix))
            }
            _ => c == &e.path,
        }
    }

    fn open_stack_key(&self) -> Option<String> {
        self.clip_version_name().and_then(|n| scan::version_of(&n)).map(|(k, _)| k)
    }

    pub(super) fn render_bin(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(bin) = &self.bin else { return div() };
        let title = bin.title.clone();
        let root = bin.dir.clone();
        let parent = root.as_ref().and_then(|d| d.parent().map(|p| p.to_path_buf()));
        let rows = self.visible_rows();
        let open_key = self.open_stack_key();

        let selected: Vec<bool> = rows
            .iter()
            .map(|r| match &r.kind {
                RowKind::StackHead { key, expanded, .. } => Some(key) == open_key.as_ref() && (!*expanded || self.is_open(&r.entry)),
                RowKind::Folder { .. } => false,
                _ => self.is_open(&r.entry),
            })
            .collect();
        if self.reveal_pending {
            if let Some(ix) = selected.iter().position(|s| *s) {
                self.bin_scroll.scroll_to_item(ix);
            }
            self.reveal_pending = false;
        }

        let mut list = div().id("bin-list").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.bin_scroll).py(s(4.)).px(s(4.)).flex().flex_col();
        for (i, r) in rows.iter().enumerate() {
            list = list.child(self.tree_row(i, r, selected[i], open_key.as_deref(), cx));
        }
        if rows.is_empty() {
            list = list.child(div().p(s(12.)).text_color(theme::ink_faint()).text_size(s(12.)).child("No media in this folder"));
        }

        div()
            .w(s(280.))
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
                    .gap(s(4.))
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(
                        div()
                            .id("bin-title")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(div().font_weight(FontWeight::MEDIUM).whitespace_nowrap().overflow_hidden().text_ellipsis().child(title))
                            .child(tag(root.as_ref().map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|| "OPENED FILES".into())).whitespace_nowrap().overflow_hidden().text_ellipsis())
                            .when_some(root.clone(), |d, r| d.tip(r.to_string_lossy().to_string())),
                    )
                    .when_some(parent, |d, parent| {
                        d.child(icon_button("bin-up", "up", false).tip("Up a folder").on_click(cx.listener(move |this, _, _, cx| {
                            this.set_root(&parent, cx);
                            cx.notify();
                        })))
                    })
                    .child(icon_button("bin-collapse", "collapse", false).tip("Collapse all").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(b) = this.bin.as_mut() {
                            b.expanded.clear();
                        }
                        this.expanded_stacks.clear();
                        cx.notify();
                    })))
                    .child(icon_button("bin-open", "folder", false).tip(crate::actions::shortcut_label("Open folder  (Ctrl Shift O)")).on_click(cx.listener(|this, _, _, cx| this.prompt_open(true, cx))))
                    .child(icon_button("bin-hide", "sidebar", true).tip("Hide  (Shift Tab)").on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.show_bin = false;
                        cx.notify();
                    }))),
            )
            .child(list)
    }

    fn tree_row(&self, i: usize, r: &Row, selected: bool, open_key: Option<&str>, cx: &mut Context<Self>) -> impl IntoElement {
        let e = &r.entry;
        let (glyph, note, warn) = match &e.kind {
            EntryKind::Directory => ("folder", None, false),
            EntryKind::Dcp => ("projector", Some("DCP".to_string()), false),
            EntryKind::Video => ("film", None, false),
            EntryKind::Still => ("image", None, false),
            EntryKind::Sequence(seq) => {
                let missing = seq.missing();
                if missing > 0 {
                    ("layers", Some(format!("{missing} MISSING")), true)
                } else {
                    ("layers", Some(format!("{}–{}", seq.first, seq.last)), false)
                }
            }
        };
        let note = match &r.kind {
            RowKind::StackHead { count, .. } => Some(format!("{count} VERSIONS")),
            _ => note,
        };

        // Twisty for folders and stacks; a blank of the same width otherwise,
        // so names line up. A stack's twisty expands it; the rest of its row
        // opens the latest version.
        let mut twisty = div().id(("twisty", i)).w(s(16.)).h_full().flex_none().flex().items_center().justify_center();
        match &r.kind {
            RowKind::Folder { expanded } => {
                twisty = twisty.child(icon_sized(if *expanded { "chevron-down" } else { "chevron-right" }, 12.).text_color(theme::ink_faint()));
            }
            RowKind::StackHead { key, expanded, .. } => {
                let key = key.clone();
                twisty = twisty
                    .rounded(s(3.))
                    .hover(|st| st.bg(theme::hover()))
                    .child(icon_sized(if *expanded { "chevron-down" } else { "chevron-right" }, 12.).text_color(theme::ink_muted()))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.expanded_stacks.remove(&key) {
                            this.expanded_stacks.insert(key.clone());
                        }
                        cx.notify();
                    }));
            }
            _ => {}
        }

        let mut line = row(("tree", i), selected).h(s(ROW_HEIGHT)).py(s(0.)).px(s(4.)).gap(s(0.));
        // Indent guides, one per level, joining across rows.
        for _ in 0..r.depth {
            line = line.child(div().w(s(INDENT)).h_full().flex_none().flex().justify_center().child(div().w(s(1.)).h_full().bg(theme::hairline())));
        }
        line = line
            .child(twisty)
            .child(
                icon_sized(glyph, 14.)
                    .mr(s(6.))
                    .text_color(if selected { theme::blue() } else if matches!(r.kind, RowKind::Folder { .. }) { theme::ink_muted() } else { theme::ink_faint() }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(s(12.))
                    .text_color(if selected || matches!(r.kind, RowKind::Folder { .. }) { theme::ink() } else { theme::ink_muted() })
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(scan::version_name(e)),
            )
            .children(note.map(|t| tag(t).flex_none().ml(s(6.)).when(warn, |d| d.text_color(theme::warn()))));

        let entry = e.clone();
        let kind = r.kind.clone();
        let same_stack = matches!(r.kind, RowKind::StackHead { .. } | RowKind::StackMember)
            && open_key.is_some()
            && scan::version_of(&scan::version_name(e)).map(|(k, _)| k).as_deref() == open_key;
        line.on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
            match &kind {
                RowKind::Folder { .. } => {
                    if ev.click_count >= 2 {
                        // Double-click: this folder becomes the root.
                        this.set_root(&entry.path, cx);
                    } else {
                        this.toggle_folder(&entry.path);
                    }
                }
                _ => {
                    if same_stack {
                        this.open_version(&entry, cx);
                    } else {
                        this.open_entry(&entry, cx);
                    }
                    this.focus_viewer(window);
                }
            }
            cx.notify();
        }))
    }
}
