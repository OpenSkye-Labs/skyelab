//! Waiting in gpui tests for work done by real processes (a fake agent, git), which can take many seconds on a
//! loaded machine.

use std::time::{Duration, Instant};

use gpui::{App, AsyncApp, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;

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
/// that clock 100 ms on, so the timers fire while the real process takes its time. Unix only: its callers drive the
/// fake agent, a shell script.
#[cfg(unix)]
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

/// Shuts `cx`'s app down when the test's thread ends. gpui's test teardown quits through the test platform, which does
/// nothing, so no window or view was ever dropped: every agent and shell a test started ran until the test binary
/// exited (about 200 fake agents by the end of the app suite). This runs `App::shutdown`, as the real app's quit does:
/// quit hooks run and the windows drop with every view in them, which ends their processes.
pub(crate) fn shut_down_at_test_end(cx: &mut App) {
    /// Each app, and a token whose other copy its quit hook holds: a shut down or released app has dropped that hook.
    struct Apps(RefCell<Vec<(AsyncApp, Rc<()>)>>);
    impl Drop for Apps {
        fn drop(&mut self) {
            for (app, token) in self.0.take() {
                if Rc::strong_count(&token) > 1 {
                    // A panic here (in a thread-local destructor) would abort the whole test binary.
                    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        app.update(|cx| cx.shutdown())
                    }));
                    if ended.is_err() {
                        eprintln!(
                            "shutting the test's app down failed: what it started may still run"
                        );
                    }
                }
            }
        }
    }
    thread_local!(static APPS: Apps = const { Apps(RefCell::new(Vec::new())) });
    let token = Rc::new(());
    let held = token.clone();
    cx.on_app_quit(move |_| {
        let _ = &held;
        async {}
    })
    .detach();
    APPS.with(|apps| apps.0.borrow_mut().push((cx.to_async(), token)));
}
