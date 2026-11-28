//! Find in thread (T3's ThreadFindBar): mod+F opens a bar over the timeline. It counts every match of the query
//! (case-insensitive) in the thread's rows, steps through them with Enter / Shift-Enter, scrolls to each, opens the
//! folded turn or collapsed row it sits in, and marks the matched words (other rows that match are tinted). Esc closes it.

use super::find_marks::word_ranges;
use super::thread::{CONTEXT, ThreadView};
use super::thread_items::ItemsView;
use super::thread_model::Item;
use super::timeline_fold::{fold_key, folded_by};
use super::turn_files::minimap::markdown_plain;
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::h_flex;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, KeyBinding, ListState, Subscription, Window,
    actions, div, prelude::*, px,
};
use gpui_component::{
    Sizable as _,
    input::{Input, InputEvent, InputState},
};
use skyelab_agent::DeltaKind;

actions!(skyelab, [FindInThread, FindNext, FindPrevious, CloseFind]);

const FIND_CONTEXT: &str = "ThreadFind";
pub(super) const FIND_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-f"
} else {
    "ctrl-f"
};

/// Binds mod+F in a thread and a file preview, and Esc in the find bar. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(FIND_KEY, FindInThread, Some(CONTEXT)),
        KeyBinding::new(FIND_KEY, FindInThread, Some(super::remote_thread::CONTEXT)),
        // A file preview's editor takes mod+F first (its own search), and passes it on: it isn't searchable.
        KeyBinding::new(FIND_KEY, FindInThread, Some(super::files::PREVIEW_CONTEXT)),
        KeyBinding::new("escape", CloseFind, Some(FIND_CONTEXT)),
    ]);
}

#[derive(Default)]
pub struct Find {
    /// The query field, while the bar is open.
    pub input: Option<Entity<InputState>>,
    /// The row of each match, in order: a row matching twice is listed twice.
    pub matches: Vec<usize>,
    /// Which of `matches` is current.
    pub active: usize,
    /// The query `matches` were counted for.
    pub query: String,
    pub _sub: Option<Subscription>,
}

impl Find {
    /// `Some(true)` for the current match's row, `Some(false)` for another matching row.
    pub fn mark(&self, ix: usize) -> Option<bool> {
        self.words(ix).map(|(_, active)| active.is_some())
    }

    /// For a matching row: the query, and which of the row's matches is the current one (`None` on another row).
    pub fn words(&self, ix: usize) -> Option<(&str, Option<usize>)> {
        self.input.as_ref()?;
        let first = self.matches.partition_point(|&m| m < ix);
        if self.matches.get(first) != Some(&ix) {
            return None;
        }
        let active = (self.matches.get(self.active) == Some(&ix)).then(|| self.active - first);
        Some((&self.query, active))
    }

    /// "2/7", "0/0", or nothing before a query.
    pub fn count(&self, query: &str) -> String {
        match (query.trim().is_empty(), self.matches.len()) {
            (true, _) => String::new(),
            (false, 0) => "0/0".into(),
            (false, n) => format!("{}/{n}", self.active + 1),
        }
    }
}

/// The text a row is searched in, folded or not: a tool's whole input and result, not only its one-line summary. A
/// reply is searched as drawn (its markdown's rendered text, the text find marks), so the count and the marks agree.
pub fn text_of(item: &Item) -> String {
    let tool = |t: &super::thread_model::ToolItem| {
        format!(
            "{} {} {}",
            t.name,
            t.input,
            t.result.as_deref().unwrap_or_default()
        )
    };
    match item {
        Item::Text(t)
        | Item::Live {
            kind: DeltaKind::Text,
            text: t,
        } => markdown_plain(t),
        Item::Prompt(t) | Item::Steer(t) => t.clone(),
        Item::Live { text, .. } | Item::Thinking { text, .. } | Item::Output { text, .. } => {
            text.clone()
        }
        Item::Notice { text, .. } => text.clone(),
        Item::Tool(t) => tool(t),
        Item::Subagent(s) => {
            let calls: Vec<String> = s.tools.iter().map(tool).collect();
            let summary = s.summary.as_deref().unwrap_or_default();
            format!("{} {} {summary} {}", s.kind, s.description, calls.join(" "))
        }
        Item::Todo(todos) => todos
            .iter()
            .map(|t| t.content.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Item::Approval(a) => format!("{} {}", a.tool, a.input),
        Item::Done { error, .. } => error.clone().unwrap_or_default(),
        Item::Limit { .. } | Item::Interrupted => String::new(),
    }
}

/// Each match's row, in order (case-insensitive, as the marks find them). An empty query matches nothing.
pub fn matches(items: &[Item], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    items
        .iter()
        .enumerate()
        // shortcut: parses every reply's markdown on each count (a keystroke, a step); cache the rendered text per row
        // if find lags on long threads.
        .flat_map(|(ix, i)| std::iter::repeat_n(ix, word_ranges(&text_of(i), query).len()))
        .collect()
}

/// A view with a timeline the find bar searches: a local thread or another machine's.
pub(super) trait FindHost: ItemsView {
    fn find(&self) -> &Find;
    fn find_mut(&mut self) -> &mut Find;
    fn list_state(&self) -> &ListState;
    /// Where focus goes once the bar closes (the composer).
    fn focus_after_find(&self, window: &mut Window, cx: &mut App);

    /// mod+F: opens the bar (or focuses it again), its query selected.
    fn find_in_thread(&mut self, _: &FindInThread, window: &mut Window, cx: &mut Context<Self>) {
        if self.items().is_empty() {
            return; // nothing to find, and the bar only draws over a timeline
        }
        open_bar(
            self.find_mut(),
            "Find in thread",
            window,
            cx,
            Self::find_query_changed,
            Self::find_step,
        );
    }

    fn close_find(&mut self, _: &CloseFind, window: &mut Window, cx: &mut Context<Self>) {
        *self.find_mut() = Find::default();
        self.focus_after_find(window, cx);
        cx.notify();
    }

    fn find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.find_step(true, cx);
    }

    fn find_previous(&mut self, _: &FindPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.find_step(false, cx);
    }

    fn find_query(&self, cx: &App) -> String {
        self.find()
            .input
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// A new query: count again and show the first match. The field also reports a change on Enter, with the same
    /// text: that one keeps the place.
    fn find_query_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.find_query(cx);
        if query == self.find().query {
            return;
        }
        let found = matches(self.items(), &query);
        let f = self.find_mut();
        f.matches = found;
        f.query = query;
        f.active = 0;
        self.reveal_match(cx);
    }

    /// The next (or previous) match, wrapping around. Rows may have changed since the count, so it counts again.
    fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.find_rows_changed();
        let f = self.find_mut();
        let n = f.matches.len();
        f.active = match n {
            0 => 0,
            _ if forward => (f.active + 1) % n,
            _ => (f.active + n - 1) % n,
        };
        self.reveal_match(cx);
    }

    /// Rows streamed in or changed while the bar is open: count again, keeping the place (the first match at or
    /// after the current one's row).
    fn find_rows_changed(&mut self) {
        if self.find().input.is_none() {
            return;
        }
        let now = matches(self.items(), &self.find().query);
        let f = self.find_mut();
        if now == f.matches {
            return;
        }
        let row = f.matches.get(f.active).copied();
        f.active = row
            .and_then(|r| now.iter().position(|&m| m >= r))
            .unwrap_or(0);
        f.matches = now;
    }

    /// Scrolls to the current match, opening the fold and the row it is in.
    fn reveal_match(&mut self, cx: &mut Context<Self>) {
        let Some(&ix) = self.find().matches.get(self.find().active) else {
            return cx.notify();
        };
        if let Some(done) = folded_by(self.items(), ix) {
            let key = fold_key(self.items(), done);
            if !self.is_expanded(&key) {
                self.toggle_tool(&key, cx);
            }
        }
        let collapsed = match &self.items()[ix] {
            Item::Tool(_) | Item::Subagent(_) | Item::Thinking { .. } | Item::Output { .. } => {
                self.items()[ix].expand_id().map(str::to_owned)
            }
            _ => None,
        };
        if let Some(id) = collapsed.filter(|id| !self.is_expanded(id)) {
            self.toggle_tool(&id, cx);
        }
        // A reveal alone keeps tail-follow, which puts the list back at its end on the next frame.
        self.list_state().pause_following_tail();
        self.list_state().scroll_to_reveal_item(ix);
        cx.notify();
    }

    /// The bar, above the timeline on its right.
    fn render_find(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        find_bar(
            self.find(),
            cx,
            |this, forward, cx| this.find_step(forward, cx),
            |this, window, cx| this.close_find(&CloseFind, window, cx),
        )
    }
}

/// Opens `find`'s bar (or focuses it again), its query selected. The field reports each edit to `changed` and Enter /
/// Shift-Enter to `step` (forward or back). The thread, the Changes tab and the file preview open theirs with it.
pub(super) fn open_bar<V: 'static>(
    find: &mut Find,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<V>,
    changed: fn(&mut V, &mut Context<V>),
    step: fn(&mut V, bool, &mut Context<V>),
) {
    let input = match &find.input {
        Some(input) => input.clone(),
        None => {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let sub = cx.subscribe_in(
                &input,
                window,
                move |this, _, e: &InputEvent, _, cx| match e {
                    InputEvent::Change => changed(this, cx),
                    InputEvent::PressEnter { shift, .. } => step(this, !*shift, cx),
                    _ => {}
                },
            );
            *find = Find {
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

/// A find bar for a view's top right (T3: 12 px in, popover surface): the query field, the "i/n" count, step
/// buttons and close. `None` while `find` is closed. The Changes tab draws the same bar. It takes its own strip
/// above the content rather than floating over it, so it never hides the first row (a message, a file header).
pub(super) fn find_bar<V: 'static>(
    f: &Find,
    cx: &mut Context<V>,
    step: impl Fn(&mut V, bool, &mut Context<V>) + Copy + 'static,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Option<AnyElement> {
    let input = f.input.clone()?;
    let query = input.read(cx).value().to_string();
    let theme = cx.theme();
    let none = !query.trim().is_empty() && f.matches.is_empty();
    let step_button = |id: &'static str, tip: &'static str, icon: SkyeIcon, forward: bool| {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(icon)
            .tooltip(tip)
            .disabled(f.matches.is_empty())
            .on_click(cx.listener(move |this, _, _, cx| step(this, forward, cx)))
    };
    Some(
        h_flex()
            .flex_none()
            .w_full()
            .justify_end()
            .px_3()
            .pt_3()
            .child(
                div().w(px(360.)).max_w_full().child(
                    h_flex()
                        .id("find-bar")
                        .role(gpui::accesskit::Role::Search)
                        .aria_label("Find")
                        .debug_selector(|| "find-bar".into())
                        .key_context(FIND_CONTEXT)
                        .gap_1()
                        .pl_1()
                        .pr_1()
                        .py_0p5()
                        .rounded(ui::RADIUS_LG)
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .shadow_md()
                        .child(
                            div().flex_1().child(
                                Input::new(&input)
                                    .appearance(false)
                                    .small()
                                    .aria_label("Find query"),
                            ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "find-count".into())
                                .min_w(px(40.))
                                .text_size(ui::META)
                                .text_color(if none {
                                    ui::ink(theme, theme.danger)
                                } else {
                                    theme.muted_foreground
                                })
                                .child(f.count(&query)),
                        )
                        .child(step_button(
                            "find-prev",
                            "Previous match",
                            SkyeIcon::ChevronUp,
                            false,
                        ))
                        .child(step_button(
                            "find-next",
                            "Next match",
                            SkyeIcon::ChevronDown,
                            true,
                        ))
                        .child(
                            Button::new("find-close")
                                .ghost()
                                .xsmall()
                                .icon(SkyeIcon::Close)
                                .tooltip("Close find")
                                .on_click(
                                    cx.listener(move |this, _, window, cx| close(this, window, cx)),
                                ),
                        ),
                ),
            )
            .into_any_element(),
    )
}

impl FindHost for ThreadView {
    fn find(&self) -> &Find {
        &self.timeline.find
    }
    fn find_mut(&mut self) -> &mut Find {
        &mut self.timeline.find
    }
    fn list_state(&self) -> &ListState {
        self.own_list()
    }
    fn focus_after_find(&self, window: &mut Window, cx: &mut App) {
        self.focus_prompt(window, cx);
    }
}
