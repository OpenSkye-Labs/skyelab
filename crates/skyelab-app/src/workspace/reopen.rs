//! Reopen Closed View (T3's ⌘⇧T): the right dock's closed tabs, latest first, per window. A tab records how to come
//! back when the dock drops it (`Panel::on_removed`, which a move between groups doesn't fire): the user's browser
//! tab by its page, the Preview tab by its file, a Changes tab by its thread. Terminals, side chats and agents'
//! browser tabs aren't reopened: their process or owner is gone.

use super::Workspace;
use super::thread::ThreadView;
use gpui::{AnyWindowHandle, App, AppContext as _, Context, Global, WeakEntity, Window, actions};
use skyelab_browser::BrowserView;
use std::path::PathBuf;

actions!(skyelab, [ReopenClosedView]);

/// The most closed tabs kept, across windows.
const MAX_CLOSED: usize = 20;

pub(crate) enum Closed {
    Browser(String),
    Preview { root: PathBuf, path: PathBuf },
    Changes(WeakEntity<ThreadView>),
}

#[derive(Default)]
struct ClosedViews(Vec<(AnyWindowHandle, Closed)>);

impl Global for ClosedViews {}

/// Records a tab `window`'s dock just dropped.
pub(crate) fn remember(window: &Window, closed: Closed, cx: &mut App) {
    let list = &mut cx.default_global::<ClosedViews>().0;
    list.push((window.window_handle(), closed));
    if list.len() > MAX_CLOSED {
        list.remove(0);
    }
}

/// How many closed tabs `window` can reopen.
#[cfg(test)]
pub(crate) fn pending(window: &Window, cx: &App) -> usize {
    let here = window.window_handle();
    cx.try_global::<ClosedViews>()
        .map_or(0, |c| c.0.iter().filter(|(w, _)| *w == here).count())
}

impl Workspace {
    /// Reopens this window's latest closed tab that can still come back (a Changes tab whose thread view is gone is
    /// skipped and forgotten).
    pub(super) fn reopen_closed_view(
        &mut self,
        _: &ReopenClosedView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A remote machine's view has no local tabs to reopen; keep the list for when it's local again.
        if self.is_remote() {
            return;
        }
        let here = window.window_handle();
        loop {
            let latest = cx
                .try_global::<ClosedViews>()
                .and_then(|c| c.0.iter().rposition(|(w, _)| *w == here));
            let Some(ix) = latest else { return };
            let (_, closed) = cx.global_mut::<ClosedViews>().0.remove(ix);
            match closed {
                Closed::Browser(url) => {
                    let browser = cx.new(|cx| BrowserView::new(Some(&url), window, cx));
                    self.add_browser(browser, None, window, cx);
                }
                Closed::Preview { root, path } => self.open_preview(root, path, None, window, cx),
                Closed::Changes(thread) => match thread.upgrade() {
                    Some(view) => {
                        self.open_changes(&view, window, cx);
                    }
                    None => continue,
                },
            }
            return;
        }
    }
}
