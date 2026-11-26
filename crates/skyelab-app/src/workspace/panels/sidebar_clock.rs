//! The sidebar rows' times ("3m", "Working 1m 12s") move on without a reload. While a row works the list redraws each
//! second; otherwise a minute timer redraws only when a time label would read differently, and only while the window
//! is active (coming back to it checks at once). docs/gpui/redraw.md: no per-second ticks for minute labels, no
//! redraws for nothing.

use super::Sidebar;
use crate::ui;
use gpui::{Context, Subscription, Task, Window};
use std::hash::{Hash as _, Hasher as _};
use std::time::Duration;

#[cfg(test)]
#[path = "sidebar_clock_test.rs"]
mod tests;

pub(in crate::workspace) struct Clock {
    /// The time rows are drawn at (ms since the epoch); a test moves it.
    pub(in crate::workspace) now: fn() -> i64,
    active: bool,
    /// The time labels as last drawn, hashed.
    drawn: u64,
    /// Which timer runs: true each second, false each minute.
    seconds: bool,
    tick: Option<Task<()>>,
    _activation: Subscription,
}

impl Clock {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Sidebar>) -> Self {
        Self {
            now: ui::now_ms,
            active: window.is_window_active(),
            drawn: 0,
            seconds: false,
            tick: None,
            _activation: cx.observe_window_activation(window, |this, window, cx| {
                this.clock.active = window.is_window_active();
                this.refresh_times(cx);
            }),
        }
    }
}

impl Sidebar {
    /// Every time label the list can show, at `now`.
    fn time_labels(&self, now: i64) -> Vec<String> {
        let shelved = self.shelves.iter().flat_map(|s| &s.items);
        let archived = self.archived.iter().flat_map(|a| &a.items);
        self.sections
            .iter()
            .flat_map(|s| &s.items)
            .chain(shelved)
            .chain(archived)
            .filter_map(|i| i.updated_ms.map(|t| ui::ago(now, t)))
            .collect()
    }

    fn time_hash(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.time_labels((self.clock.now)()).hash(&mut h);
        h.finish()
    }

    /// Redraws when a time label changed since the last draw, while the window is active.
    fn refresh_times(&mut self, cx: &mut Context<Self>) {
        if !self.clock.active {
            return;
        }
        let now = self.time_hash();
        if now != self.clock.drawn {
            self.clock.drawn = now;
            cx.notify();
        }
    }

    /// The rows were rebuilt (and redraw): note their labels, and run the second or the minute timer.
    pub(super) fn sync_tick(&mut self, cx: &mut Context<Self>) {
        self.clock.drawn = self.time_hash();
        let seconds = self.marks.values().any(|m| m.started_ms.is_some());
        if self.clock.tick.is_some() && self.clock.seconds == seconds {
            return;
        }
        self.clock.seconds = seconds;
        let every = Duration::from_secs(if seconds { 1 } else { 60 });
        self.clock.tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(every).await;
                let ticked = this.update(cx, |this, cx| {
                    if seconds {
                        cx.notify();
                    } else {
                        this.refresh_times(cx);
                    }
                });
                if ticked.is_err() {
                    return;
                }
            }
        }));
    }
}
