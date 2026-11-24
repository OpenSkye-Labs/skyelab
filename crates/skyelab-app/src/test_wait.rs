//! Waiting in gpui tests for work done by real processes (a fake agent, git), which can take many seconds on a
//! loaded machine.

use std::time::{Duration, Instant};

use gpui::VisualTestContext;

/// How long a wait may take before the test fails: by the clock, not by a count of polls, so a slow machine gets
/// the same budget and a stuck one still ends.
pub(crate) const DEADLINE: Duration = Duration::from_secs(60);

/// Polls `done` (after letting the app run) until it holds, failing the test after [`DEADLINE`].
pub(crate) fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let until = Instant::now() + DEADLINE;
    while Instant::now() < until {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// [`wait_until`] for work that also waits on the executor's clock (a retry timer, an exit watch): each poll moves
/// that clock 100 ms on, so the timers fire while the real process takes its time.
pub(crate) fn wait_ticking(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    wait_until(cx, what, |cx| {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        done(cx)
    });
}
