//! Under and above the composer card: the queue of waiting messages and the footer strip.

use super::thread::ThreadView;
use super::thread_worktree::{OpenChanges, ToggleWorktree};
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};

/// The machine the thread runs on, as a chip that opens the machine menu (T3's environment selector).
pub(in crate::workspace) fn machine_chip(
    name: SharedString,
    icon: SkyeIcon,
    hover: gpui::Hsla,
) -> gpui::Stateful<gpui::Div> {
    let tip: SharedString = format!("Runs on {name}. Click to switch machine").into();
    h_flex()
        .id("composer-machine")
        .flex_shrink_0()
        .gap_1p5()
        .px_1p5()
        .py_0p5()
        .rounded(ui::RADIUS)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .tooltip(move |window, cx| crate::ui::Tooltip::new(tip.clone()).build(window, cx))
        .on_click(|e, window, cx| {
            crate::workspace::dispatch(
                Box::new(crate::workspace::env_picker::ShowEnvironments::at(
                    e.position(),
                )),
                window,
                cx,
            )
        })
        .child(Icon::new(icon).xsmall())
        .child(name)
        .child(Icon::new(SkyeIcon::ChevronsUpDown).xsmall())
}

impl ThreadView {
    /// Messages waiting for the running turn: dashed, unfilled bubbles (a turn not sent yet), each with Steer and Remove.
    pub(in crate::workspace) fn render_queue(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.composer.queue.is_empty() {
            return None;
        }
        let can_steer =
            self.composer_account().map(|a| a.provider) != Some(skyelab_core::Provider::Cursor);
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
                                .child(Icon::new(SkyeIcon::Clock).xsmall())
                                .child("Sends when the agent finishes")
                                .when(can_steer, |el| {
                                    el.child(
                                        Button::new(SharedString::from(format!("steer-{id}")))
                                            .ghost()
                                            .xsmall()
                                            .label("Steer")
                                            .tooltip(format!(
                                                "Send into the current turn now ({})",
                                                ui::keys("⌘⇧↩")
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.steer_queued(id, window, cx)
                                            })),
                                    )
                                })
                                .child(
                                    Button::new(SharedString::from(format!("unqueue-{id}")))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(SkyeIcon::X))
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
    pub(in crate::workspace) fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let fg = if theme.is_dark() {
            theme.muted_foreground
        } else {
            theme.foreground.opacity(0.7)
        };
        let footer = &self.composer.footer;
        let divider = || div().w(px(1.)).h(px(14.)).bg(theme.border);
        let item = |icon: SkyeIcon, label: SharedString| {
            h_flex()
                .min_w_0()
                .gap_1p5()
                .child(Icon::new(icon).xsmall())
                .child(div().min_w_0().truncate().child(label))
        };
        let worktree = self.thread.as_ref().and_then(|t| t.worktree_branch.clone());
        let checkout: AnyElement = match (&self.thread, worktree) {
            (_, Some(_)) => item(SkyeIcon::FolderGit2, "Worktree".into()).into_any_element(),
            (Some(_), None) => item(SkyeIcon::Folder, "Local checkout".into()).into_any_element(),
            // Before the first send: a toggle between this checkout and a new worktree.
            (None, None) => {
                let (icon, label) = if self.wt.use_worktree {
                    (SkyeIcon::FolderGit2, "New worktree")
                } else {
                    (SkyeIcon::Folder, "Local checkout")
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
            .children(
                footer
                    .machine
                    .clone()
                    .map(|m| machine_chip(m, SkyeIcon::Monitor, theme.accent)),
            )
            .children(footer.machine.as_ref().map(|_| divider()))
            .child(div().id("provider-badge").child(self.account_badge()))
            .child(divider())
            .child(checkout)
            .child(div().flex_1())
            .children(self.context_meter().map(|m| {
                let color = super::thread::context_meter::meter_color(&m, theme);
                let tip = m.tooltip();
                h_flex()
                    .id("footer-context")
                    .debug_selector(|| "footer-context".into())
                    .flex_shrink_0()
                    .gap_1()
                    .text_color(color)
                    .child(m.text())
                    .when(m.near_full(), |el| el.child("· /compact"))
                    .tooltip(move |window, cx| {
                        crate::ui::Tooltip::new(tip.clone()).build(window, cx)
                    })
            }))
            .children(footer.changes.map(|n| {
                Button::new("footer-changes")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(SkyeIcon::FileDiff))
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
            .children(footer.branch.clone().map(|b| item(SkyeIcon::GitBranch, b)))
            .into_any_element()
    }
}
