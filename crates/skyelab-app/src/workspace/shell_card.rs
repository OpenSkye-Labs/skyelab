//! Shell commands, T3's command block: the command, how it ended, and its output with escape sequences dropped.

use super::thread_items::{ItemsView, clip};
use super::thread_model::ToolItem;
use super::tool_cards::chevron;
use crate::ui;
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};

/// How a finished command ended: `exit N` when the output says (unconfirmed: Claude's failed Bash results start
/// "Exit code N"), `failed` for another error, `None` for success or while it runs.
pub(super) fn exit_status(tool: &ToolItem) -> Option<String> {
    let result = tool.result.as_deref()?;
    let code = result
        .strip_prefix("Exit code ")
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .filter(|c| !c.is_empty());
    match (code, tool.is_error) {
        (Some(code), _) => Some(format!("exit {code}")),
        (None, true) => Some("failed".into()),
        (None, false) => None,
    }
}

/// Command output as a terminal would leave it: escape sequences dropped, a carriage return overwriting its line.
pub(super) fn plain_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut line = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // CSI: ESC [ params… final byte (@ to ~). OSC (titles, links): ESC ] … BEL or ESC \\. Any other
                // escape: drop the next char.
                if chars.next_if_eq(&'[').is_some() {
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                } else if chars.next_if_eq(&']').is_some() {
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' || (n == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                } else {
                    chars.next();
                }
            }
            '\r' if chars.peek() != Some(&'\n') => line.clear(),
            '\r' => {}
            '\n' => {
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            c => line.push(c),
        }
    }
    out.push_str(&line);
    out
}

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
