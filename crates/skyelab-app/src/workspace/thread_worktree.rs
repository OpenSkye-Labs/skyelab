//! A thread's worktree controls: the composer's opt-in box, the header actions, and discarding.

use super::lock;
use super::thread::{ThreadView, ThreadViewEvent};
use super::worktree::{self, Discard};
use crate::ui::Button;
use gpui::{AnyElement, Context, FontWeight, Window, actions, div, prelude::*};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

actions!(
    skyelab,
    [
        ToggleWorktree,
        OpenChanges,
        OpenWorktreeTerminal,
        DiscardWorktree,
        ConfirmDiscardWorktree,
        CancelDiscardWorktree
    ]
);

#[derive(Default)]
pub struct WorktreeState {
    /// "New worktree" (the checkout toggle under the composer); read when the first prompt starts the thread.
    pub use_worktree: bool,
    /// A multi-model send's shared base commit, so every column's worktree starts from the same one.
    pub(super) pin: Option<super::worktree::BasePin>,
    /// A dirty worktree's status the user was asked to confirm discarding.
    pub discard_pending: Option<skyelab_git::Status>,
    task: Option<gpui::Task<()>>,
    /// A discard is running; another click is ignored.
    busy: bool,
}

impl ThreadView {
    pub(super) fn toggle_worktree(
        &mut self,
        _: &ToggleWorktree,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.thread.is_none() && !self.starting {
            self.wt.use_worktree = !self.wt.use_worktree;
            cx.notify();
        }
    }

    pub(super) fn open_changes(&mut self, _: &OpenChanges, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ThreadViewEvent::OpenChanges);
    }

    pub(super) fn open_worktree_terminal(
        &mut self,
        _: &OpenWorktreeTerminal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(t) = &self.thread else { return };
        let Some(cwd) = t.worktree_path.clone() else {
            return;
        };
        let account_id = t.account_id.clone();
        cx.emit(ThreadViewEvent::OpenTerminal { cwd, account_id });
    }

    pub(super) fn discard_worktree(
        &mut self,
        _: &DiscardWorktree,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_discard(None, cx);
    }

    pub(super) fn confirm_discard(
        &mut self,
        _: &ConfirmDiscardWorktree,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(shown) = self.wt.discard_pending.clone() {
            self.run_discard(Some(shown), cx);
        }
    }

    pub(super) fn cancel_discard(
        &mut self,
        _: &CancelDiscardWorktree,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.wt.discard_pending = None;
        cx.notify();
    }

    fn run_discard(&mut self, confirmed: Option<skyelab_git::Status>, cx: &mut Context<Self>) {
        let Some(thread) = &self.thread else { return };
        let Some(path) = thread.worktree_path.clone() else {
            return;
        };
        if self.wt.busy {
            return;
        }
        if self.session.is_some() {
            self.error = Some("Stop the agent before discarding its worktree.".into());
            cx.notify();
            return;
        }
        self.wt.busy = true;
        let (store, id, branch) = (
            self.backend.store.clone(),
            thread.id,
            thread.worktree_branch.clone(),
        );
        self.wt.task = Some(cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move {
                    // A fork or handoff runs in its source's worktree: never pull it out from under a running agent.
                    if lock(&store)?.worktree_busy_elsewhere(id, &path)? {
                        anyhow::bail!(
                            "another thread's agent is running in this worktree (a fork or handoff shares it); stop it first"
                        );
                    }
                    let result = worktree::discard(&path, confirmed.as_ref())?;
                    if matches!(result, Discard::Removed) {
                        lock(&store)?.set_thread_worktree(id, None, branch.as_deref())?;
                    }
                    anyhow::Ok(result)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.wt.busy = false;
                match done {
                    Ok(Discard::Removed) => {
                        this.wt.discard_pending = None;
                        if let Some(t) = this.thread.as_mut() {
                            t.worktree_path = None;
                        }
                        this.error = None;
                    }
                    Ok(Discard::NeedsConfirm(st)) => this.wt.discard_pending = Some(st),
                    Err(e) => {
                        this.error =
                            Some(format!("{:#}", e.context("discarding the worktree")).into())
                    }
                }
                cx.notify();
            });
        }));
    }

    /// The confirm row for a dirty worktree, naming what would be lost.
    pub(super) fn render_discard_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let st = self.wt.discard_pending.as_ref()?;
        let theme = cx.theme();
        // The exact files the confirm covers; a confirm after they change is refused and asked again.
        let msg = worktree::discard_message(st);
        // The file list wraps on its own line, with the choices under it (a long one pushed them out of view).
        Some(
            v_flex()
                .mx_4()
                .mb_2()
                .p_2()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(theme.danger)
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(msg))
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-discard")
                                .ghost()
                                .xsmall()
                                .label("Keep it")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_discard(&CancelDiscardWorktree, window, cx)
                                })),
                        )
                        .child(
                            Button::new("confirm-discard")
                                .danger()
                                .xsmall()
                                .label("Discard anyway")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_discard(&ConfirmDiscardWorktree, window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
