use super::widgets::{icon_button, icon_sized, row, tag, TipExt};
use std::collections::{HashMap, HashSet};
use crate::app::{dir_title, Bin, Plate};
use crate::theme::{self, s};
use gpui::{div, prelude::*, Context, FontWeight, Window};
use plate_core::scan::{self, EntryKind};

impl Plate {
    pub(super) fn render_bin(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(bin) = &self.bin else { return div() };
        let current = self.clip.as_ref().map(|c| c.info.path.clone());
        let parent = bin.dir.as_ref().and_then(|d| d.parent().map(|p| p.to_path_buf()));

        let is_current = |e: &scan::BinEntry| {
            current.as_ref().is_some_and(|c| match &e.kind {
                EntryKind::Sequence(seq) => {
                    c.parent() == Some(seq.dir.as_path())
                        && c.file_name().map(|n| n.to_string_lossy().to_string()).is_some_and(|n| n.starts_with(&seq.prefix) && n.ends_with(&seq.suffix))
                }
                _ => *c == e.path,
            })
        };

        // Stack versions of the same shot: v001…v009 become one row showing
        // the latest, expandable to the rest.
        let mut stacks: HashMap<String, Vec<(u32, usize)>> = HashMap::new();
        for (i, e) in bin.entries.iter().enumerate() {
            if !e.is_playable() {
                continue;
            }
            if let Some((key, v)) = scan::version_of(&scan::version_name(e)) {
                stacks.entry(key).or_default().push((v, i));
            }
        }
        stacks.retain(|_, v| v.len() > 1);
        for v in stacks.values_mut() {
            v.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
        }
        let stack_of = |i: usize| stacks.iter().find(|(_, v)| v.iter().any(|(_, j)| *j == i)).map(|(k, v)| (k.clone(), v.clone()));

        let mut list = div().id("bin-list").flex_1().min_h_0().overflow_y_scroll().p(s(6.)).flex().flex_col();
        let mut emitted: HashSet<String> = HashSet::new();
        for (i, e) in bin.entries.iter().enumerate() {
            match stack_of(i) {
                None => list = list.child(self.bin_row(i, e, is_current(e), 0, None, cx)),
                Some((key, members)) => {
                    if !emitted.insert(key.clone()) {
                        continue;
                    }
                    let (latest_v, latest_i) = members[0];
                    let latest = &bin.entries[latest_i];
                    let holds_current = members.iter().any(|(_, j)| is_current(&bin.entries[*j]));
                    let expanded = self.expanded_stacks.contains(&key);
                    let badge = format!("{} VERSIONS", members.len());
                    list = list.child(self.bin_row(latest_i, latest, holds_current && (!expanded || is_current(latest)), 0, Some((key.clone(), expanded, badge, latest_v)), cx));
                    if expanded {
                        for (_, j) in members.iter().skip(1) {
                            let m = &bin.entries[*j];
                            list = list.child(self.bin_row(*j, m, is_current(m), 1, None, cx));
                        }
                    }
                }
            }
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

    /// One bin row. `depth` indents versions under their stack; `stack` makes
    /// this the stack's head row (key, expanded, badge, version).
    fn bin_row(
        &self,
        i: usize,
        e: &scan::BinEntry,
        selected: bool,
        depth: u8,
        stack: Option<(String, bool, String, u32)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // The name already carries the extension; only sequences and DCPs
        // get a quiet right-hand note.
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
        let name = scan::version_name(e);
        let sub = match &stack {
            Some((_, _, badge, _)) => Some(badge.clone()),
            None => sub,
        };
        let entry = e.clone();
        let in_stack = stack.is_some() || depth > 0;
        let mut r = row(("bin", i), selected).py(s(5.)).when(depth > 0, |d| d.pl(s(30.)));
        r = match &stack {
            Some((key, expanded, _, _)) => {
                let key = key.clone();
                r.child(
                    div()
                        .id(("stack-toggle", i))
                        .size(s(16.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(s(3.))
                        .hover(|st| st.bg(theme::hover()))
                        .child(icon_sized(if *expanded { "chevron-down" } else { "chevron-right" }, 12.).text_color(theme::ink_muted()))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !this.expanded_stacks.remove(&key) {
                                this.expanded_stacks.insert(key.clone());
                            }
                            cx.notify();
                        })),
                )
            }
            None => r.child(icon_sized(glyph, 14.).text_color(if selected { theme::blue() } else { theme::ink_faint() })),
        };
        r.child(
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
            // Moving within a stack keeps the frame and view, like Alt ↑/↓.
            let same_stack = in_stack
                && this.clip_version_name().and_then(|n| scan::version_of(&n)).map(|(k, _)| k)
                    == scan::version_of(&scan::version_name(&entry)).map(|(k, _)| k);
            if same_stack {
                this.open_version(&entry, cx);
            } else {
                this.open_entry(&entry, cx);
            }
            this.focus_viewer(window);
        }))
    }
}
