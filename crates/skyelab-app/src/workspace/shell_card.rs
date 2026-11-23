//! Shell commands, T3's command block: the command, how it ended, and its output with escape sequences dropped.

use super::thread_items::{ItemsView, clip};
use super::thread_model::ToolItem;
use super::tool_cards::chevron;
use crate::ui;
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
pub(super) use skyelab_agent::timeline::cards::{exit_status, plain_output};

/// A shell command, T3's command block: `$ command` in mono with how it ended; opened, its output in a scroll box.
pub(super) fn render_bash<V: ItemsView>(
    this: &V,
    tool: &ToolItem,
    cx: &mut Context<V>,
) -> AnyElement {
    let open = this.is_expanded(&tool.id);
    let theme = cx.theme();
    let command = tool.raw["command"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| tool.raw["command"].to_string());
    let status = exit_status(tool);
    let (icon, color) = match (&tool.result, &status) {
        (None, _) => (IconName::LoaderCircle, theme.info),
        (Some(_), Some(_)) => (IconName::CircleAlert, theme.danger),
        (Some(_), None) => (IconName::SquareTerminal, theme.muted_foreground),
    };
    let key = tool.id.clone();
    let line = h_flex()
        .id(SharedString::from(format!("bash-{}", tool.id)))
        .gap_2()
        .px_3()
        .py_2()
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx)))
        .child(Icon::new(icon).size_3p5().text_color(color))
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme.muted_foreground)
                .child("$"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.foreground.opacity(0.85))
                .child(command),
        )
        .children(status.map(|s| {
            div()
                .flex_shrink_0()
                .text_color(ui::ink(theme, theme.danger))
                .child(s)
        }))
        .when(tool.result.is_none(), |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_color(theme.info)
                    .child("running"),
            )
        })
        .child(chevron(open, cx));
    let theme = cx.theme();
    let output = tool.result.as_deref().filter(|_| open).map(|r| {
        let text = plain_output(r);
        div()
            .id(SharedString::from(format!("bash-output-{}", tool.id)))
            .debug_selector(|| "bash-output".into())
            .max_h(px(256.))
            .overflow_y_scroll()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .text_color(theme.muted_foreground)
            .child(if text.trim().is_empty() {
                "(no output)".to_owned()
            } else {
                clip(text.trim_end())
            })
    });
    v_flex()
        .debug_selector(|| "bash-card".into())
        .w_full()
        .my_1()
        .rounded(ui::RADIUS)
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted.opacity(0.5))
        .overflow_hidden()
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .child(line)
        .children(output)
        .into_any_element()
}
