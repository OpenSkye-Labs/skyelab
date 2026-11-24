//! How the prompt stash looks (T3's `ComposerStashBadge` and `ComposerStashMenu`); behavior is `composer_stash.rs`.

use super::composer_stash::{PromptStash, SHORTCUT};
use super::popover;
use super::thread::ThreadView;
use crate::ui;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, FontWeight, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};

impl ThreadView {
    /// The tab on the composer's top-right shoulder: "Stash" and the count. Hidden while empty or open.
    pub(in crate::workspace) fn render_stash_badge(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let count = PromptStash::rows(cx).len();
        if count == 0 || self.composer.stash_open.is_some() {
            return None;
        }
        let theme = cx.theme();
        let tab = h_flex()
            .id("stash-badge")
            .debug_selector(|| "stash-badge".into())
            .h(px(24.))
            .px_2p5()
            .gap_1p5()
            .rounded_t(ui::RADIUS_LG)
            .border_1()
            .border_b_0()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|this| this.text_color(cx.theme().foreground))
            .child(Icon::new(SkyeIcon::Bookmark).xsmall())
            .child("Stash")
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(count.to_string()),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.composer.stash_open = Some(0);
                cx.notify();
            }));
        // Overlaps the card's top border by a pixel, so the tab reads as part of it.
        Some(
            h_flex()
                .w_full()
                .justify_end()
                .pr_4()
                .mb(px(-1.))
                .child(tab)
                .into_any_element(),
        )
    }

    /// The open list, floating above the card like the `@` menu.
    pub(in crate::workspace) fn render_stash_menu(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let selected = self.composer.stash_open?;
        let rows = PromptStash::rows(cx).to_vec();
        let theme = cx.theme();
        let now = ui::now_ms();
        let header = h_flex()
            .id("stash-header")
            .h(px(28.))
            .px_2()
            .gap_2()
            .rounded(ui::RADIUS_SM)
            .text_size(ui::META)
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().accent))
            .child(
                Icon::new(SkyeIcon::Bookmark)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .text_color(theme.muted_foreground)
                    .child("Stash"),
            )
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(rows.len().to_string()),
            )
            .child(
                Icon::new(SkyeIcon::ChevronDown)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.composer.stash_open = None;
                cx.notify();
            }));
        let body: Vec<AnyElement> = if rows.is_empty() {
            vec![super::hint(
                format!(
                    "Nothing stashed yet. Press {SHORTCUT} with a prompt in the composer to stash it."
                ),
                cx,
            )]
        } else {
            rows.into_iter()
                .enumerate()
                .map(|(ix, row)| {
                    let (restore, delete) = (row.owner.clone(), row.owner.clone());
                    h_flex()
                        .id(("stash-row", ix))
                        .debug_selector(move || format!("stash-row-{ix}"))
                        .h(px(28.))
                        .px_2()
                        .gap_2()
                        .rounded(ui::RADIUS_SM)
                        .text_size(ui::BODY)
                        .cursor_pointer()
                        .when(ix == selected, |this| {
                            this.bg(theme.foreground.opacity(0.09))
                        })
                        .hover(|this| this.bg(cx.theme().accent))
                        .child(
                            Icon::new(SkyeIcon::FileText)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.foreground.opacity(0.8))
                                .child(row.snippet),
                        )
                        .when(row.files > 0, |this| {
                            this.child(
                                h_flex()
                                    .gap_1()
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground)
                                    .child(Icon::new(SkyeIcon::File).xsmall())
                                    .child(row.files.to_string()),
                            )
                        })
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(ui::ago(now, row.at)),
                        )
                        .child(
                            Button::new(("stash-delete", ix))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(SkyeIcon::X))
                                .tooltip("Delete stashed prompt")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.delete_stash(delete.clone(), cx);
                                })),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.restore_stash(restore.clone(), window, cx)
                        }))
                        .into_any_element()
                })
                .collect()
        };
        Some(
            popover(cx)
                .id("stash-menu")
                .max_h(px(320.))
                .overflow_y_scroll()
                .left_0()
                .right_0()
                .child(header)
                .children(body)
                .into_any_element(),
        )
    }
}
