//! How the composer looks, after T3's (docs/design/t3-reference-chat.md §4): the queue above a rounded card holding the
//! attachments, the prompt and a control row (attach and the permission chip; the model chip and Send), and a footer
//! strip under it (machine, account, checkout, changes, branch). Behavior lives in `composer_state.rs`.

// The composer's parts. Their `super::` paths name the workspace's modules through the imports below.
#[path = "composer_attach.rs"]
mod composer_attach;
#[path = "composer_context.rs"]
pub(crate) mod composer_context;
#[path = "composer_draft.rs"]
mod composer_draft;
#[path = "composer_footer.rs"]
mod composer_footer;
#[path = "composer_menu.rs"]
mod composer_menu;
#[cfg(test)]
#[path = "composer_menu_test.rs"]
mod composer_menu_test;
#[path = "composer_picker.rs"]
mod composer_picker;
#[path = "composer_queue.rs"]
mod composer_queue;
#[cfg(test)]
#[path = "composer_queue_test.rs"]
mod composer_queue_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "composer_restore_test.rs"]
mod composer_restore_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "composer_review_test.rs"]
mod composer_review_test;
#[path = "composer_send.rs"]
mod composer_send;
#[path = "composer_state.rs"]
mod composer_state;
pub(in crate::workspace) use composer_state::machine_name;

#[cfg(test)]
use crate::workspace::*;
use crate::workspace::{github, lock, thread, thread_worktree};
pub(in crate::workspace) use composer_state::{
    Composer, SteerPrompt, Submit, bind_keys, shown_prompt,
};

use crate::ui;
use composer_menu::MenuKind;
use composer_picker::title_case;
use composer_state::{AttachFiles, MENU_CONTEXT, Menu};
use gpui::{
    AnyElement, Context, ExternalPaths, FontWeight, IntoElement, SharedString, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonRounded, ButtonVariants as _},
    h_flex,
    input::Textarea,
    v_flex,
};
use gpui_kit_assets::IconName as Lucide;
use skyelab_agent::{AttachmentKind, CLAUDE_MODELS};
use thread::{SendPrompt, ThreadView};

/// Under this card width the control row goes compact (T3 collapses its chips the same way).
const COMPACT_BELOW: gpui::Pixels = px(560.);

/// Whether a card this wide (0: not measured yet) gets the compact control row.
fn compact(width: gpui::Pixels) -> bool {
    width > px(0.) && width < COMPACT_BELOW
}

impl ThreadView {
    /// The model chip's label: "Opus · High", or "Default model". `compact` leaves the effort out.
    fn pick_label(&self, compact: bool) -> String {
        let pick = &self.composer.pick;
        let model = match pick.model.as_deref() {
            None => "Default model".to_owned(),
            Some(id) => CLAUDE_MODELS
                .iter()
                .find(|(alias, _)| *alias == id)
                .map(|(_, label)| (*label).to_owned())
                .or_else(|| {
                    let listed = self.composer.codex_models.as_ref()?.as_ref().ok()?;
                    listed.iter().find(|m| m.id == id).map(|m| m.label.clone())
                })
                .unwrap_or_else(|| id.to_owned()),
        };
        match &pick.effort {
            Some(e) if !compact => format!("{model} · {}", title_case(e)),
            _ => model,
        }
    }

    pub(in crate::workspace) fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let menu = self.composer.menu.as_ref();
        let menu_el = menu.map(|m| self.render_menu(m, cx));
        let picker = self.composer.picker_open.then(|| self.render_picker(cx));
        let attachments = self.render_attachments(cx);
        let contexts = self.render_contexts(cx);
        let controls = self.render_controls(cx);
        let queue = self.render_queue(cx);
        let footer = self.render_footer(cx);
        let view = cx.entity().downgrade();
        let (width, view_id) = (self.composer.width.clone(), cx.entity_id());
        // The card's width, read by the next frame's control row: crossing the compact line redraws once.
        let measure = gpui::canvas(
            move |bounds, _, cx| {
                let old = width.replace(bounds.size.width);
                if compact(old) != compact(bounds.size.width) {
                    cx.notify(view_id);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let theme = cx.theme();
        let card = v_flex()
            .id("composer-card")
            .relative()
            .w_full()
            .rounded(ui::RADIUS_XL)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .when(!theme.is_dark(), |this| this.shadow_xs())
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.border_color(cx.theme().ring))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.attach_paths(paths.paths().to_vec(), cx)
            }))
            .child(measure)
            .children(menu_el)
            .children(picker)
            .children(attachments)
            .children(contexts)
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .text_size(ui::BODY)
                    .when(menu.is_some(), |this| this.key_context(MENU_CONTEXT))
                    .child(Textarea::new(&self.prompt).appearance(false).on_paste(
                        move |item, _, cx| {
                            view.update(cx, |this, cx| this.attach_clipboard(item, cx))
                                .unwrap_or(false)
                        },
                    )),
            )
            .child(controls);
        v_flex()
            .w_full()
            .max_w(ui::CHAT_MAX_W)
            .children(queue)
            .child(card)
            .children(self.composer.notice.clone().map(|n| {
                div()
                    .px_2()
                    .pt_1()
                    .text_size(ui::META)
                    .text_color(ui::ink(theme, theme.danger))
                    .child(n)
            }))
            .child(footer)
            .into_any_element()
    }

    /// The row inside the card under the prompt, T3's: attach and the permission chip on the left; the model chip
    /// and Send on the right. A narrow card (a right panel open) keeps only the chips' icons and the model's name.
    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let width = self.composer.width.get();
        let compact = compact(width);
        h_flex()
            .px_2()
            .pb_2()
            .gap_0p5()
            .child(
                Button::new("attach")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Paperclip))
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
                    .label(self.pick_label(compact))
                    .dropdown_caret(true)
                    .tooltip("Model and effort")
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
            && self.composer.terminal_contexts.is_empty();
        let round = ButtonRounded::Size(px(16.));
        let mut out = Vec::new();
        if running && self.composer.stalled {
            out.push(
                Button::new("force-stop")
                    .ghost()
                    .small()
                    .label("Force stop")
                    .tooltip("The agent isn't answering Interrupt: end it (⌥⌘.)")
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
                    gpui_component::tooltip::Tooltip::new("Interrupt (⌘.)").build(window, cx)
                })
                .child(div().size(px(12.)).rounded(px(3.)).bg(gpui::white()))
                .on_click(
                    cx.listener(|this, _, window, cx| {
                        this.interrupt(&thread::Interrupt, window, cx)
                    }),
                )
                .into_any_element()
        } else {
            Button::new("send")
                .primary()
                .icon(IconName::ArrowUp)
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
                        AttachmentKind::Image(_) => IconName::GalleryVerticalEnd,
                        AttachmentKind::Pdf | AttachmentKind::Text => IconName::File,
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
                                .icon(Icon::new(Lucide::X))
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

    /// The `@` / `/` / `$` menu, floating above the card.
    fn render_menu(&self, menu: &Menu, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (title, empty) = match menu.kind {
            MenuKind::Files => ("Files", "No tracked file matches"),
            MenuKind::Commands => ("Commands", "No command matches"),
            MenuKind::Skills => ("Skills", "No skill in this account"),
        };
        let rows = menu.items.as_ref().map(|items| {
            menu.rows
                .iter()
                .enumerate()
                .filter_map(|(pos, &i)| Some((pos, items.get(i)?.clone())))
                .collect::<Vec<_>>()
        });
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
