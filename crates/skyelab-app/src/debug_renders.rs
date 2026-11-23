//! Debug builds only: `SKYELAB_DEBUG_RENDERS=1` counts each view's `render` calls and prints them to stderr once a
//! second, so a redraw fix is measured instead of guessed (see docs/gpui.md, "Redraw discipline").

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

static COUNTS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        cfg!(debug_assertions) && std::env::var("SKYELAB_DEBUG_RENDERS").is_ok_and(|v| v == "1")
    })
}

/// One `render` of the view `name`. Free when the counter is off.
pub(crate) fn count(name: &'static str) {
    if on() {
        *COUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(name)
            .or_default() += 1;
    }
}

/// Starts the once-a-second log when the counter is on: `renders/s Workspace=60 ThreadView=58 …`, quiet seconds
/// skipped. `Workspace` renders once per window per frame, so it is the frame count.
pub(crate) fn start() {
    if !on() {
        return;
    }
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let counts = std::mem::take(&mut *COUNTS.lock().unwrap_or_else(|e| e.into_inner()));
            if !counts.is_empty() {
                let line: Vec<String> = counts.iter().map(|(k, v)| format!("{k}={v}")).collect();
                eprintln!("renders/s {}", line.join(" "));
            }
        }
    });
}
