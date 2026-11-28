//! The Changes tab's turn scopes (T3's diff panel: the working tree, the whole thread, or one turn) and "Rewind to
//! before turn N", which asks first with every file it would change.

use super::changes::ChangesView;
use super::changes_model::{self, Scope};
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::{Button, Checkbox};
use crate::ui::{h_flex, v_flex};
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*};

/// A rewind asked for while a turn runs: `_watch` asks it again once the turn ends.
pub(super) struct RewindWait {
    pub turn: u32,
    /// Stop was pressed.
    pub stopping: bool,
    _watch: gpui::Subscription,
}

/// A rewind the user was shown and has to confirm.
pub(super) struct PendingRewind {
    pub turn: u32,
    pub plan: skyelab_git::RewindPlan,
    pub overwrite_ignored: bool,
}

impl ChangesView {
    /// The checkpoint key of the thread this tab follows, if it has a row yet.
    pub(super) fn checkpoint_key(&self, cx: &gpui::App) -> Option<String> {
        let view = self.thread.as_ref()?.upgrade()?;
        let thread = view.read(cx).thread.as_ref()?;
        Some(super::thread_checkpoints::key(thread))
    }

    pub(super) fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if self.scope != scope {
            self.scope = scope;
            self.rewind = None;
            self.refresh(cx);
        }
    }

    /// No turn may run: rewinding under one would race its writes. An idle agent between turns writes nothing.
    fn turn_busy(&self, cx: &gpui::App) -> bool {
        self.thread
            .as_ref()
            .and_then(|t| t.upgrade())
            .is_some_and(|t| t.read(cx).turn_running() || t.read(cx).starting)
    }

    /// The running turn's Stop: interrupts it, as the composer's Stop does; the wait's watch asks the rewind again.
    pub(super) fn stop_for_rewind(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(thread), Some(wait)) = (
            self.thread.as_ref().and_then(|t| t.upgrade()),
            self.rewind_wait.as_mut(),
        ) else {
            return;
        };
        wait.stopping = true;
        thread.update(cx, |t, cx| {
            t.interrupt(&super::thread::Interrupt, window, cx)
        });
        cx.notify();
    }

    /// Works out what rewinding to before `turn` changes, and shows it for confirming.
    pub(super) fn ask_rewind(&mut self, turn: u32, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.turn_busy(cx)
            && let Some(thread) = self.thread.as_ref().and_then(|t| t.upgrade())
        {
            let _watch = cx.observe(&thread, |this, _, cx| {
                if !this.turn_busy(cx)
                    && let Some(wait) = this.rewind_wait.take()
                {
                    this.ask_rewind(wait.turn, cx);
                }
            });
            self.rewind_wait = Some(RewindWait {
                turn,
                stopping: false,
                _watch,
            });
            cx.notify();
            return;
        }
        self.rewind_wait = None;
        let Some(to) = self
            .changes
            .as_ref()
            .and_then(|c| c.checkpoints.iter().find(|c| c.turn + 1 == turn))
            .map(|c| c.tree.clone())
        else {
            return;
        };
        self.busy = true;
        let dir = self.dir.clone();
        let task = cx.background_spawn(async move { skyelab_git::plan_rewind(&dir, &to) });
        self._publish = Some(cx.spawn(async move |this, cx| {
            let plan = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match plan {
                    Ok(plan) => {
                        this.publish_error = None;
                        this.rewind = Some(PendingRewind {
                            turn,
                            plan,
                            overwrite_ignored: false,
                        });
                    }
                    Err(e) => this.publish_error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    /// Rewinds exactly what the confirm showed; the rewind refuses if the files changed since.
    pub(super) fn confirm_rewind(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.turn_busy(cx) {
            return;
        }
        let Some(pending) = self.rewind.take() else {
            return;
        };
        self.busy = true;
        let dir = self.dir.clone();
        let turn = pending.turn;
        let task = cx.background_spawn(async move {
            skyelab_git::rewind(&dir, &pending.plan, pending.overwrite_ignored)
        });
        self._publish = Some(cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match done {
                    Ok(()) => {
                        this.publish_error = None;
                        this.publish_note =
                            Some(format!("Files rewound to before turn {turn}.").into());
                        this.refresh(cx);
                    }
                    Err(e) => this.publish_error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    /// The scope chips, shown once the thread has a checkpointed turn (or a turn scope is picked).
    pub(super) fn render_scopes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let turns = changes_model::turns(&self.changes.as_ref()?.checkpoints);
        if turns.is_empty() && self.scope == Scope::Working {
            return None;
        }
        let chip = |id: SharedString, label: String, scope: Scope| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .selected(self.scope == scope)
                .on_click(cx.listener(move |this, _, _, cx| this.set_scope(scope, cx)))
        };
        let theme = cx.theme();
        let rewind = match self.scope {
            Scope::Turn(n) => Some(
                // An action, not another scope tab: an icon, and "…" because it asks first.
                Button::new("rewind")
                    .ghost()
                    .xsmall()
                    .icon(crate::ui::SkyeIcon::Undo2)
                    .label(format!("Rewind to before turn {n}…"))
                    .tooltip("Shows every file it would change and asks before touching any")
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, _, cx| this.ask_rewind(n, cx))),
            ),
            _ => None,
        };
        Some(
            h_flex()
                .id("change-scopes")
                .px_4()
                .py_1p5()
                .gap_1()
                .flex_wrap()
                .border_b_1()
                .border_color(theme.border)
                .child(chip(
                    "scope-working".into(),
                    "Working tree".into(),
                    Scope::Working,
                ))
                .child(chip(
                    "scope-start".into(),
                    "Since thread start".into(),
                    Scope::SinceStart,
                ))
                .children(turns.into_iter().map(|n| {
                    chip(
                        format!("scope-turn-{n}").into(),
                        format!("Turn {n}"),
                        Scope::Turn(n),
                    )
                }))
                .child(div().flex_1())
                .children(rewind)
                .into_any_element(),
        )
    }

    /// The confirm row for a rewind: every file it touches, and whether ignored ones are overwritten.
    pub(super) fn render_rewind_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(w) = &self.rewind_wait {
            return Some(self.render_rewind_wait(w.turn, w.stopping, cx));
        }
        let pending = self.rewind.as_ref()?;
        let theme = cx.theme();
        let msg = changes_model::rewind_message(&pending.plan, pending.overwrite_ignored);
        let ignored = !pending.plan.ignored.is_empty();
        let ticked = pending.overwrite_ignored;
        // The file list can be long: it wraps on its own line, with the choices under it.
        Some(
            v_flex()
                .id("rewind-confirm")
                .debug_selector(|| "rewind-confirm-box".into())
                .mx_4()
                .my_2()
                .p_2()
                .gap_2()
                .rounded(ui::RADIUS)
                .border_1()
                .border_color(theme.danger)
                .child(
                    div()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("Rewind to before turn {}? {msg}", pending.turn)),
                )
                .child(
                    h_flex()
                        .debug_selector(|| "rewind-choices".into())
                        .justify_end()
                        .gap_2()
                        .when(ignored, |row| {
                            row.child(
                                Checkbox::new("rewind-ignored")
                                    .xsmall()
                                    .label("Overwrite ignored files")
                                    .checked(ticked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = this.rewind.as_mut() {
                                            p.overwrite_ignored = !p.overwrite_ignored;
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            Button::new("cancel-rewind")
                                .ghost()
                                .xsmall()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.rewind = None;
                                    cx.notify();
                                })),
                        )
                        .when(!pending.plan.is_empty(), |row| {
                            row.child(
                                Button::new("confirm-rewind")
                                    .danger()
                                    .xsmall()
                                    .label("Rewind files")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _: &mut Window, cx| {
                                        this.confirm_rewind(cx)
                                    })),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// A rewind asked for mid-turn: says why it waits, with the turn's Stop right there.
    fn render_rewind_wait(&self, turn: u32, stopping: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .id("rewind-wait")
            .debug_selector(|| "rewind-wait".into())
            .mx_4()
            .my_2()
            .p_2()
            .gap_2()
            .rounded(ui::RADIUS)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(if stopping {
                        format!(
                            "Stopping the turn… Rewind to before turn {turn} asks once it ends."
                        )
                    } else {
                        format!("A turn is running. Stop it to rewind to before turn {turn}.")
                    }),
            )
            .child(
                Button::new("cancel-rewind-wait")
                    .ghost()
                    .xsmall()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rewind_wait = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("stop-for-rewind")
                    .danger()
                    .xsmall()
                    .icon(crate::ui::SkyeIcon::Square)
                    .label("Stop")
                    .disabled(stopping)
                    .on_click(cx.listener(|this, _, window, cx| this.stop_for_rewind(window, cx))),
            )
            .into_any_element()
    }
}
