//! Live folder watching for the views that show a folder's git state (the composer footer, the details card, the
//! Changes tab): one [`FolderWatch`] per folder however many views show it, ended when the last of them goes.
//!
//! Off unless [`FolderWatches`] is set as a global (the app sets it at launch; tests opt in), so the many gpui
//! tests that open threads on temp repos start no watcher threads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use gpui::{App, Context, Global, Task};
use skyelab_git::watch::{FolderWatch, Mode};

#[derive(Default)]
pub(crate) struct FolderWatches(HashMap<PathBuf, Weak<Shared>>);

impl Global for FolderWatches {}

impl FolderWatches {
    /// Folders watched now.
    #[cfg(test)]
    pub(crate) fn live(cx: &App) -> usize {
        cx.try_global::<Self>()
            .map_or(0, |w| w.0.values().filter(|s| s.strong_count() > 0).count())
    }

    /// Waits until the watch on `dir` is in place.
    #[cfg(test)]
    pub(crate) fn ready(dir: &Path, cx: &App) -> bool {
        let shared = cx
            .try_global::<Self>()
            .and_then(|w| w.0.get(dir)?.upgrade());
        shared.is_some_and(|s| s._watch.wait_ready(std::time::Duration::from_secs(60)))
    }
}

/// Watching turns on for the rest of the run.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(FolderWatches::default());
}

struct Shared {
    /// One slot per view: a full slot is a refresh already due, so a burst never queues twice.
    subs: Arc<Mutex<Vec<flume::Sender<()>>>>,
    _watch: FolderWatch,
}

/// Runs `on_change` on the view each time `dir` may have changed (debounced, off the UI thread until then), for as
/// long as the returned task lives. `None` when watching is off.
pub(crate) fn watch<T: 'static>(
    dir: &Path,
    cx: &mut Context<T>,
    on_change: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> Option<Task<()>> {
    let watches = cx.try_global::<FolderWatches>()?;
    let shared = watches.0.get(dir).and_then(Weak::upgrade);
    let shared = match shared {
        Some(s) => s,
        None => {
            let subs: Arc<Mutex<Vec<flume::Sender<()>>>> = Arc::default();
            let fan_out = subs.clone();
            let watch = FolderWatch::start(dir, Mode::native(), move || {
                let mut subs = fan_out.lock().unwrap_or_else(PoisonError::into_inner);
                subs.retain(|tx| {
                    !matches!(tx.try_send(()), Err(flume::TrySendError::Disconnected(_)))
                });
            });
            let s = Arc::new(Shared {
                subs,
                _watch: watch,
            });
            let map = &mut cx.global_mut::<FolderWatches>().0;
            map.retain(|_, w| w.strong_count() > 0);
            map.insert(dir.to_owned(), Arc::downgrade(&s));
            s
        }
    };
    let (tx, rx) = flume::bounded(1);
    shared
        .subs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(tx);
    Some(cx.spawn(async move |this, cx| {
        let _shared = shared;
        while rx.recv_async().await.is_ok() {
            if this.update(cx, |v, cx| on_change(v, cx)).is_err() {
                break;
            }
        }
    }))
}
