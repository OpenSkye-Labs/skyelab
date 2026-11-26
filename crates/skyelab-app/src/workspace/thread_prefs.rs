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
    /// Dictation languages allowed to go to Apple's servers, saved as `dictation_server_locales`.
    pub dictation_locales: Vec<String>,
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
        dictation_locales: loaded.dictation_server_locales,
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

/// The permission mode a new thread in project folder `dir` starts in: the project's or the app's pick, else Full
/// access (T3's default). An unknown value reads as Ask.
pub(crate) fn new_thread_mode(
    settings: &ThreadSettings,
    dir: &std::path::Path,
) -> skyelab_agent::Mode {
    settings
        .for_project(dir)
        .permission_mode
        .as_deref()
        .map_or(skyelab_agent::Mode::Bypass, skyelab_agent::Mode::parse)
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
        dictation_locales: cx
            .try_global::<ThreadPrefs>()
            .map(|p| p.dictation_locales.clone())
            .unwrap_or_default(),
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
                dictation_locales: Vec::new(),
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

/// The user allowed dictation in `locale` to be recognized on Apple's servers.
pub(crate) fn allows_server_dictation(locale: &str, cx: &App) -> bool {
    cx.try_global::<ThreadPrefs>().is_some_and(|p| {
        p.dictation_locales
            .iter()
            .any(|l| skyelab_core::same_locale(l, locale))
    })
}

/// Saves `locale` as allowed for server dictation; this session counts it only once the file has it, so a failed save
/// asks again next time.
pub(crate) fn allow_server_dictation(locale: String, cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = path(cx);
    cx.spawn(async move |cx| -> anyhow::Result<()> {
        let saved = cx
            .background_spawn(async move {
                match path {
                    Some(p) => AppSettings::update(&p, |s| {
                        s.allow_server_dictation(&locale);
                        s.dictation_server_locales.clone()
                    })
                    .map(|(_, l)| l),
                    None => {
                        let mut s = AppSettings::default();
                        s.allow_server_dictation(&locale);
                        Ok(s.dictation_server_locales)
                    }
                }
            })
            .await?;
        cx.update(|cx| {
            if cx.try_global::<ThreadPrefs>().is_none() {
                install(None, cx);
            }
            let p = cx.global_mut::<ThreadPrefs>();
            for l in saved {
                if !p
                    .dictation_locales
                    .iter()
                    .any(|m| skyelab_core::same_locale(m, &l))
                {
                    p.dictation_locales.push(l);
                }
            }
        });
        Ok(())
    })
}

/// Restore defaults (Settings → Chat): every server-side dictation language asks again.
pub(crate) fn clear_server_dictation(cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = path(cx);
    if cx.has_global::<ThreadPrefs>() {
        cx.global_mut::<ThreadPrefs>().dictation_locales.clear();
    }
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.dictation_server_locales.clear()).map(drop),
            None => Ok(()),
        }
    })
}
