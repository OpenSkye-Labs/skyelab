//! How a thread's items look, shared by local (`ThreadView`) and remote (`RemoteThreadView`) thread tabs.

use super::thread::ThreadView;
use super::thread_model::{Item, ToolItem};
use super::thread_render::opens_in_browser;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, h_flex, text::TextView, v_flex};

/// Longest tool input or result shown when a row is expanded; the store keeps the whole text.
const TOOL_PREVIEW: usize = 4000;

fn clip(s: &str) -> String {
    match s.char_indices().nth(TOOL_PREVIEW) {
        Some((at, _)) => format!("{}\n…", &s[..at]),
        None => s.to_owned(),
    }
}

/// What the item renderer needs from a thread view, local or remote, so both draw items the same way.
pub(super) trait ItemsView: Sized + 'static {
    fn items(&self) -> &[Item];
    fn is_expanded(&self, tool_id: &str) -> bool;
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>);
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
}

/// One row of a thread's item list.
pub(super) fn render_item<V: ItemsView>(
    this: &mut V,
    ix: usize,
    _: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    let theme = cx.theme();
    let Some(item) = this.items().get(ix) else {
        return div().into_any_element();
    };
    let row = div().w_full().px_4().py_1p5();
    match item {
        Item::Prompt(text) => row
            .child(
                div()
                    .ml_auto()
                    .max_w(px(560.))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(theme.secondary)
                    .text_sm()
                    .child(text.clone()),
            )
            .flex()
            .into_any_element(),
        Item::Text(text) => row
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
        Item::Tool(tool) => row.child(render_tool(this, tool, cx)).into_any_element(),
        Item::Done {
            is_error,
            duration_ms,
            cost_usd,
        } => {
            let verb = if *is_error { "Failed after" } else { "Done in" };
            let secs = *duration_ms as f64 / 1000.;
            row.child(
                div()
                    .text_xs()
                    .text_color(if *is_error {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(format!("{verb} {secs:.1}s · ${cost_usd:.4}")),
            )
            .into_any_element()
        }
    }
}

fn render_tool<V: ItemsView>(this: &V, tool: &ToolItem, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let open = this.is_expanded(&tool.id);
    let summary: String = tool
        .input
        .lines()
        .find(|l| l.contains(':'))
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',')
        .chars()
        .take(80)
        .collect();
    let state = match (&tool.result, tool.is_error) {
        (None, _) => "running",
        (Some(_), true) => "error",
        (Some(_), false) => "",
    };
    let id = tool.id.clone();
    let header = h_flex()
        .id(SharedString::from(format!("tool-{}", tool.id)))
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .hover(|s| s.bg(theme.secondary))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&id, cx)))
        .child(
            div()
                .text_color(theme.muted_foreground)
                .child(if open { "▾" } else { "▸" }),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(tool.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(summary),
        )
        .child(div().text_xs().text_color(theme.warning).child(state));
    let block = |label: &'static str, text: String| {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme.muted)
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(text),
            )
    };
    v_flex()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .child(header)
        .when(open, |this| {
            this.child(
                v_flex()
                    .gap_2()
                    .px_3()
                    .pb_2()
                    .child(block("Input", clip(&tool.input)))
                    .children(tool.result.as_deref().map(|r| block("Result", clip(r)))),
            )
        })
        .into_any_element()
}
