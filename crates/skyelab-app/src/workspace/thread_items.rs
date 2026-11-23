//! How a thread's items look, shared by local (`ThreadView`) and remote (`RemoteThreadView`) thread tabs.

use super::thread::ThreadView;
use super::thread_model::{ApprovalItem, Item, ToolItem};
use super::thread_render::opens_in_browser;
use crate::ui;
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
    relative,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    text::TextView,
    v_flex,
};
use skyelab_agent::Decision;

/// Longest tool input or result shown when a row is expanded; the store keeps the whole text.
const TOOL_PREVIEW: usize = 4000;

fn clip(s: &str) -> String {
    match s.char_indices().nth(TOOL_PREVIEW) {
        Some((at, _)) => format!("{}\n…", &s[..at]),
        None => s.to_owned(),
    }
}

#[cfg(test)]
thread_local! {
    static ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many rows `render_item` has built on this thread, so tests can check a frame builds only visible rows.
#[cfg(test)]
pub(super) fn rows_built() -> usize {
    ROWS_BUILT.with(|n| n.get())
}

/// What the item renderer needs from a thread view, local or remote, so both draw items the same way.
pub(super) trait ItemsView: Sized + 'static {
    fn items(&self) -> &[Item];
    fn is_expanded(&self, tool_id: &str) -> bool;
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>);
    /// Whether an agent is there to answer a pending permission request.
    fn can_answer(&self, cx: &App) -> bool;
    fn answer(&mut self, id: &str, decision: Decision, window: &mut Window, cx: &mut Context<Self>);
}

impl ItemsView for ThreadView {
    fn items(&self) -> &[Item] {
        &self.items
    }
    fn is_expanded(&self, tool_id: &str) -> bool {
        ThreadView::is_expanded(self, tool_id)
    }
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        ThreadView::toggle_tool(self, tool_id, cx)
    }
    fn can_answer(&self, _: &App) -> bool {
        self.session.is_some()
    }
    fn answer(&mut self, id: &str, decision: Decision, _: &mut Window, cx: &mut Context<Self>) {
        ThreadView::answer(self, id, decision, cx)
    }
}

/// One row of a thread's item list.
pub(super) fn render_item<V: ItemsView>(
    this: &mut V,
    ix: usize,
    _: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    #[cfg(test)]
    ROWS_BUILT.with(|n| n.set(n.get() + 1));
    let theme = cx.theme();
    let Some(item) = this.items().get(ix) else {
        return div().into_any_element();
    };
    // T3's chat column: 736 wide, centered; rows 16 apart.
    let column = |child: AnyElement| {
        h_flex()
            .w_full()
            .justify_center()
            .px_6()
            .child(div().w_full().max_w(ui::CHAT_MAX_W).py_2().child(child))
            .into_any_element()
    };
    // Tool rows sit close together, like T3's work log.
    let tight = |child: AnyElement| {
        h_flex()
            .w_full()
            .justify_center()
            .px_6()
            .child(div().w_full().max_w(ui::CHAT_MAX_W).py_0p5().child(child))
            .into_any_element()
    };
    match item {
        Item::Prompt(text) => column(
            h_flex()
                .w_full()
                .justify_end()
                .child(
                    div()
                        .max_w(relative(0.8))
                        .p_3()
                        .rounded(ui::RADIUS_XL)
                        .bg(theme.secondary)
                        .text_size(ui::BODY)
                        .child(text.clone()),
                )
                .into_any_element(),
        ),
        Item::Text(text) => column(
            div()
                .px_1()
                .text_size(ui::BODY)
                .child(
                    TextView::markdown(("md", ix), text.clone())
                        .selectable(true)
                        .on_link_click(|url, _, _, cx| {
                            if opens_in_browser(url) {
                                cx.open_url(url);
                            }
                        }),
                )
                .into_any_element(),
        ),
        Item::Tool(tool) => tight(render_tool(this, tool, cx)),
        Item::Approval(a) => column(render_approval(this, a, cx)),
        Item::Output { id, text } => tight(render_output(this, id, text, cx)),
        Item::Done {
            is_error,
            duration_ms,
            cost_usd,
        } => {
            let verb = if *is_error {
                "Failed after"
            } else {
                "Worked for"
            };
            let secs = *duration_ms as f64 / 1000.;
            let color = if *is_error {
                ui::ink(theme, theme.danger)
            } else {
                theme.muted_foreground
            };
            let rule = || div().flex_1().h_px().bg(theme.border);
            column(
                h_flex()
                    .gap_3()
                    .py_1()
                    .text_size(ui::META)
                    .text_color(color)
                    .child(rule())
                    .child(format!("{verb} {secs:.1}s · ${cost_usd:.4}"))
                    .child(rule())
                    .into_any_element(),
            )
        }
    }
}

fn render_tool<V: ItemsView>(this: &V, tool: &ToolItem, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let open = this.is_expanded(&tool.id);
    // The first argument's value, as T3 shows it (`ls crates`, not `"command": "ls crates"`).
    let summary: String = tool
        .input
        .lines()
        .find_map(|l| l.split_once(':').map(|(_, v)| v))
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',')
        .trim_matches('"')
        .chars()
        .take(80)
        .collect();
    let state = match (&tool.result, tool.is_error) {
        (None, _) => "running",
        (Some(_), true) => "error",
        (Some(_), false) => "",
    };
    let id = tool.id.clone();
    let (icon, icon_color) = match (&tool.result, tool.is_error) {
        (None, _) => (IconName::LoaderCircle, theme.info),
        (Some(_), true) => (IconName::CircleAlert, theme.danger),
        (Some(_), false) => (IconName::Check, theme.muted_foreground),
    };
    // T3's work-log row: 24 px icon slot, the tool's name, its argument in mono, a chevron that turns when open.
    let header = h_flex()
        .id(SharedString::from(format!("tool-{}", tool.id)))
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .text_size(ui::BODY)
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&id, cx)))
        .child(
            div()
                .w(px(24.))
                .flex()
                .justify_center()
                .flex_shrink_0()
                .child(Icon::new(icon).size_4().text_color(icon_color)),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child(tool.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(summary),
        )
        .when(!state.is_empty(), |this| {
            this.child(
                div()
                    .text_size(ui::META)
                    .text_color(icon_color)
                    .child(state),
            )
        })
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size_3()
            .text_color(theme.muted_foreground.opacity(0.7)),
        );
    let block = |label: &'static str, text: String| {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(ui::RADIUS)
                    .bg(theme.muted)
                    .border_1()
                    .border_color(theme.border)
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(text),
            )
    };
    v_flex()
        .w_full()
        .child(header)
        .when(open, |this| {
            this.child(
                v_flex()
                    .gap_2()
                    .ml_7()
                    .pt_1()
                    .pb_2()
                    .child(block("Input", clip(&tool.input)))
                    .children(tool.result.as_deref().map(|r| block("Result", clip(r)))),
            )
        })
        .into_any_element()
}

/// The label a decided request keeps.
fn decided(d: Decision) -> &'static str {
    match d {
        Decision::AllowOnce => "Allowed once",
        Decision::AllowThread => "Allowed for this thread",
        Decision::Deny => "Denied",
        Decision::Withdrawn => "Withdrawn",
    }
}

/// A permission request, T3's pending-approval row: what the tool wants to do in mono, then the answers. Once
/// decided it shrinks to one line saying what was decided.
fn render_approval<V: ItemsView>(this: &V, a: &ApprovalItem, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let summary = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .text_color(theme.muted_foreground)
        .child(a.summary.clone());
    if let Some(d) = a.decision {
        let (icon, color) = match d {
            Decision::AllowOnce | Decision::AllowThread => {
                (IconName::Check, theme.muted_foreground)
            }
            Decision::Deny => (IconName::CircleAlert, theme.danger),
            Decision::Withdrawn => (IconName::CircleAlert, theme.muted_foreground),
        };
        return h_flex()
            .min_h(px(28.))
            .gap_1p5()
            .px_1()
            .text_size(ui::BODY)
            .child(
                div()
                    .w(px(24.))
                    .flex()
                    .justify_center()
                    .flex_shrink_0()
                    .child(Icon::new(icon).size_4().text_color(color)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(a.tool.clone()),
            )
            .child(summary)
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(color)
                    .child(decided(d)),
            )
            .into_any_element();
    }
    let answerable = this.can_answer(cx);
    let theme = cx.theme();
    let button = |label: &'static str, key: &str, decision: Decision| {
        let id = a.id.clone();
        Button::new(SharedString::from(format!("{key}-{}", a.id)))
            .xsmall()
            .label(label)
            .on_click(
                cx.listener(move |this, _, window, cx| this.answer(&id, decision, window, cx)),
            )
    };
    let actions = if answerable {
        h_flex()
            .gap_2()
            .child(button("Allow once", "allow-once", Decision::AllowOnce).primary())
            .child(
                button(
                    "Allow for this thread",
                    "allow-thread",
                    Decision::AllowThread,
                )
                .outline(),
            )
            .child(button("Deny", "deny", Decision::Deny).ghost())
            .into_any_element()
    } else {
        div()
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .child(
                "The agent isn't running, so this can't be answered. Send a message to continue.",
            )
            .into_any_element()
    };
    let theme = cx.theme();
    v_flex()
        .debug_selector(|| "pending-approval".into())
        .w_full()
        .gap_2()
        .px_3()
        .py_2p5()
        .rounded(ui::RADIUS_LG)
        .bg(theme.warning.opacity(0.08))
        .border_1()
        .border_color(theme.warning.opacity(0.35))
        .child(
            h_flex()
                .gap_1p5()
                .text_size(ui::BODY)
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .text_color(ui::ink(theme, theme.warning)),
                )
                .child(div().font_weight(FontWeight::MEDIUM).child(a.tool.clone()))
                .child(
                    div()
                        .text_size(ui::META)
                        .text_color(ui::ink(theme, theme.warning))
                        .child("Waiting for approval"),
                ),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .rounded(ui::RADIUS)
                .bg(theme.muted)
                .border_1()
                .border_color(theme.border)
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .child(clip(&a.input)),
        )
        .child(actions)
        .into_any_element()
}

/// The agent's stderr, collapsed to one line until opened.
fn render_output<V: ItemsView>(this: &V, id: &str, text: &str, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let open = this.is_expanded(id);
    let first = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .to_owned();
    let key = id.to_owned();
    let header = h_flex()
        .id(SharedString::from(id.to_owned()))
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .text_size(ui::BODY)
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx)))
        .child(
            div()
                .w(px(24.))
                .flex()
                .justify_center()
                .flex_shrink_0()
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size_4()
                        .text_color(theme.danger),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child("Agent output"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(first),
        )
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size_3()
            .text_color(theme.muted_foreground.opacity(0.7)),
        );
    v_flex()
        .w_full()
        .child(header)
        .when(open, |this| {
            this.child(
                div()
                    .ml_7()
                    .mt_1()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .rounded(ui::RADIUS)
                    .bg(theme.muted)
                    .border_1()
                    .border_color(theme.border)
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(clip(text)),
            )
        })
        .into_any_element()
}
