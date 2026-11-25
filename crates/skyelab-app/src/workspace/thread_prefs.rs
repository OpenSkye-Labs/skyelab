//! Settings → Threads: the defaults new threads start with, from `settings.toml`, shared app-wide.

use gpui::{App, AppContext as _, Global, Task};
use skyelab_core::{
    AppSettings, DiffSettings, TerminalSettings, ThreadSettings, VisualReplySettings,
};
use std::path::PathBuf;

/// The loaded Threads settings and the file they save to.
pub(crate) struct ThreadPrefs {
    pub path: Option<PathBuf>,
    pub settings: ThreadSettings,
    /// The Changes tab's toggles, saved under `[diff]`.
    pub diff: DiffSettings,
    /// Settings → General's terminal switch, saved under `[terminal]`.
    pub terminal: TerminalSettings,
    /// Settings → Chat → Visual replies, saved under `[visual_replies]`.
    pub visual: VisualReplySettings,
}

impl Global for ThreadPrefs {}

/// Loads Settings → Threads from `path` (the app's `settings.toml`; a missing or unreadable file gives the defaults).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let loaded = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .unwrap_or_default();
    cx.set_global(super::super::saved_views::SidebarViews(
        loaded.sidebar_views,
    ));
    cx.set_global(ThreadPrefs {
        path,
        settings: loaded.threads,
        diff: loaded.diff,
        terminal: loaded.terminal,
        visual: loaded.visual_replies,
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

/// Applies `change` to the settings now, and to the file's `[threads]` as it is on disk, so an outside edit to
/// another field (a project table edited by hand) survives the save.
pub(crate) fn edit(
    change: impl Fn(&mut ThreadSettings) + Send + 'static,
    cx: &mut App,
) -> Task<anyhow::Result<()>> {
    let mut next = get(cx);
    change(&mut next);
    let path = path(cx);
    cx.set_global(ThreadPrefs {
        path: path.clone(),
        settings: next,
        diff: get_diff(cx),
        terminal: get_terminal(cx),
        visual: VisualReplySettings {
            allowed_hosts: page_hosts(cx),
        },
    });
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| change(&mut s.threads)).map(drop),
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
                terminal: get_terminal(cx),
                visual: VisualReplySettings {
                    allowed_hosts: page_hosts(cx),
                },
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

/// The terminal settings now (the defaults with none installed: confirmation on).
pub(crate) fn get_terminal(cx: &App) -> TerminalSettings {
    cx.try_global::<ThreadPrefs>()
        .map_or_else(TerminalSettings::default, |p| p.terminal)
}

/// Uses `terminal` from now on and saves it into the file as it is on disk (other settings untouched).
pub(crate) fn save_terminal(terminal: TerminalSettings, cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = path(cx);
    if cx.try_global::<ThreadPrefs>().is_none() {
        install(None, cx);
    }
    cx.global_mut::<ThreadPrefs>().terminal = terminal;
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.terminal = terminal).map(drop),
            None => Ok(()),
        }
    })
}

/// The hosts a visual reply's page may load scripts, styles and fonts from (the defaults with none installed). The
/// page checks each again.
pub(crate) fn page_hosts(cx: &App) -> Vec<String> {
    cx.try_global::<ThreadPrefs>().map_or_else(
        || VisualReplySettings::default().allowed_hosts,
        |p| p.visual.allowed_hosts.clone(),
    )
}

/// Uses `hosts` from now on (only a user's action calls this: Settings or a card's Allow) and saves it into the file
/// as it is on disk. Every window redraws, so cards shot under the old list are shot again.
pub(crate) fn save_page_hosts(hosts: Vec<String>, cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = path(cx);
    if cx.try_global::<ThreadPrefs>().is_none() {
        install(None, cx);
    }
    cx.global_mut::<ThreadPrefs>().visual.allowed_hosts = hosts.clone();
    cx.refresh_windows();
    cx.background_spawn(async move {
        match path {
            Some(p) => {
                AppSettings::update(&p, |s| s.visual_replies.allowed_hosts = hosts).map(drop)
            }
            None => Ok(()),
        }
    })
}
