//! Settings → Threads: the defaults new threads start with, from `settings.toml`, shared app-wide.

use gpui::{App, AppContext as _, Global, Task};
use skyelab_core::{AppSettings, DiffSettings, ThreadSettings};
use std::path::PathBuf;

/// The loaded Threads settings and the file they save to.
pub(crate) struct ThreadPrefs {
    pub path: Option<PathBuf>,
    pub settings: ThreadSettings,
    /// The Changes tab's toggles, saved under `[diff]`.
    pub diff: DiffSettings,
}

impl Global for ThreadPrefs {}

/// Loads Settings → Threads from `path` (the app's `settings.toml`; a missing or unreadable file gives the defaults).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let loaded = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .unwrap_or_default();
    cx.set_global(ThreadPrefs {
        path,
        settings: loaded.threads,
        diff: loaded.diff,
    });
}

/// The settings.toml every preference saves to (`None` in tests and on an OS with no config dir).
pub(crate) fn path(cx: &App) -> Option<PathBuf> {
    cx.try_global::<ThreadPrefs>().and_then(|p| p.path.clone())
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
        diff: get_diff(cx),
    });
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.threads = settings).map(drop),
            None => Ok(()),
        }
    })
}

/// The Changes tab's toggles now (the defaults with none installed).
pub(crate) fn get_diff(cx: &App) -> DiffSettings {
    cx.try_global::<ThreadPrefs>()
        .map_or_else(DiffSettings::default, |p| p.diff)
}

/// Uses `diff` from now on and saves it into the file as it is on disk (other settings untouched).
pub(crate) fn save_diff(diff: DiffSettings, cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = match cx.try_global::<ThreadPrefs>() {
        Some(p) => p.path.clone(),
        None => {
            cx.set_global(ThreadPrefs {
                path: None,
                settings: get(cx),
                diff,
            });
            return Task::ready(Ok(()));
        }
    };
    cx.global_mut::<ThreadPrefs>().diff = diff;
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.diff = diff).map(drop),
            None => Ok(()),
        }
    })
}
