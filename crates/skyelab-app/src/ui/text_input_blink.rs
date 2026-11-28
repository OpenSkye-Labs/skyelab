//! The caret's blink: shown for a phase, hidden for a phase, held shown after the idle time. A phase notifies only the
//! input, which is a view of its own.

use super::text_input::TextInput;
use gpui::{Context, Task};
use std::time::{Duration, Instant};

/// How long the caret stays solid after a key or a click.
const PAUSE: Duration = Duration::from_millis(300);

#[derive(Default)]
pub(super) struct Blink {
    pub visible: bool,
    paused: bool,
    last_activity: Option<Instant>,
    task: Option<Task<()>>,
}

impl TextInput {
    pub(super) fn start_blink(&mut self, cx: &mut Context<Self>) {
        self.restart_blink(false, cx);
    }

    /// After a key or a click; an unfocused field has no blink to pause.
    pub(super) fn pause_blink(&mut self, cx: &mut Context<Self>) {
        if self.blink.last_activity.is_some() {
            self.restart_blink(true, cx);
        }
    }

    pub(super) fn stop_blink(&mut self, cx: &mut Context<Self>) {
        self.blink = Blink::default();
        cx.notify();
    }

    fn restart_blink(&mut self, paused: bool, cx: &mut Context<Self>) {
        self.blink.last_activity = Some(cx.background_executor().now());
        self.blink.paused = paused;
        self.show_caret(true, cx);
        self.schedule_blink(cx);
    }

    fn show_caret(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.blink.visible != visible {
            self.blink.visible = visible;
            cx.notify();
        }
    }

    /// Until the next phase; `None` once idle.
    fn blink_delay(&self, cx: &Context<Self>) -> Option<Duration> {
        let elapsed = cx.background_executor().now() - self.blink.last_activity?;
        let phase = super::CURSOR_BLINK.interval()?;
        let left = super::CURSOR_BLINK.idle_timeout()?.checked_sub(elapsed)?;
        let delay = match PAUSE.checked_sub(elapsed) {
            Some(rest) if self.blink.paused => rest,
            _ => phase,
        };
        (!left.is_zero()).then(|| delay.min(left))
    }

    fn schedule_blink(&mut self, cx: &mut Context<Self>) {
        let Some(delay) = self.blink_delay(cx) else {
            self.blink.task = None;
            self.show_caret(true, cx);
            return;
        };
        let timer = cx.background_executor().timer(delay);
        self.blink.task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            _ = this.update(cx, |this, cx| {
                if this.blink_delay(cx).is_some() {
                    this.blink.paused = false;
                    this.show_caret(!this.blink.visible, cx);
                }
                this.schedule_blink(cx);
            });
        }));
    }
}
