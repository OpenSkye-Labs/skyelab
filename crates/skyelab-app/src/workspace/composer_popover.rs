//! The composer's floating surfaces: T3's popup panel, a muted hint line, and the `@` / `/` / `$` / `#` menu.

use super::composer_mentions;
use super::composer_menu::MenuKind;
use super::composer_state::Menu;
use crate::ui;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

/// A floating panel above the composer card, T3's popup surface.
pub(in crate::workspace) fn popover(cx: &gpui::App) -> gpui::Div {
    let theme = cx.theme();
    v_flex()
        .absolute()
        .bottom_full()
        .mb_2()
        .p_1()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_md()
}

pub(in crate::workspace) fn hint(text: impl Into<SharedString>, cx: &gpui::App) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_size(ui::META)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}

impl super::ThreadView {
    /// The `@` / `/` / `$` / `#` menu, floating above the card.
    pub(super) fn render_menu(&self, menu: &Menu, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (title, empty) = match menu.kind {
            MenuKind::Files => ("Files", "No file matches"),
            MenuKind::Commands => ("Commands", "No command matches"),
            MenuKind::Skills => ("Skills", "No skill in this account"),
            // Only a thread's own linked pull request is offered, under the thread: the title says so.
            MenuKind::Context => (
                "Threads and their pull requests",
                "No other thread of this account in this project",
            ),
        };
        let rows = menu.items.as_ref().map(|items| {
            menu.rows
                .iter()
                .enumerate()
                .filter_map(|(pos, &i)| Some((pos, items.get(i)?.clone())))
                .collect::<Vec<_>>()
        });
        let models_shown = rows
            .as_ref()
            .is_some_and(|r| r.iter().any(|(_, i)| i.model.is_some()));
        let title = if models_shown {
            "Models and files"
        } else {
            title
        };
        let body: Vec<AnyElement> = match rows {
            None => vec![hint("Loading…", cx)],
            Some(rows) if rows.is_empty() => vec![hint(empty, cx)],
            Some(rows) => {
                rows.into_iter()
                    .map(|(pos, item)| {
                        let selected = pos == menu.selected;
                        let item_label = item.label.clone();
                        let a11y_label = if item.detail.is_empty() {
                            item_label.to_string()
                        } else {
                            format!("{item_label}, {}", item.detail)
                        };
                        h_flex()
                            .id(SharedString::from(format!("menu-row-{pos}")))
                            .role(gpui::accesskit::Role::MenuItem)
                            .aria_label(a11y_label)
                            .aria_selected(selected)
                            .h(px(28.))
                            .px_2()
                            .gap_2()
                            .rounded(ui::RADIUS_SM)
                            .text_size(ui::BODY)
                            .when(selected, |this| this.bg(theme.foreground.opacity(0.09)))
                            .hover(|this| this.bg(theme.accent))
                            .when(item.model.is_some(), |this| {
                                this.child(ui::provider_logo(
                                    composer_mentions::provider_of(&item.insert),
                                    px(12.),
                                    cx,
                                ))
                            })
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(px(420.))
                                    .truncate()
                                    .child(item_label),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground)
                                    .child(item.detail),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.accept_row(pos, window, cx)
                            }))
                            .into_any_element()
                    })
                    .collect()
            }
        };
        popover(cx)
            .id("composer-menu")
            .role(gpui::accesskit::Role::Menu)
            .aria_label(title)
            .max_h(px(320.))
            .overflow_y_scroll()
            .left_0()
            .right_0()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(ui::META)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .child(title),
            )
            .children(body)
            .into_any_element()
    }
}
