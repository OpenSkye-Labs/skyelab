//! How the composer looks, after T3's (docs/design/t3-reference-chat.md §4): the queue above a rounded card holding the
//! attachments, the prompt and a control row (attach; model and effort, the permission slot and Send), and a footer
//! strip under it (machine, account, checkout, changes, branch). Behavior lives in `composer_state.rs`.

use super::composer_menu::MenuKind;
use super::composer_picker::title_case;
use super::composer_state::{AttachFiles, MENU_CONTEXT, Menu};
use super::thread::{SendPrompt, ThreadView};
use super::thread_worktree::{OpenChanges, ToggleWorktree};
use crate::ui;
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

impl ThreadView {
    /// The model picker's button label: "Opus High", or "Default model".
    fn pick_label(&self) -> String {
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
            Some(e) => format!("{model} {}", title_case(e)),
            None => model,
        }
    }

    pub(super) fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let menu = self.composer.menu.as_ref();
        let menu_el = menu.map(|m| self.render_menu(m, cx));
        let picker = self.composer.picker_open.then(|| self.render_picker(cx));
        let attachments = self.render_attachments(cx);
        let controls = self.render_controls(cx);
        let queue = self.render_queue(cx);
        let footer = self.render_footer(cx);
        let view = cx.entity().downgrade();
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
            .children(menu_el)
            .children(picker)
            .children(attachments)
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

    /// The row inside the card under the prompt: attach on the left; model, the permission slot and Send on the right.
    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let running = self.turn_running();
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
            .child(div().flex_1())
            .child(
                Button::new("model-picker")
                    .ghost()
                    .small()
                    .label(self.pick_label())
                    .dropdown_caret(true)
                    .tooltip("Model and effort")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_picker(cx))),
            )
            // Slot for the permission-mode picker (another branch adds it).
            .child(div().id("permission-slot"))
            .child(
                h_flex().pl_2().child(
                    Button::new("send")
                        .primary()
                        .icon(IconName::ArrowUp)
                        .rounded(ButtonRounded::Size(px(16.)))
                        .tooltip(if running {
                            "Queue for when the agent finishes (⌘↩). ⌘⇧↩ sends it now."
                        } else {
                            "Send (⌘↩)"
                        })
                        .disabled(self.read_only || self.starting)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.send_prompt(&SendPrompt, window, cx)
                        })),
                ),
            )
            .into_any_element()
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
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_attachment(ix, cx)
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

    /// Messages waiting for the running turn: dashed, unfilled bubbles (a turn not sent yet), each with Steer and Remove.
    fn render_queue(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.composer.queue.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            v_flex()
                .w_full()
                .items_end()
                .gap_2()
                .pb_2()
                .children(self.composer.queue.iter().map(|q| {
                    let id = q.id;
                    let shown = super::composer_state::shown_prompt(&q.text, &q.attachments);
                    v_flex()
                        .items_end()
                        .gap_1()
                        .max_w(gpui::relative(0.8))
                        .child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded(ui::RADIUS_XL)
                                .border_1()
                                .border_dashed()
                                .border_color(theme.muted_foreground.opacity(0.4))
                                .text_size(ui::BODY)
                                .text_color(theme.muted_foreground)
                                .line_clamp(3)
                                .child(shown),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(Icon::new(Lucide::Clock).xsmall())
                                .child("Sends when the agent finishes")
                                .child(
                                    Button::new(SharedString::from(format!("steer-{id}")))
                                        .ghost()
                                        .xsmall()
                                        .label("Steer")
                                        .tooltip("Send into the current turn now (⌘⇧↩)")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.steer_queued(id, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("unqueue-{id}")))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(Lucide::X))
                                        .tooltip("Remove from the queue")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.remove_queued(id, cx)
                                        })),
                                ),
                        )
                }))
                .into_any_element(),
        )
    }

    /// The strip under the card: where it runs (machine, account, checkout) on the left, the git state on the right.
    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let fg = if theme.is_dark() {
            theme.muted_foreground
        } else {
            theme.foreground.opacity(0.7)
        };
        let footer = &self.composer.footer;
        let divider = || div().w(px(1.)).h(px(14.)).bg(theme.border);
        let item = |icon: Lucide, label: SharedString| {
            h_flex()
                .min_w_0()
                .gap_1p5()
                .child(Icon::new(icon).xsmall())
                .child(div().min_w_0().truncate().child(label))
        };
        let worktree = self.thread.as_ref().and_then(|t| t.worktree_branch.clone());
        let checkout: AnyElement = match (&self.thread, worktree) {
            (_, Some(_)) => item(Lucide::FolderGit2, "Worktree".into()).into_any_element(),
            (Some(_), None) => item(Lucide::Folder, "Local checkout".into()).into_any_element(),
            // Before the first send: a toggle between this checkout and a new worktree.
            (None, None) => {
                let (icon, label) = if self.wt.use_worktree {
                    (Lucide::FolderGit2, "New worktree")
                } else {
                    (Lucide::Folder, "Local checkout")
                };
                Button::new("checkout-mode")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(icon))
                    .label(label)
                    .tooltip("Run here, or in a git worktree on a new skyelab/… branch")
                    .disabled(self.read_only)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_worktree(&ToggleWorktree, window, cx)
                    }))
                    .into_any_element()
            }
        };
        h_flex()
            .id("composer-footer")
            .w_full()
            .min_w_0()
            .h(px(28.))
            .px_2()
            .pt_1()
            .gap_2()
            .text_size(ui::META)
            .text_color(fg)
            .children(footer.machine.clone().map(|m| item(Lucide::Laptop, m)))
            .children(footer.machine.as_ref().map(|_| divider()))
            .child(div().id("provider-badge").child(self.account_badge()))
            .child(divider())
            .child(checkout)
            .child(div().flex_1())
            .children(footer.changes.map(|n| {
                Button::new("footer-changes")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(Lucide::FileDiff))
                    .label(if n == 0 {
                        "No changes".into()
                    } else {
                        format!("{n} changed")
                    })
                    .tooltip("Open Changes")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_changes(&OpenChanges, window, cx)
                    }))
            }))
            .children(footer.branch.clone().map(|b| item(Lucide::GitBranch, b)))
            .into_any_element()
    }
}

/// A floating panel above the composer card, T3's popup surface.
pub(super) fn popover(cx: &gpui::App) -> gpui::Div {
    let theme = cx.theme();
    v_flex()
        .absolute()
        .bottom_full()
        .mb_2()
        .p_1()
        .max_h(px(320.))
        .overflow_hidden()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_md()
}

pub(super) fn hint(text: impl Into<SharedString>, cx: &gpui::App) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_size(ui::META)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}
