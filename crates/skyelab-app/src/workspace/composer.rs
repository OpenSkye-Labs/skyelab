//! How the composer looks, after T3's (docs/design/t3-reference-chat.md §4): the queue above a rounded card holding the
//! attachments, the prompt and a control row (attach and the permission chip; the model chip and Send), and a footer
//! strip under it (machine, account, checkout, changes, branch). Behavior lives in `composer_state.rs`.

// The composer's parts. Their `super::` paths name the workspace's modules through the imports below.
#[path = "composer_attach.rs"]
mod composer_attach;
#[path = "composer_context.rs"]
pub(crate) mod composer_context;
#[path = "composer_context_pick.rs"]
mod composer_context_pick;
#[path = "composer_draft.rs"]
mod composer_draft;
#[path = "composer_footer.rs"]
mod composer_footer;
#[path = "composer_machine.rs"]
mod composer_machine;
#[path = "composer_menu.rs"]
mod composer_menu;
#[cfg(test)]
#[path = "composer_menu_test.rs"]
mod composer_menu_test;
#[path = "composer_model.rs"]
mod composer_model;
#[cfg(all(test, unix))]
#[path = "composer_paste_test.rs"]
mod composer_paste_test;
#[path = "composer_picker.rs"]
mod composer_picker;
#[path = "composer_queue.rs"]
mod composer_queue;
#[cfg(test)]
#[path = "composer_queue_test.rs"]
mod composer_queue_test;
#[path = "composer_quote.rs"]
mod composer_quote;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "composer_restore_test.rs"]
mod composer_restore_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "composer_review_test.rs"]
mod composer_review_test;
#[path = "composer_send.rs"]
mod composer_send;
#[path = "composer_stash.rs"]
pub(in crate::workspace) mod composer_stash;
#[path = "composer_stash_view.rs"]
mod composer_stash_view;
#[path = "composer_state.rs"]
mod composer_state;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "context_chips_test.rs"]
mod context_chips_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "prompt_stash_test.rs"]
mod prompt_stash_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "quote_selection_test.rs"]
mod quote_selection_test;
#[cfg(test)]
#[path = "side_commands_test.rs"]
mod side_commands_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "stash_account_test.rs"]
mod stash_account_test;
pub(in crate::workspace) use composer_machine::machine_name;

use crate::ui::SkyeIcon;
#[cfg(test)]
use crate::workspace::*;
use crate::workspace::{lock, thread, thread_worktree};
pub(in crate::workspace) use composer_state::{
    Composer, SteerPrompt, Submit, bind_keys, shown_prompt,
};

use crate::ui;
use composer_menu::MenuKind;
use composer_state::{AttachFiles, MENU_CONTEXT, Menu};
use gpui::{
    AnyElement, Context, ExternalPaths, FontWeight, IntoElement, SharedString, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonRounded, ButtonVariants as _},
    h_flex,
    input::Textarea,
    v_flex,
};
use skyelab_agent::AttachmentKind;
use thread::{SendPrompt, ThreadView};

/// Under this card width the control row goes compact (T3 collapses its chips the same way).
const COMPACT_BELOW: gpui::Pixels = px(560.);

/// Whether a card this wide (0: not measured yet) gets the compact control row.
fn compact(width: gpui::Pixels) -> bool {
    width > px(0.) && width < COMPACT_BELOW
}

impl ThreadView {
    pub(in crate::workspace) fn render_composer(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let menu = self.composer.menu.as_ref();
        let menu_el = menu.map(|m| self.render_menu(m, cx));
        let stash_menu = self.render_stash_menu(cx);
        let stash_badge = self.render_stash_badge(cx);
        let picker = self.composer.picker_open.then(|| self.render_picker(cx));
        let attachments = self.render_attachments(cx);
        let contexts = self.render_contexts(cx);
        let controls = self.render_controls(cx);
        let queue = self.render_queue(cx);
        let footer = self.render_footer(cx);
        let view = cx.entity().downgrade();
        let (width, view_id) = (self.composer.width.clone(), cx.entity_id());
        let owned = self.owned_by(window);
        // The card's width, read by the next frame's control row: crossing the compact line redraws once. Only the
        // owner window measures, or two windows of different widths would redraw each other forever.
        let measure = gpui::canvas(
            move |bounds, _, cx| {
                if !owned {
                    return;
                }
                let old = width.replace(bounds.size.width);
                if compact(old) != compact(bounds.size.width) {
                    cx.notify(view_id);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let input = if owned {
            Textarea::new(&self.prompt)
                .appearance(false)
                .on_paste(move |item, _, cx| {
                    view.update(cx, |this, cx| this.attach_clipboard(item, cx))
                        .unwrap_or(false)
                })
                .into_any_element()
        } else {
            self.render_borrowed_prompt(cx)
        };
        let ringed = crate::workspace::focus_areas::composer_ringed(
            &gpui::Focusable::focus_handle(self.prompt.read(cx), cx),
            window,
            cx,
        );
        let theme = cx.theme();
        let card = v_flex()
            .id("composer-card")
            .relative()
            .w_full()
            .rounded(ui::RADIUS_XL)
            .bg(theme.popover)
            .border_1()
            .border_color(if ringed { theme.ring } else { theme.border })
            .when(!theme.is_dark(), |this| this.shadow_xs())
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.border_color(cx.theme().ring))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.attach_paths(paths.paths().to_vec(), cx)
            }))
            .child(measure)
            .children(menu_el)
            .children(stash_menu)
            .children(picker)
            .children(attachments)
            .children(contexts)
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .text_size(ui::BODY)
                    .when(
                        menu.is_some() || self.composer.stash_open.is_some(),
                        |this| this.key_context(MENU_CONTEXT),
                    )
                    .child(input),
            )
            .child(controls);
        v_flex()
            .w_full()
            .max_w(ui::CHAT_MAX_W)
            .children(queue)
            .children(stash_badge)
            .child(card)
            .children(self.composer.notice.clone().map(|n| {
                div()
                    .px_2()
                    .pt_1()
                    .text_size(ui::META)
                    .text_color(ui::ink(theme, theme.danger))
                    .child(n)
            }))
            .children(self.composer.pasted.as_ref().map(|(name, _)| {
                h_flex()
                    .px_2()
                    .pt_1()
                    .gap_1()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(format!("Long paste attached as {name}."))
                    .child(
                        Button::new("paste-as-text")
                            .ghost()
                            .xsmall()
                            .label("Paste as text")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.paste_as_text(window, cx)),
                            ),
                    )
            }))
            .child(footer)
            .into_any_element()
    }

    /// The row inside the card under the prompt, T3's: attach and the permission chip on the left; the model chip
    /// and Send on the right. A narrow card (a right panel open) keeps only the chips' icons and the model's name.
    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let width = self.composer.width.get();
        let compact = compact(width);
        let (chip, account_default) = self.chip_label(compact);
        h_flex()
            .px_2()
            .pb_2()
            .gap_0p5()
            .child(
                Button::new("attach")
                    .ghost()
                    .small()
                    .icon(Icon::new(SkyeIcon::Paperclip))
                    .tooltip("Attach images or files")
                    .disabled(self.read_only)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.attach_files(&AttachFiles, window, cx)
                    })),
            )
            .child(self.render_mode_picker(compact, cx))
            .child(div().flex_1())
            .child(
                Button::new("model-picker")
                    .ghost()
                    .small()
                    .icon(SkyeIcon::Model)
                    .label(chip)
                    .dropdown_caret(true)
                    .tooltip(if account_default {
                        "Account default model"
                    } else {
                        "Model and effort"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_picker(cx))),
            )
            .child(h_flex().pl_1().gap_1().children(self.render_primary(cx)))
            .into_any_element()
    }

    /// The round button on the right, after T3's: Send; while a turn runs with nothing typed, Interrupt ("Stopping…"
    /// once pressed); and Force stop beside it when an interrupt went unanswered.
    fn render_primary(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let running = self.turn_running();
        let empty = self.prompt.read(cx).value().trim().is_empty()
            && self.composer.attachments.is_empty()
            && self.composer.contexts.is_empty();
        let round = ButtonRounded::Size(px(16.));
        let mut out = Vec::new();
        if running && self.composer.stalled {
            out.push(
                Button::new("force-stop")
                    .ghost()
                    .small()
                    .label("Force stop")
                    .tooltip_with_action(
                        "The agent isn't answering Interrupt: end it",
                        &thread::Stop,
                        Some(thread::CONTEXT),
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.stop(&thread::Stop, window, cx)),
                    )
                    .into_any_element(),
            );
        }
        let button: AnyElement = if running && empty && self.composer.interrupting.is_some() {
            Button::new("interrupt")
                .ghost()
                .small()
                .label("Stopping…")
                .disabled(true)
                .into_any_element()
        } else if running && empty {
            // T3's Stop: a solid destructive circle with a small rounded square. Drawn here because a gpui-component
            // button only ever fills with a tint.
            let danger = cx.theme().danger;
            div()
                .id("interrupt")
                .size(px(32.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(danger.opacity(0.9))
                .hover(move |this| this.bg(danger))
                .cursor_pointer()
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new("Interrupt")
                        .action(&thread::Interrupt, Some(thread::CONTEXT))
                        .build(window, cx)
                })
                .child(
                    div()
                        .size(px(12.))
                        .rounded(px(3.))
                        .bg(cx.theme().danger_foreground),
                )
                .on_click(
                    cx.listener(|this, _, window, cx| {
                        this.interrupt(&thread::Interrupt, window, cx)
                    }),
                )
                .into_any_element()
        } else {
            Button::new("send")
                .primary()
                .icon(SkyeIcon::Send)
                .rounded(round)
                .tooltip(if running {
                    "Queue for when the agent finishes (⌘↩). ⌘⇧↩ sends it now."
                } else {
                    "Send (⌘↩)"
                })
                .disabled(self.read_only || self.starting)
                .on_click(
                    cx.listener(|this, _, window, cx| this.send_prompt(&SendPrompt, window, cx)),
                )
                .into_any_element()
        };
        out.push(button);
        out
    }

    /// Removable chips for the attached files, above the prompt.
    fn render_attachments(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.composer.attachments.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_wrap()
                .gap_1p5()
                .px_3()
                .pt_3()
                .children(self.composer.attachments.iter().enumerate().map(|(ix, a)| {
                    let icon = match a.kind {
                        AttachmentKind::Image(_) => SkyeIcon::Image,
                        AttachmentKind::Pdf | AttachmentKind::Text => SkyeIcon::File,
                    };
                    h_flex()
                        .id(SharedString::from(format!("attachment-{ix}")))
                        .h(px(28.))
                        .max_w(px(220.))
                        .pl_2()
                        .gap_1()
                        .rounded(ui::RADIUS)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.muted)
                        .text_size(ui::META)
                        .child(Icon::new(icon).xsmall().text_color(theme.muted_foreground))
                        .child(div().min_w_0().truncate().child(a.name.clone()))
                        .child(
                            Button::new(SharedString::from(format!("remove-attachment-{ix}")))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(SkyeIcon::X))
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.remove_attachment(ix, cx);
                                    this.restore_draft(window, cx);
                                })),
                        )
                }))
                .into_any_element(),
        )
    }

    /// The `@` / `/` / `$` / `#` menu, floating above the card.
    fn render_menu(&self, menu: &Menu, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (title, empty) = match menu.kind {
            MenuKind::Files => ("Files", "No tracked file matches"),
            MenuKind::Commands => ("Commands", "No command matches"),
            MenuKind::Skills => ("Skills", "No skill in this account"),
            MenuKind::Context => (
                "Threads and pull requests",
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
                        h_flex()
                            .id(SharedString::from(format!("menu-row-{pos}")))
                            .h(px(28.))
                            .px_2()
                            .gap_2()
                            .rounded(ui::RADIUS_SM)
                            .text_size(ui::BODY)
                            .when(selected, |this| this.bg(theme.foreground.opacity(0.09)))
                            .hover(|this| this.bg(theme.accent))
                            .when(item.model.is_some(), |this| {
                                this.child(
                                    Icon::new(SkyeIcon::Bot)
                                        .xsmall()
                                        .text_color(theme.muted_foreground),
                                )
                            })
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(px(420.))
                                    .truncate()
                                    .child(item.label),
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
