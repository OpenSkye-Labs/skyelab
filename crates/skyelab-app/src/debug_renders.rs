//! Per-view render counts, so a redraw fix is measured instead of guessed (docs/gpui/redraw.md): debug mode's overlay
//! shows the busiest views, and `SKYELAB_DEBUG_RENDERS=1` prints `renders/s Workspace=17 ThreadView=16 …` to stderr once
//! a second. Each render is also a `render` span in a `SKYELAB_TRACE` trace. Off, a count is one atomic load.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, PoisonError};

static COUNTS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());
/// The last full second's counts, busiest first.
static LAST: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());
static PRINT: AtomicBool = AtomicBool::new(false);

fn on() -> bool {
    skyelab_core::debug::enabled() || PRINT.load(Ordering::Relaxed)
}

/// One `render` of the view `name`: call it first in the render and keep the guard (`let _render = …`), so the trace
/// span covers the render.
pub(crate) fn count(name: &'static str) -> tracing::span::EnteredSpan {
    #[cfg(test)]
    TEST_COUNTS.with(|c| *c.borrow_mut().entry(name).or_default() += 1);
    if on() {
        *COUNTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(name)
            .or_default() += 1;
    }
    tracing::info_span!("render", view = name).entered()
}

/// The last second's renders per view, busiest first.
pub(crate) fn last_second() -> Vec<(&'static str, u64)> {
    LAST.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Starts the once-a-second roll (and the stderr line when `print`). Safe to call again: one thread at most.
pub(crate) fn start(print: bool) {
    if print {
        PRINT.store(true, Ordering::Relaxed);
    }
    if !on() {
        return;
    }
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let counts =
                    std::mem::take(&mut *COUNTS.lock().unwrap_or_else(PoisonError::into_inner));
                let mut sorted: Vec<_> = counts.into_iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
                if PRINT.load(Ordering::Relaxed) && !sorted.is_empty() {
                    let line: Vec<String> =
                        sorted.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    eprintln!("renders/s {}", line.join(" "));
                }
                *LAST.lock().unwrap_or_else(PoisonError::into_inner) = sorted;
            }
        });
    });
}

#[cfg(test)]
thread_local! {
    /// Tests count every render on their own thread, whatever the switches say (GPUI tests render on the test thread).
    static TEST_COUNTS: std::cell::RefCell<BTreeMap<&'static str, u64>> = Default::default();
}

/// Renders of `name` on this thread since the last call (tests only).
#[cfg(test)]
pub(crate) fn take_test_count(name: &str) -> u64 {
    TEST_COUNTS.with(|c| c.borrow_mut().remove(name).unwrap_or(0))
}
