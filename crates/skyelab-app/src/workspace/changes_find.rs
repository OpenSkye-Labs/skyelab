//! The Changes tab's view toggles (T3's diff header: split or unified, wrap, hide whitespace; saved in settings)
//! and find in diff (mod+F while the tab is focused, the thread's find bar).

use super::ChangesView;
use crate::workspace::changes_model::Row;
use crate::workspace::thread_find::{CloseFind, Find, FindNext, FindPrevious, find_bar};
use gpui::{AnyElement, App, AppContext as _, Context, KeyBinding, Window, actions, prelude::*};
use gpui_component::{
    Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState},
};
use gpui_kit_assets::IconName as Lucide;
use skyelab_core::DiffSettings;

actions!(skyelab, [FindInChanges]);

/// Binds mod+F in a focused Changes tab. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        crate::workspace::thread_find::FIND_KEY,
        FindInChanges,
        Some(super::CONTEXT),
    )]);
}

/// The text a row is searched in.
fn text_of(row: &Row, files: &[crate::workspace::changes_model::ChangedFile]) -> String {
    match row {
        Row::File(i) => files
            .get(*i)
            .map(|f| f.path.display().to_string())
            .unwrap_or_default(),
        Row::Hunk(h) => h.clone(),
        Row::Line { text, .. } => text.clone(),
        Row::Placeholder(_) | Row::TooLarge(_) => String::new(),
    }
}

/// Each match's row, in order (case-insensitive); a row matching twice is listed twice.
pub(super) fn matches(
    rows: &[Row],
    files: &[crate::workspace::changes_model::ChangedFile],
    query: &str,
) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    rows.iter()
        .enumerate()
        .flat_map(|(ix, r)| {
            std::iter::repeat_n(ix, text_of(r, files).to_lowercase().matches(&q).count())
        })
        .collect()
}

impl ChangesView {
    /// Changes one toggle, redraws and saves the result.
    pub(in crate::workspace) fn set_prefs(
        &mut self,
        change: impl FnOnce(&mut DiffSettings),
        cx: &mut Context<Self>,
    ) {
        let before = self.prefs;
        change(&mut self.prefs);
        let prefs = self.prefs;
        crate::workspace::thread_extras::prefs::save_diff(prefs, cx).detach_and_log_err(cx);
        if prefs.ignore_whitespace != before.ignore_whitespace {
            self.refresh(cx);
        }
        self.relayout();
        cx.notify();
    }

    /// The toggles at the right of the header: split/unified, wrap, whitespace.
    pub(super) fn render_toggles(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.prefs;
        let toggle = |id: &'static str, icon: Lucide, on: bool, tip: &'static str| {
            Button::new(id)
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tip)
                .map(|b| if on { b.secondary() } else { b.ghost() })
        };
        h_flex()
            .gap_0p5()
            .child(
                toggle("diff-unified", Lucide::Rows3, !p.split, "Unified diff").on_click(
                    cx.listener(|this, _, _, cx| this.set_prefs(|p| p.split = false, cx)),
                ),
            )
            .child(
                toggle("diff-split", Lucide::Columns2, p.split, "Split diff")
                    .on_click(cx.listener(|this, _, _, cx| this.set_prefs(|p| p.split = true, cx))),
            )
            .child(
                toggle(
                    "diff-wrap",
                    Lucide::TextWrap,
                    p.wrap,
                    if p.wrap {
                        "Disable line wrapping"
                    } else {
                        "Enable line wrapping"
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| this.set_prefs(|p| p.wrap = !p.wrap, cx))),
            )
            .child(
                toggle(
                    "diff-whitespace",
                    Lucide::Pilcrow,
                    p.ignore_whitespace,
                    if p.ignore_whitespace {
                        "Show whitespace changes"
                    } else {
                        "Hide whitespace changes"
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_prefs(|p| p.ignore_whitespace = !p.ignore_whitespace, cx)
                })),
            )
            .into_any_element()
    }

    /// mod+F: opens the find bar (or focuses it again), its query selected.
    pub(super) fn find_in_changes(
        &mut self,
        _: &FindInChanges,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.changes.as_ref().is_none_or(|c| c.rows.is_empty()) {
            return;
        }
        let input = match &self.find.input {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in diff"));
                let sub =
                    cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| match e {
                        InputEvent::Change => this.find_changed(cx),
                        InputEvent::PressEnter { shift, .. } => this.find_step(!*shift, cx),
                        _ => {}
                    });
                self.find = Find {
                    input: Some(input.clone()),
                    _sub: Some(sub),
                    ..Find::default()
                };
                input
            }
        };
        input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
        cx.notify();
    }

    pub(super) fn close_find(
        &mut self,
        _: &CloseFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find = Find::default();
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.find_step(true, cx);
    }

    pub(super) fn find_previous(
        &mut self,
        _: &FindPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_step(false, cx);
    }

    /// A new query (or a reload): count again from the first match. The field also reports a change on Enter with
    /// the same text: that one keeps the place.
    fn find_changed(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.find.input else {
            return;
        };
        let query = input.read(cx).value().to_string();
        if query == self.find.query {
            return;
        }
        self.recount(query);
        self.reveal_match(cx);
    }

    /// Counts `query`'s matches in the loaded rows, from the first.
    pub(super) fn recount(&mut self, query: String) {
        let found = self
            .changes
            .as_ref()
            .map(|c| matches(&c.rows, &c.files, &query))
            .unwrap_or_default();
        (self.find.matches, self.find.query, self.find.active) = (found, query, 0);
    }

    /// The next (or previous) match, wrapping around.
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let n = self.find.matches.len();
        self.find.active = match n {
            0 => 0,
            _ if forward => (self.find.active + 1) % n,
            _ => (self.find.active + n - 1) % n,
        };
        self.reveal_match(cx);
    }

    fn reveal_match(&mut self, cx: &mut Context<Self>) {
        if let Some(&row) = self.find.matches.get(self.find.active) {
            self.scroll_to_row(row);
        }
        cx.notify();
    }

    pub(super) fn render_find(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        find_bar(
            &self.find,
            cx,
            |this, forward, cx| this.find_step(forward, cx),
            |this, window, cx| this.close_find(&CloseFind, window, cx),
        )
    }
}
