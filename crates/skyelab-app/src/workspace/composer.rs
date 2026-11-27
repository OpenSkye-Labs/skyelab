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
#[path = "composer_keys.rs"]
mod composer_keys;
#[path = "composer_machine.rs"]
mod composer_machine;
#[path = "composer_mentions.rs"]
pub(in crate::workspace) mod composer_mentions;
#[cfg(test)]
#[path = "composer_mentions_test.rs"]
mod composer_mentions_test;
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
#[path = "composer_picker_keys.rs"]
mod composer_picker_keys;
#[path = "composer_popover.rs"]
mod composer_popover;
#[cfg(all(test, unix))] // the fake agent is a shell script
#[path = "composer_press_test.rs"]
mod composer_press_test;
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
#[path = "composer_voice.rs"]
pub(in crate::workspace) mod composer_voice;
#[cfg(all(test, target_os = "macos"))] // dictation is macOS only
#[path = "composer_voice_test.rs"]
mod composer_voice_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "context_chips_test.rs"]
mod context_chips_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "live_rebind_test.rs"]
mod live_rebind_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "model_picker_keys_test.rs"]
mod model_picker_keys_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "prompt_stash_test.rs"]
mod prompt_stash_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "quote_selection_test.rs"]
mod quote_selection_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
#[path = "send_shortcut_test.rs"]
mod send_shortcut_test;
#[cfg(test)]
#[path = "side_commands_test.rs"]
mod side_commands_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "stash_account_test.rs"]
mod stash_account_test;
pub(in crate::workspace) use composer_footer::machine_chip;
pub(in crate::workspace) use composer_machine::machine_name;

use crate::ui::SkyeIcon;
#[cfg(test)]
use crate::workspace::*;
use crate::workspace::{lock, thread, thread_worktree};
pub(in crate::workspace) use composer_keys::bind_keys;
pub(in crate::workspace) use composer_popover::{hint, popover};
pub(in crate::workspace) use composer_state::{Composer, SteerPrompt, Submit, shown_prompt};

use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use composer_keys::MENU_CONTEXT;
use composer_state::AttachFiles;
use gpui::{AnyElement, Context, ExternalPaths, IntoElement, div, prelude::*, px};
use gpui_component::{Icon, h_flex, input::Textarea, v_flex};
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
        let picker = self
            .composer
            .picker
            .is_some()
            .then(|| self.render_picker(window, cx));
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
            // Read-only while dictating. The element sets this on the state every render, so set_readonly wouldn't hold.
            Textarea::new(&self.prompt)
                .appearance(false)
                .aria_label("Message prompt")
                .accessibility_id("composer-prompt")
                .readonly(self.composer.dictation.active())
                .token(|token, _, cx| composer_mentions::chip(token, cx))
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
            .role(gpui::accesskit::Role::Region)
            .aria_label("Composer")
            .accessibility_id("composer")
            // A submitting Enter propagates out of the textarea; stopping it here keeps it from being typed as a new line.
            .on_action(|_: &gpui_component::input::Enter, _, _| {})
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
                    .when(self.composer.dictation.active(), |this| {
                        this.key_context(composer_voice::VOICE_CONTEXT)
                    })
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
            .child(self.render_mode_picker(
                crate::workspace::thread_render::mode_picker::icon_only(width),
                cx,
            ))
            .child(div().flex_1())
            .child(
                Button::new("model-picker")
                    .ghost()
                    .small()
                    .accessibility_label(format!("Model: {chip}"))
                    .accessibility_expanded(self.composer.picker.is_some())
                    // The provider's logo is an image in its own colors, so it goes in as a child, not a tinted icon.
                    .map(|b| match self.composer_account() {
                        Some(a) => b.child(ui::provider_logo(a.provider, px(14.), cx)).child(
                            div()
                                .min_w_0()
                                .text_ellipsis()
                                .overflow_hidden()
                                .child(chip),
                        ),
                        None => b.icon(SkyeIcon::Model).label(chip),
                    })
                    .dropdown_caret(true)
                    .tooltip(if account_default {
                        "Account default model"
                    } else {
                        "Model and effort"
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_picker(window, cx))),
            )
            .children(self.render_voice(cx))
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
        let round = px(16.);
        let mut out = Vec::new();
        // A docked question: Next or Submit, with Interrupt beside it while the turn waits (T3's).
        let next = self.render_dock_send(cx);
        let docked = next.is_some();
        let empty = empty || docked;
        out.extend(next);
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
                .role(gpui::accesskit::Role::Button)
                .aria_label("Interrupt")
                .accessibility_id("interrupt")
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
                    crate::ui::Tooltip::new("Interrupt")
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
        } else if docked {
            return out;
        } else {
            Button::new("send")
                .primary()
                // 32 px, the same as the Stop circle that takes its place.
                .medium()
                .icon(SkyeIcon::ArrowUp)
                .rounded(round)
                .tooltip({
                    let key = composer_keys::send_key(cx);
                    if running {
                        format!(
                            "Queue for when the agent finishes ({key}). {} sends it now.",
                            ui::keys("⌘⇧↩")
                        )
                    } else {
                        format!("Send ({key})")
                    }
                })
                .disabled(self.read_only || self.starting || empty)
                .on_click(
                    cx.listener(|this, _, window, cx| this.send_prompt(&SendPrompt, window, cx)),
                )
                .into_any_element()
        };
        out.push(button);
        out
    }
}
