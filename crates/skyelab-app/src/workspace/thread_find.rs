//! Find in thread (T3's ThreadFindBar): mod+F opens a bar over the timeline. It counts every match of the query
//! (case-insensitive) in the thread's rows, steps through them with Enter / Shift-Enter, scrolls to each, opens the
//! folded turn or collapsed row it sits in, and tints the rows that match. Esc closes it.

use super::thread::{CONTEXT, ThreadView};
use super::thread_model::Item;
use super::timeline_fold::{fold_key, folded_by};
use crate::ui;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, KeyBinding, Subscription, Window, actions,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
};

actions!(skyelab, [FindInThread, FindNext, FindPrevious, CloseFind]);

const FIND_CONTEXT: &str = "ThreadFind";
const FIND_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-f"
} else {
    "ctrl-f"
};

/// Binds mod+F in a thread and Esc in its find bar. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(FIND_KEY, FindInThread, Some(CONTEXT)),
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
    query: String,
    _sub: Option<Subscription>,
}

impl Find {
    /// `Some(true)` for the current match's row, `Some(false)` for another matching row.
    pub fn mark(&self, ix: usize) -> Option<bool> {
        self.input.as_ref()?;
        if self.matches.get(self.active) == Some(&ix) {
            return Some(true);
        }
        self.matches.binary_search(&ix).is_ok().then_some(false)
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

/// The text a row is searched in, folded or not: a tool's whole input and result, not only its one-line summary.
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
        Item::Prompt(t) | Item::Steer(t) | Item::Text(t) => t.clone(),
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

/// Each match's row, in order (case-insensitive). An empty query matches nothing.
pub fn matches(items: &[Item], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    items
        .iter()
        .enumerate()
        .flat_map(|(ix, i)| std::iter::repeat_n(ix, text_of(i).to_lowercase().matches(&q).count()))
        .collect()
}

impl ThreadView {
    /// mod+F: opens the bar (or focuses it again), its query selected.
    pub(super) fn find_in_thread(
        &mut self,
        _: &FindInThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = match &self.timeline.find.input {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in thread"));
                let sub =
                    cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| match e {
                        InputEvent::Change => this.find_query_changed(cx),
                        InputEvent::PressEnter { shift, .. } => this.find_step(!*shift, cx),
                        _ => {}
                    });
                self.timeline.find = Find {
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
        self.timeline.find = Find::default();
        self.focus_prompt(window, cx);
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

    fn find_query(&self, cx: &App) -> String {
        let f = &self.timeline.find;
        f.input
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// A new query: count again and show the first match. The field also reports a change on Enter, with the same
    /// text: that one keeps the place.
    fn find_query_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.find_query(cx);
        if query == self.timeline.find.query {
            return;
        }
        self.timeline.find.matches = matches(&self.items, &query);
        self.timeline.find.query = query;
        self.timeline.find.active = 0;
        self.reveal_match(cx);
    }

    /// The next (or previous) match, wrapping around. Rows may have changed since the count, so it counts again.
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let query = self.find_query(cx);
        let now = matches(&self.items, &query);
        let f = &mut self.timeline.find;
        let n = now.len();
        f.active = match (n, f.matches == now) {
            (0, _) => 0,
            (_, false) => 0,
            (_, true) if forward => (f.active + 1) % n,
            (_, true) => (f.active + n - 1) % n,
        };
        f.matches = now;
        self.reveal_match(cx);
    }

    /// Scrolls to the current match, opening the fold and the row it is in.
    fn reveal_match(&mut self, cx: &mut Context<Self>) {
        let Some(&ix) = self.timeline.find.matches.get(self.timeline.find.active) else {
            return cx.notify();
        };
        if let Some(done) = folded_by(&self.items, ix) {
            let key = fold_key(done);
            if !self.is_expanded(&key) {
                self.toggle_tool(&key, cx);
            }
        }
        let collapsed = match &self.items[ix] {
            Item::Tool(_) | Item::Subagent(_) | Item::Thinking { .. } | Item::Output { .. } => {
                self.items[ix].expand_id().map(str::to_owned)
            }
            _ => None,
        };
        if let Some(id) = collapsed.filter(|id| !self.is_expanded(id)) {
            self.toggle_tool(&id, cx);
        }
        self.list.scroll_to_reveal_item(ix);
        cx.notify();
    }

    /// The bar, over the timeline's top right (T3: 12 px in, popover surface).
    pub(super) fn render_find(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = self.timeline.find.input.clone()?;
        let query = input.read(cx).value().to_string();
        let f = &self.timeline.find;
        let theme = cx.theme();
        let none = !query.trim().is_empty() && f.matches.is_empty();
        let step = |id: &'static str, icon: IconName, forward: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .disabled(f.matches.is_empty())
                .on_click(cx.listener(move |this, _, _, cx| this.find_step(forward, cx)))
        };
        Some(
            div()
                .absolute()
                .top_3()
                .right_3()
                .w(px(360.))
                .child(
                    h_flex()
                        .id("find-bar")
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
                            div()
                                .flex_1()
                                .child(Input::new(&input).appearance(false).small()),
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
                        .child(step("find-prev", IconName::ChevronUp, false))
                        .child(step("find-next", IconName::ChevronDown, true))
                        .child(
                            Button::new("find-close")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_find(&CloseFind, window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
