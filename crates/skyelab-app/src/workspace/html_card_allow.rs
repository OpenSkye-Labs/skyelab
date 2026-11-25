//! A visual reply card's blocked hosts: the row that names them and the Allow that asks the user first.

use super::super::thread_extras::prefs;
use super::super::thread_items::ItemsView;
use crate::ui;
use crate::ui::Button;
use gpui::{AnyElement, App, Context, SharedString, Window, prelude::*, px};
use gpui_component::{ActiveTheme as _, WindowExt as _, h_flex};

/// Under a card whose page was refused hosts: which ones, each with Allow. The page named them, so they are only
/// offered: Allow asks first ([`confirm_allow`]), and nothing is added without the user's click there.
pub(super) fn blocked_row<V: ItemsView>(
    tool_id: &str,
    blocked: &[String],
    cx: &Context<V>,
) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .debug_selector(|| "html-card-blocked".into())
        .flex_wrap()
        .gap_2()
        .px_3()
        .py_1p5()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.muted.opacity(0.5))
        .text_size(ui::META)
        .text_color(theme.muted_foreground)
        .child(format!(
            "The page asked to load from {}:",
            blocked.join(", ")
        ))
        .children(blocked.iter().enumerate().map(|(i, host)| {
            let host = host.clone();
            Button::new(SharedString::from(format!("html-allow-{tool_id}-{i}")))
                .outline()
                .xsmall()
                .label(format!("Allow {host}"))
                .on_click(move |_, window, cx| confirm_allow(host.clone(), window, cx))
        }))
        .into_any_element()
}

/// Asks before `host` joins Settings → Chat → Visual replies; only Allow here saves it.
fn confirm_allow(host: String, window: &mut Window, cx: &mut App) {
    crate::ui::open_dialog(window, cx, move |dialog, _, _| {
        let host = host.clone();
        dialog
            .w(px(440.))
            .title(format!("Allow {host}?"))
            .child(format!(
                "Visual replies from every agent may then load scripts, styles and fonts from https://{host}. \
                 A script from there runs in the page. Remove it any time in Settings → Chat."
            ))
            .footer(
                ui::dialog_footer().child(ui::dialog_cancel("html-allow-cancel")).child(
                    Button::new("html-allow-confirm")
                        .primary()
                        .small()
                        .label("Allow")
                        .on_click(move |_, window, cx| {
                            let mut hosts = prefs::page_hosts(cx);
                            if !hosts.contains(&host) {
                                hosts.push(host.clone());
                            }
                            prefs::save_page_hosts(hosts, cx).detach_and_log_err(cx);
                            window.close_dialog(cx);
                        }),
                ),
            )
    });
}
