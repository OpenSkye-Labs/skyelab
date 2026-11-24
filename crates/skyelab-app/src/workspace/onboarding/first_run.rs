//! Whether the first-run onboarding was finished or skipped (`onboarded` in settings.toml), so it opens by itself
//! only once. Without the global it counts as done: the app always installs it, and so do the tests about onboarding,
//! so no other test window opens onboarding (it takes the keyboard focus when it shows).

use gpui::{App, AppContext as _, Global};
use skyelab_core::AppSettings;
use std::path::PathBuf;

struct FirstRun {
    path: Option<PathBuf>,
    done: bool,
}

impl Global for FirstRun {}

/// Reads `onboarded` from `path` (missing or unreadable: not done).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let done = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .is_some_and(|s| s.onboarded);
    cx.set_global(FirstRun { path, done });
}

pub(crate) fn done(cx: &App) -> bool {
    cx.try_global::<FirstRun>().is_none_or(|f| f.done)
}

/// Marks it done now and saves that into settings.toml as it is on disk, off the UI thread.
pub(crate) fn set_done(cx: &mut App) {
    let path = cx.try_global::<FirstRun>().and_then(|f| f.path.clone());
    cx.set_global(FirstRun {
        path: path.clone(),
        done: true,
    });
    if let Some(p) = path {
        cx.background_spawn(async move {
            if let Err(e) = AppSettings::update(&p, |s| s.onboarded = true) {
                eprintln!("saving onboarded to {}: {e:#}", p.display());
            }
        })
        .detach();
    }
}
