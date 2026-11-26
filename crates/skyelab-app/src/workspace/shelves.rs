//! T3's settle and snooze: a settled thread is done and lists in the Settled shelf; a snoozed one hides in the
//! Snoozed shelf until its time, then lists again as woke until it is opened. Both from the row's hover buttons and
//! its menu; settling and snoozing show an Undo notice that puts the thread back where it was. Also the sidebar
//! footer's and the subagent strip's actions. Another machine's threads go through its daemon (`remote_shelves.rs`).

use super::super::Workspace;
use super::super::manage::Job;
use super::super::panels::RowShelf;
use gpui::{Action, AppContext as _, Context, SharedString, Window, actions};
use gpui_component::menu::PopupMenu;
use skyelab_core::snooze;

/// Where a thread row goes: settled, back, snoozed until a time (ms since the epoch), or awake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shelf {
    Settle,
    Unsettle,
    Snooze(i64),
    /// Asks for the time ("3h", "18:30", "2026-12-01 09:00").
    SnoozeCustom,
    Wake,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ShelveThread {
    pub id: i64,
    pub to: Shelf,
    /// The machine the thread is on; `None` for this one.
    pub host: Option<SharedString>,
}

actions!(
    skyelab,
    [
        /// Settles the shown thread, or un-settles it.
        SettleThread,
        /// Snoozes the shown thread until a typed time.
        SnoozeThread,
        /// Reloads the thread list and its rows' previews.
        RefreshThreads
    ]
);

pub(in crate::workspace) const SETTLED_PAGE: usize = 25;

impl Workspace {
    pub(in crate::workspace) fn shelf_actions(
        &self,
        el: gpui::Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        el.on_action(cx.listener(Self::shelve_thread))
            .on_action(cx.listener(|ws, _: &SettleThread, window, cx| {
                let Some(id) = ws.shown_thread(cx) else {
                    return;
                };
                let settled = ws
                    .threads
                    .iter()
                    .any(|t| t.id == id && t.settled_at.is_some());
                let to = if settled {
                    Shelf::Unsettle
                } else {
                    Shelf::Settle
                };
                let host = None;
                ws.shelve_thread(&ShelveThread { id, to, host }, window, cx);
            }))
            .on_action(cx.listener(|ws, _: &SnoozeThread, window, cx| {
                if let Some(id) = ws.shown_thread(cx) {
                    let (to, host) = (Shelf::SnoozeCustom, None);
                    ws.shelve_thread(&ShelveThread { id, to, host }, window, cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &RefreshThreads, _, cx| ws.load_threads(cx)))
            .on_action(cx.listener(Self::stop_children))
    }

    pub(in crate::workspace) fn shelve_thread(
        &mut self,
        a: &ShelveThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = a.id;
        if let Shelf::SnoozeCustom = a.to {
            let host = a.host.clone();
            return self.prompt_text(
                "Snooze until (45m, 3h, 18:30 or 2026-12-01 09:00)",
                "3h".into(),
                window,
                cx,
                move |ws, text, window, cx| match snooze::parse_custom(&text, &chrono::Local::now())
                {
                    Some(until) => {
                        let (to, host) = (Shelf::Snooze(until), host.clone());
                        ws.shelve_thread(&ShelveThread { id, to, host }, window, cx)
                    }
                    None => ws.say(format!("“{text}” isn't a time ahead."), window, cx),
                },
            );
        }
        if let Some(host) = &a.host {
            return self.shelve_on(host.clone(), id, a.to, window, cx);
        }
        if self.is_remote() {
            return;
        }
        match a.to {
            Shelf::Settle => self.settle(id, true, true, window, cx),
            Shelf::Unsettle => self.settle(id, false, false, window, cx),
            Shelf::Snooze(until) => self.snooze(id, Some(until), true, window, cx),
            Shelf::Wake => self.snooze(id, None, false, window, cx),
            Shelf::SnoozeCustom => {} // asked above
        }
    }

    /// Where thread `id` sits now: (settled, snoozed until), for an undo to put it back.
    fn shelf_of(&self, id: i64) -> (bool, Option<i64>) {
        self.threads
            .iter()
            .find(|t| t.id == id)
            .map_or((false, None), |t| (t.settled_at.is_some(), t.snoozed_until))
    }

    fn restore(&mut self, id: i64, was: (bool, Option<i64>), cx: &mut Context<Self>) {
        self.put_on_shelf(id, was.0, was.1, cx, |ws, done, cx| {
            if let Err(e) = done {
                ws.store_error = Some(format!("{e:#}").into());
                cx.notify();
            }
        });
    }

    /// Puts thread `id` settled, snoozed until a time, or on neither shelf (a settle wins over a snooze), in the
    /// store and then its row; `done` hears how the write went.
    pub(in crate::workspace) fn put_on_shelf(
        &mut self,
        id: i64,
        settled: bool,
        until: Option<i64>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, anyhow::Result<()>, &mut Context<Self>) + 'static,
    ) {
        let until = until.filter(|_| !settled);
        let store = self.backend.store.clone();
        let write = cx.background_spawn(async move {
            let s = super::super::lock(&store)?;
            if settled {
                return s.set_thread_settled(id, true);
            }
            s.set_thread_settled(id, false)?;
            s.set_thread_snoozed(id, until)
        });
        cx.spawn(async move |this, cx| {
            let result = write.await;
            let _ = this.update(cx, |ws, cx| {
                if result.is_ok() {
                    ws.update_row(
                        id,
                        |t| {
                            t.settled_at = settled.then(crate::ui::now_ms);
                            t.snoozed_until = until;
                        },
                        cx,
                    );
                }
                done(ws, result, cx);
            });
        })
        .detach(); // one short write; `this` is weak
    }

    fn settle(
        &mut self,
        id: i64,
        on: bool,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.thread_title(id, cx).unwrap_or_default();
        let was = self.shelf_of(id);
        let job: Job = Box::new(move |s| s.set_thread_settled(id, on));
        let then = move |ws: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            ws.update_row(
                id,
                |t| {
                    t.settled_at = on.then(crate::ui::now_ms);
                    t.snoozed_until = t.snoozed_until.filter(|_| !on);
                },
                cx,
            );
            if undo {
                ws.thread_undo_notice(
                    format!("Settled “{title}”"),
                    move |ws, _, cx| ws.restore(id, was, cx),
                    window,
                    cx,
                );
            }
        };
        self.store_then(job, then, window, cx);
    }

    fn snooze(
        &mut self,
        id: i64,
        until: Option<i64>,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.thread_title(id, cx).unwrap_or_default();
        let was = self.shelf_of(id);
        let job: Job = Box::new(move |s| s.set_thread_snoozed(id, until));
        let then = move |ws: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            ws.update_row(
                id,
                |t| {
                    t.snoozed_until = until;
                    t.settled_at = t.settled_at.filter(|_| until.is_none());
                },
                cx,
            );
            if let Some(until) = until.filter(|_| undo) {
                let when = snooze::wake_label(until, crate::ui::now_ms());
                ws.thread_undo_notice(
                    format!("Snoozed “{title}” for {when}"),
                    move |ws, _, cx| ws.restore(id, was, cx),
                    window,
                    cx,
                );
            }
        };
        self.store_then(job, then, window, cx);
    }

    /// T3 clears Woke on a visit.
    pub(in crate::workspace) fn clear_woke(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // the shown thread is another machine's: its id means nothing here
        }
        let now = crate::ui::now_ms();
        let Some(t) = self
            .threads
            .iter_mut()
            .find(|t| t.id == id && snooze::is_woke(t.snoozed_until, now))
        else {
            return;
        };
        t.snoozed_until = None;
        let store = self.backend.store.clone();
        // Best effort, like the unread mark: the Woke label is a hint.
        cx.background_spawn(async move {
            if let Ok(s) = super::super::lock(&store) {
                let _ = s.clear_woke(id, now);
            }
        })
        .detach();
        self.refresh_sidebar(cx);
    }

    /// Redraws the sidebar when the next snoozed thread wakes. The timer is monotonic and stops while the machine
    /// sleeps, so it wakes at least each minute to read the wall clock again.
    pub(in crate::workspace) fn arm_wake(&mut self, cx: &mut Context<Self>) {
        self.arm_limit_resume(cx);
        let now = crate::ui::now_ms();
        // Other machines' snoozes wake here too: the row moves on this window's clock, as a local one does.
        let envs = self
            .remote_env()
            .into_iter()
            .chain(self.env.linked.iter().map(|l| l.env.clone()));
        let remote: Vec<i64> = envs
            .flat_map(|e| {
                e.read(cx)
                    .threads
                    .iter()
                    .filter_map(|t| t.snoozed_until)
                    .collect::<Vec<_>>()
            })
            .collect();
        let next = self
            .threads
            .iter()
            .filter_map(|t| t.snoozed_until)
            .chain(remote)
            .filter(|&u| u > now)
            .min();
        if self.previews.wake.as_ref().map(|w| w.0) == next {
            return;
        }
        self.previews.wake = next.map(|at| {
            let wait = std::time::Duration::from_millis((at - now).unsigned_abs().min(60_000));
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    this.previews.wake = None;
                    if at <= crate::ui::now_ms() {
                        this.refresh_sidebar(cx);
                    } else {
                        this.arm_wake(cx);
                    }
                });
            });
            (at, task)
        });
    }
}

/// The snooze choices (computed as the menu opens, so "Tonight" is only there while it's ahead), then Custom.
pub(in crate::workspace) fn snooze_menu(
    mut menu: PopupMenu,
    id: i64,
    host: Option<SharedString>,
) -> PopupMenu {
    for c in snooze::presets(&chrono::Local::now()) {
        let (to, host) = (Shelf::Snooze(c.until_ms), host.clone());
        menu = menu.menu(c.label, Box::new(ShelveThread { id, to, host }));
    }
    let to = Shelf::SnoozeCustom;
    menu.menu("Custom…", Box::new(ShelveThread { id, to, host }))
}

/// The row menu's settle and snooze entries, by where the thread is now.
pub(in crate::workspace) fn shelf_menu(
    menu: PopupMenu,
    s: &RowShelf,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let id = s.id;
    let host = s.host.clone();
    let entry = |label: &'static str, to| {
        let host = host.clone();
        (SharedString::from(label), ShelveThread { id, to, host })
    };
    let menu = menu.separator();
    let (label, action) = if s.settled {
        entry("Un-settle", Shelf::Unsettle)
    } else {
        entry("Settle", Shelf::Settle)
    };
    let menu = menu.menu(label, Box::new(action));
    if snooze::is_asleep(s.snoozed_until, crate::ui::now_ms()) {
        let (label, action) = entry("Wake now", Shelf::Wake);
        return menu.menu(label, Box::new(action));
    }
    menu.submenu("Snooze", window, cx, move |menu, _, _| {
        snooze_menu(menu, id, host.clone())
    })
}

#[path = "remote_shelves.rs"]
mod remote_shelves;

#[cfg(test)]
#[path = "settle_test.rs"]
mod tests;
