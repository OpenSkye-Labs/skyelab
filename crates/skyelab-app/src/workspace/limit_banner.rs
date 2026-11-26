//! The usage limit banner over the composer (T3's UsageLimitRecoveryBanner): **Usage limit reached · Resets tomorrow at
//! 2:10 AM**, with Resume at reset (the thread continues by itself once, at the reset) and Snooze until reset. The
//! schedule is stored on the thread's row (`limit_resume_at`) and sent by the workspace's timer, through the same
//! resume a typed follow-up takes: the thread's own account and saved permission mode, refused if its folder moved to
//! another account.

use super::thread::{ThreadView, ThreadViewEvent};
use super::thread_list::{Shelf, ShelveThread};
use super::{Workspace, lock};
use crate::ui::toast::{self, Toast};
use crate::ui::{self, Button, SkyeIcon};
use gpui::{AnyElement, AppContext as _, Context, FontWeight, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, h_flex};
use skyelab_core::{limit_reset, snooze};

/// How often the banner's "Resets in 25 min" is read again.
const TICK: std::time::Duration = std::time::Duration::from_secs(30);

impl ThreadView {
    /// `Some(reset)` while the limit holds (`None` inside: the agent didn't say when it resets), in ms.
    fn limit_reset_ms(&self) -> Option<Option<i64>> {
        self.limited.map(|r| r.map(|s| s.saturating_mul(1000)))
    }

    pub(in crate::workspace) fn render_limit_banner(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reset = self.limit_reset_ms()?;
        let thread = self.thread.as_ref()?;
        let now = ui::now_ms();
        if self.limit_tick.is_none() && reset.is_some_and(|r| r > now) {
            self.limit_tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TICK).await;
                let _ = this.update(cx, |this, cx| {
                    this.limit_tick = None;
                    cx.notify();
                });
            }));
        }
        let scheduled = thread.limit_resume_at.is_some();
        let snoozed = snooze::is_asleep(thread.snoozed_until, now);
        let local = chrono::Local::now();
        let detail = match reset {
            None => "Reset time unavailable; retry manually".to_owned(),
            Some(r) => match thread
                .limit_resume_at
                .and_then(|at| limit_reset::reset_when(at, &local))
            {
                Some(when) => format!("Resumes {when}"),
                None => limit_reset::reset_words(r, &local),
            },
        };
        let passed = reset.is_some_and(|r| r <= now);
        let resume_label = if scheduled {
            "Cancel auto-resume"
        } else if passed {
            "Resume now"
        } else {
            "Resume at reset"
        };
        let actions = reset.is_some() && !self.read_only;
        let theme = cx.theme();
        let ink = ui::ink(theme, theme.warning);
        Some(
            h_flex()
                .debug_selector(|| "limit-banner".into())
                .w_full()
                .max_w(ui::CHAT_MAX_W)
                // T3's composer banner: one line, so it never pushes the chat or a docked question up. Only a center
                // too narrow for the text and the buttons side by side wraps the buttons under it.
                .flex_wrap()
                .mb_1p5()
                .pl_3()
                .pr_1p5()
                .py_1()
                .gap_2()
                .rounded(ui::RADIUS_LG)
                .bg(theme.warning.opacity(0.08))
                .border_1()
                .border_color(theme.warning.opacity(0.35))
                .child(
                    Icon::new(SkyeIcon::TriangleAlert)
                        .size_3p5()
                        .text_color(ink),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(160.))
                        .gap_2()
                        .items_baseline()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(ui::BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .child("Usage limit reached"),
                        )
                        .child(
                            div()
                                .debug_selector(|| "limit-banner-reset".into())
                                .min_w_0()
                                .truncate()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(detail),
                        ),
                )
                .when(actions, |el| {
                    el.child(
                        h_flex()
                            .flex_shrink_0()
                            .gap_0p5()
                            .child(
                                div().debug_selector(|| "limit-resume".into()).child(
                                    Button::new("limit-resume")
                                        .ghost()
                                        .xsmall()
                                        .label(resume_label)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.toggle_limit_resume(window, cx)
                                        })),
                                ),
                            )
                            .child(
                                div().debug_selector(|| "limit-snooze".into()).child(
                                    Button::new("limit-snooze")
                                        .ghost()
                                        .xsmall()
                                        .label(if snoozed {
                                            "Wake now"
                                        } else {
                                            "Snooze until reset"
                                        })
                                        .disabled(!snoozed && passed)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.toggle_limit_snooze(window, cx)
                                        })),
                                ),
                            ),
                    )
                })
                .into_any_element(),
        )
    }

    /// Resume at reset ⇄ Cancel auto-resume; Resume now once the reset has passed.
    pub(in crate::workspace) fn toggle_limit_resume(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Some(reset)) = self.limit_reset_ms() else {
            return;
        };
        if self
            .thread
            .as_ref()
            .is_some_and(|t| t.limit_resume_at.is_some())
        {
            return self.set_limit_resume(None, cx);
        }
        if reset > ui::now_ms() {
            return self.set_limit_resume(Some(reset), cx);
        }
        if let Err(why) = self.submit_remote(limit_reset::RESUME_PROMPT, window, cx) {
            toast::push(
                window,
                Toast::error(why).title("Could not resume the thread"),
                cx,
            );
        }
    }

    /// Snooze until reset ⇄ Wake now, through the sidebar's snooze. Refused once the reset has passed, as in T3.
    fn toggle_limit_snooze(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Some(reset)), Some(t)) = (self.limit_reset_ms(), self.thread.as_mut()) else {
            return;
        };
        let now = ui::now_ms();
        let to = if snooze::is_asleep(t.snoozed_until, now) {
            t.snoozed_until = None;
            Shelf::Wake
        } else if let Some(until) = limit_reset::snooze_until(reset, now) {
            t.snoozed_until = Some(until);
            Shelf::Snooze(until)
        } else {
            let why = "The reset time has passed. Retry the thread manually.";
            toast::push(
                window,
                Toast::error(why).title("Could not snooze thread"),
                cx,
            );
            return cx.notify();
        };
        let (id, host) = (t.id, None);
        crate::workspace::dispatch(Box::new(ShelveThread { id, to, host }), window, cx);
        cx.notify();
    }

    /// The user sent first: T3 drops the resume scheduled for the limit's reset.
    pub(in crate::workspace) fn sent_before_resume(&mut self, cx: &mut Context<Self>) {
        if self
            .thread
            .as_ref()
            .is_some_and(|t| t.limit_resume_at.is_some())
        {
            self.set_limit_resume(None, cx);
        }
    }

    /// Stores the thread's resume time (`None` cancels it) and tells the workspace, whose timer sends it.
    pub(in crate::workspace) fn set_limit_resume(
        &mut self,
        at: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        let Some(t) = self.thread.as_mut() else {
            return;
        };
        t.limit_resume_at = at;
        let (id, store) = (t.id, self.backend.store.clone());
        let write = cx.background_spawn(async move { lock(&store)?.set_limit_resume(id, at) });
        cx.spawn(async move |this, cx| {
            if let Err(e) = write.await {
                let _ = this.update(cx, |this, cx| this.show_error(e, cx));
            }
        })
        .detach(); // one short write; `this` is weak
        cx.emit(ThreadViewEvent::LimitResume(at));
        cx.notify();
    }
}

impl Workspace {
    /// A tab scheduled or cancelled its resume. Every window keeps its own rows, so each arms its timer; the store
    /// lets one of them send.
    pub(in crate::workspace) fn limit_resume_changed(
        &mut self,
        view: &gpui::Entity<ThreadView>,
        at: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) {
            self.update_row(id, |t| t.limit_resume_at = at, cx);
        }
    }

    /// Wakes at the next scheduled limit resume. Like the snooze timer, it reads the wall clock again at least each
    /// minute, since the monotonic timer stops while the machine sleeps.
    pub(in crate::workspace) fn arm_limit_resume(&mut self, cx: &mut Context<Self>) {
        let next = if self.is_remote() {
            None // the rows shown are another machine's; its own app resumes them
        } else {
            self.threads
                .iter()
                .filter(|t| t.archived_at.is_none())
                .filter_map(|t| t.limit_resume_at)
                .min()
        };
        if self.previews.limit_resume.as_ref().map(|w| w.0) == next {
            return;
        }
        let handle = self.window_handle;
        self.previews.limit_resume = next.map(|at| {
            let wait = (at - ui::now_ms()).clamp(0, 60_000).unsigned_abs();
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(wait))
                    .await;
                let _ = cx.update_window(handle, |_, window, cx| {
                    this.update(cx, |ws, cx| {
                        ws.previews.limit_resume = None;
                        ws.send_due_resumes(window, cx);
                        ws.arm_limit_resume(cx);
                    })
                });
            });
            (at, task)
        });
    }

    /// Sends each due resume once: the store claims it first, so another window (or a second timer) can't send it too.
    pub(in crate::workspace) fn send_due_resumes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = ui::now_ms();
        let due: Vec<i64> = self
            .threads
            .iter()
            .filter(|t| {
                t.limit_resume_at
                    .is_some_and(|at| limit_reset::due(at, now))
            })
            .map(|t| t.id)
            .collect();
        for id in due {
            self.update_row(id, |t| t.limit_resume_at = None, cx);
            let store = self.backend.store.clone();
            let claim = cx.background_spawn(async move {
                let s = lock(&store)?;
                anyhow::Ok(if s.take_limit_resume(id, now)? {
                    s.thread(id)?
                } else {
                    None
                })
            });
            cx.spawn_in(window, async move |this, cx| {
                let Ok(Some(row)) = claim.await else {
                    return; // already sent, cancelled, archived or deleted
                };
                let _ = this.update_in(cx, |ws, window, cx| ws.resume_limited(row, window, cx));
            })
            .detach(); // one short claim; `this` is weak
        }
    }

    /// Continues thread `row` as T3 does: "Continue where you left off." in the same session, from its open tab, or one
    /// opened in the background.
    fn resume_limited(
        &mut self,
        row: skyelab_store::Thread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found = self
            .open_view(row.id, cx)
            .or_else(|| self.shared_view(row.id, cx));
        let view = found.unwrap_or_else(|| {
            let (config, backend) = (self.config.clone(), self.backend.clone());
            let view = cx.new(|cx| ThreadView::open(row, config, backend, window, cx));
            super::projects::name_project(&self.projects, &view, cx);
            self.add_thread_tab(view.clone(), false, window, cx);
            view
        });
        if !self.open_threads.contains(&view) {
            self.track_thread(&view, window, cx);
        }
        view.update(cx, |v, _| {
            if let Some(t) = v.thread.as_mut() {
                t.limit_resume_at = None; // sent now: the prompt below must not cancel anything
            }
        });
        if view.read(cx).read_only {
            // Opened just now: it sends once its row has loaded.
            return self.ask_when_ready(&view, limit_reset::RESUME_PROMPT.to_owned(), window, cx);
        }
        let sent = view.update(cx, |v, cx| {
            v.submit_remote(limit_reset::RESUME_PROMPT, window, cx)
        });
        if let Err(why) = sent {
            toast::push(
                window,
                Toast::error(why).title("Could not resume the thread"),
                cx,
            );
        }
    }
}
