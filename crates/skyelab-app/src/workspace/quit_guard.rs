//! Quitting while work is in flight: ⌘Q, the Quit menu and (off macOS) closing the last window ask first when a
//! thread or side chat is working or waiting on the user, a thread holds diff comments or context chips not sent yet
//! (kept in memory only), or a terminal runs a command, since quitting stops or drops them all. A second
//! ⌘Q while the dialog is open quits; "Don't ask again" (Settings → General) quits at once from then on.

use super::panels::Ask;
use super::windows::{Windows, window_handles};
use crate::ui::{Button, Checkbox};
use gpui::{AnyWindowHandle, App, Global, SharedString, Window, prelude::*};
use gpui_component::{ActiveTheme as _, WindowExt as _, v_flex};
use skyelab_core::AppSettings;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

/// The quit setting and the settings.toml it saves to.
pub(crate) struct QuitPrefs {
    pub path: Option<PathBuf>,
    pub ask: bool,
}

impl Global for QuitPrefs {}

/// The window whose quit dialog is open, if one is.
#[derive(Default)]
struct Asking(Option<AnyWindowHandle>);

impl Global for Asking {}

/// Loads the quit setting from `path` (missing or unreadable: ask).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let quiet = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .is_some_and(|s| s.quit_without_asking);
    cx.set_global(QuitPrefs { path, ask: !quiet });
}

/// Whether quitting asks first (Settings → General); on with none installed.
pub(crate) fn asks(cx: &App) -> bool {
    cx.try_global::<QuitPrefs>().is_none_or(|p| p.ask)
}

/// Uses `ask` from now on and saves it into settings.toml as it is on disk.
pub(crate) fn set_asks(ask: bool, cx: &mut App) -> anyhow::Result<()> {
    let path = cx.try_global::<QuitPrefs>().and_then(|p| p.path.clone());
    cx.set_global(QuitPrefs {
        path: path.clone(),
        ask,
    });
    match path {
        Some(p) => AppSettings::update(&p, |s| s.quit_without_asking = !ask).map(drop),
        None => Ok(()),
    }
}

/// What a quit now would stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Blocker {
    Working,
    Waiting(Ask),
    /// Diff comments (saved or being written) or context chips not sent yet: memory only, so a quit drops them.
    Unsent,
    Terminal,
}

/// What quitting would stop or drop in thread `v`; `drafting`: a diff comment is being written for it.
fn thread_blocker(v: &super::thread::ThreadView, drafting: bool) -> Option<Blocker> {
    let state = super::thread_rows::RowState {
        status: v.live_status(),
        ask: super::thread_rows::waiting_ask(v),
        started_ms: None,
    };
    if let Some(ask) = state.ask {
        Some(Blocker::Waiting(ask))
    } else if state.running() {
        Some(Blocker::Working)
    } else if drafting || !v.composer.review.items.is_empty() || !v.composer.contexts.is_empty() {
        Some(Blocker::Unsent)
    } else {
        None
    }
}

/// Every local thread and side chat that works, waits on the user or holds something unsent, and every terminal
/// running a command, with its name.
pub(crate) fn blockers(cx: &App) -> Vec<(SharedString, Blocker)> {
    let Some(reg) = cx.try_global::<Windows>() else {
        return Vec::new();
    };
    let workspaces: Vec<_> = reg
        .workspaces
        .iter()
        .filter_map(|r| r.workspace.upgrade())
        .collect();
    // Threads with a diff comment being written in a Changes tab (its text typed, not saved yet).
    let drafting: Vec<_> = workspaces
        .iter()
        .flat_map(|ws| ws.read(cx).changes_views.iter().filter_map(|c| c.upgrade()))
        .filter_map(|c| {
            let c = c.read(cx);
            let typed = !c.draft.as_ref()?.input.read(cx).value().trim().is_empty();
            typed.then_some(c.thread.as_ref()?.entity_id())
        })
        .collect();
    let sides = workspaces.iter().flat_map(|ws| {
        ws.read(cx)
            .side_chats
            .iter()
            .map(|p| p.read(cx).view.clone())
    });
    let mut out = Vec::new();
    for view in reg.threads.iter().cloned().chain(sides) {
        let v = view.read(cx);
        if let Some(b) = thread_blocker(v, drafting.contains(&view.entity_id())) {
            out.push((v.title(), b));
        }
    }
    for ws in &workspaces {
        for panel in &ws.read(cx).terminals {
            let p = panel.read(cx);
            let ids: Vec<_> = p.panes.iter().map(|p| p.view.entity_id()).collect();
            out.extend(p.busy(&ids, cx).into_iter().map(|n| (n, Blocker::Terminal)));
        }
    }
    out
}

/// The dialog's title and summary line for `items` (each part starts with its count).
pub(crate) fn question(items: &[(SharedString, Blocker)]) -> (SharedString, SharedString) {
    let count = |f: fn(&Blocker) -> bool| items.iter().filter(|(_, b)| f(b)).count();
    let working = count(|b| *b == Blocker::Working);
    let waiting = count(|b| matches!(b, Blocker::Waiting(_)));
    let unsent = count(|b| *b == Blocker::Unsent);
    let terminals = count(|b| *b == Blocker::Terminal);
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let mut parts = Vec::new();
    if working > 0 {
        parts.push(plural(working, "thread is working", "threads are working"));
    }
    if waiting > 0 {
        parts.push(plural(
            waiting,
            "thread is waiting for you",
            "threads are waiting for you",
        ));
    }
    if unsent > 0 {
        parts.push(plural(
            unsent,
            "thread has unsent comments or context",
            "threads have unsent comments or context",
        ));
    }
    if terminals > 0 {
        parts.push(plural(
            terminals,
            "terminal is running a command",
            "terminals are running commands",
        ));
    }
    let list = match parts.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    };
    (
        "Quit SkyeLab?".into(),
        format!(
            "{list}. Quitting {} {}.",
            if unsent == items.len() {
                "loses"
            } else {
                "stops"
            },
            if items.len() == 1 { "it" } else { "them" }
        )
        .into(),
    )
}

/// The row label a blocker shows next to its name.
fn label(b: Blocker) -> &'static str {
    match b {
        Blocker::Working => "Working",
        Blocker::Waiting(ask) => ask.label(),
        Blocker::Unsent => "Not sent",
        Blocker::Terminal => "Terminal",
    }
}

/// The most rows the dialog lists; the rest are "and N more".
const SHOWN: usize = 6;

/// ⌘Q and the Quit menu: quit at once, or ask when something would stop. A quit asked while the dialog is open
/// (a second ⌘Q) quits.
pub fn request_quit(cx: &mut App) {
    if asking(cx) {
        return quit_now(cx);
    }
    let items = blockers(cx);
    if items.is_empty() || !asks(cx) {
        return quit_now(cx);
    }
    // macOS keeps running with every window closed, agents included: a window opens to ask in.
    if window_handles(cx).is_empty()
        && let Some(open) = cx.try_global::<crate::FreshWindow>().map(|f| f.0.clone())
    {
        open(cx);
    }
    let handles = window_handles(cx);
    let target = cx
        .active_window()
        .filter(|w| handles.contains(w))
        .or_else(|| handles.first().copied());
    let Some(target) = target else {
        return quit_now(cx);
    };
    // Deferred: ⌘Q runs inside its window's update, where that window can't be updated again.
    cx.defer(move |cx| {
        let _ = target.update(cx, |_, window, cx| open(items, window, cx));
    });
}

/// Off macOS, closing the last window quits: it asks the same way and keeps the window while the dialog is open.
pub(super) fn may_close(window: &mut Window, cx: &mut App) -> bool {
    let handle = window.window_handle();
    let last = window_handles(cx).iter().all(|w| *w == handle);
    if !last || asking(cx) || !asks(cx) {
        return true;
    }
    let items = blockers(cx);
    if items.is_empty() {
        return true;
    }
    open(items, window, cx);
    false
}

/// The quit dialog is open now.
fn asking(cx: &mut App) -> bool {
    let Some(window) = cx.try_global::<Asking>().and_then(|a| a.0) else {
        return false;
    };
    // Inside that window's own update (⌘Q there) it can't be read: the flag stands, as the dialog's close clears it.
    let open = cx.windows().contains(&window)
        && window
            .update(cx, |_, window, cx| window.has_active_dialog(cx))
            .unwrap_or(true);
    if !open {
        cx.set_global(Asking(None));
    }
    open
}

fn open(items: Vec<(SharedString, Blocker)>, window: &mut Window, cx: &mut App) {
    let (title, summary) = question(&items);
    cx.set_global(Asking(Some(window.window_handle())));
    let quiet = Rc::new(Cell::new(false));
    crate::ui::open_dialog(window, cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        let rows = items.iter().take(SHOWN).map(|(name, b)| {
            gpui::div()
                .text_color(muted)
                .child(format!("{name} · {}", label(*b)))
        });
        let more = items.len().saturating_sub(SHOWN);
        let quiet_set = quiet.clone();
        let quiet_get = quiet.clone();
        dialog
            .w(gpui::px(440.))
            .title(title.clone())
            .on_close(|_, _, cx| cx.set_global(Asking(None)))
            .child(
                v_flex()
                    .gap_3()
                    .child(summary.clone())
                    .child(v_flex().gap_1().children(rows).when(more > 0, |d| {
                        d.child(
                            gpui::div()
                                .text_color(muted)
                                .child(format!("and {more} more")),
                        )
                    }))
                    .child(
                        Checkbox::new("quit-dont-ask")
                            .checked(quiet.get())
                            .label("Don't ask again")
                            .on_click(move |on, window, _| {
                                quiet_set.set(*on);
                                window.refresh();
                            }),
                    ),
            )
            .footer(
                crate::ui::dialog_footer()
                    .child(
                        Button::new("quit-cancel")
                            .outline()
                            .small()
                            .label("Cancel")
                            .on_click(|_, window, cx| {
                                cx.set_global(Asking(None));
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("quit-confirm")
                            .danger()
                            .small()
                            .label("Quit")
                            .on_click(move |_, _, cx| {
                                if quiet_get.get()
                                    && let Err(e) = set_asks(false, cx)
                                {
                                    eprintln!("skyelab: couldn't save the quit setting: {e:#}");
                                }
                                quit_now(cx);
                            }),
                    ),
            )
    });
}

/// Quits: every view's quit hook saves what it holds (layout, drafts, stopped rows) on the way out.
fn quit_now(cx: &mut App) {
    #[cfg(test)]
    {
        cx.default_global::<TestQuits>().0 += 1;
    }
    cx.quit();
}

/// Tests: how many times the app quit (the test platform's quit does nothing).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestQuits(pub usize);

#[cfg(test)]
impl Global for TestQuits {}
