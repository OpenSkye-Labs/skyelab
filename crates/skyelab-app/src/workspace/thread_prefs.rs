//! Settings → Threads: the defaults new threads start with, from `settings.toml`, shared app-wide.

use gpui::{App, AppContext as _, Global, Task};
use skyelab_core::{AppSettings, ThreadSettings};
use std::path::PathBuf;

/// The loaded Threads settings and the file they save to.
pub(crate) struct ThreadPrefs {
    pub path: Option<PathBuf>,
    pub settings: ThreadSettings,
}

impl Global for ThreadPrefs {}

/// Loads Settings → Threads from `path` (the app's `settings.toml`; a missing or unreadable file gives the defaults).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let settings = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .map(|s| s.threads)
        .unwrap_or_default();
    cx.set_global(ThreadPrefs { path, settings });
}

/// The settings now. With none installed (a test window), titles are off: no setting turned them on.
pub(crate) fn get(cx: &App) -> ThreadSettings {
    cx.try_global::<ThreadPrefs>().map_or_else(
        || ThreadSettings {
            generate_titles: false,
            ..Default::default()
        },
        |p| p.settings.clone(),
    )
}

/// Uses `settings` from now on and saves them into the file as it is on disk (other settings untouched).
pub(crate) fn save(settings: ThreadSettings, cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = cx.try_global::<ThreadPrefs>().and_then(|p| p.path.clone());
    cx.set_global(ThreadPrefs {
        path: path.clone(),
        settings: settings.clone(),
    });
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.threads = settings).map(drop),
            None => Ok(()),
        }
    })
}
