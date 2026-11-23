//! How a thread's items look, shared by local (`ThreadView`) and remote (`RemoteThreadView`) thread tabs.

use super::thread::ThreadView;
use super::thread_model::{Item, ToolItem};
use super::thread_render::opens_in_browser;
use crate::ui;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
    relative,
};
use gpui_component::{ActiveTheme as _, Icon, IconName, h_flex, text::TextView, v_flex};

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
        Item::Tool(tool) => column(render_tool(this, tool, cx)),
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
                theme.danger
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
                .w(px(20.))
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
