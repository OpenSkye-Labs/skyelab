//! A remote machine's last shell snapshot on disk (`skyelab_env::cache`): shown at launch while its connection is
//! still being made, saved a moment after live changes, deleted on any refusal, Forget or unlink.

use gpui::{App, AppContext as _, Context, Global};
use skyelab_core::AppSettings;
use skyelab_env::cache;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use super::{ConnState, RemoteEnv};

/// Where remote shells are cached (main.rs sets it). Unset, nothing is cached: tests never touch the real cache.
pub struct ShellCacheDir(pub PathBuf);

impl Global for ShellCacheDir {}

/// How long after a live change the rows are saved, once however many changes come meanwhile.
const SAVE_AFTER: Duration = Duration::from_secs(2);

/// Each host's delete count. A save or load that began under an older count is dropped, so a refused machine's rows
/// never come back from a save still in flight. Never held across disk I/O so UI callers never block on `sync_all`.
static EPOCHS: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Default::default);

/// Serializes cache saves and deletes on background threads so they cannot interleave out of order.
static IO: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn epochs() -> std::sync::MutexGuard<'static, HashMap<String, u64>> {
    // Only integers live here; a panic elsewhere cannot corrupt them.
    EPOCHS.lock().unwrap_or_else(|e| e.into_inner())
}

fn io_lock() -> std::sync::MutexGuard<'static, ()> {
    IO.lock().unwrap_or_else(|e| e.into_inner())
}

fn key(host: &str) -> String {
    host.trim().to_ascii_lowercase()
}

fn epoch(host: &str) -> u64 {
    epochs().get(&key(host)).copied().unwrap_or(0)
}

fn bump_epoch(host: &str) {
    *epochs().entry(key(host)).or_default() += 1;
}

fn dir(cx: &App) -> Option<PathBuf> {
    cx.try_global::<ShellCacheDir>().map(|d| d.0.clone())
}

fn save_blocking(dir: &Path, cached: &cache::Cached, began: u64) {
    let _io = io_lock();
    if epoch(&cached.host) == began {
        // A failed save only means the next launch waits for the connection.
        let _ = cache::save(dir, cached);
    }
}

/// Deletes `host`'s cached rows and turns away any save or load begun before. Bumps the epoch synchronously so
/// in-flight UI updates notice immediately, then deletes on a background thread.
pub fn forget_cache(host: &str, cx: &App) {
    bump_epoch(host);
    let Some(dir) = dir(cx) else {
        return;
    };
    let host = host.to_owned();
    cx.background_spawn(async move {
        let _io = io_lock();
        // A file left behind still loads only for the node it was saved from.
        let _ = cache::delete(&dir, &host);
    })
    .detach();
}

impl RemoteEnv {
    /// Shows the rows saved for this name, if they came from the node it is pinned to, until live ones arrive.
    pub(crate) fn load_cache(&mut self, cx: &mut Context<Self>) {
        let (Some(dir), Some(settings)) = (dir(cx), self.settings_path.clone()) else {
            return;
        };
        let host = self.host.clone();
        let began = epoch(&host);
        let read = cx.background_spawn(async move {
            let settings = AppSettings::load(&settings).ok()?;
            cache::load(&dir, &host, settings.environment_node(&host))
        });
        self._cache = Some(cx.spawn(async move |this, cx| {
            let Some(cached) = read.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                // Live rows came first, or the cache was deleted while it was read.
                if this.live || epoch(&this.host) != began {
                    return;
                }
                (this.projects, this.threads) = (cached.projects, cached.threads);
                this.keep_project();
                cx.notify();
            });
        }));
    }

    /// The rows shown are live: no cached ones replace them, and they are saved shortly.
    pub(super) fn got_live(&mut self, cx: &mut Context<Self>) {
        self.live = true;
        self._cache = None;
        if self._persist.is_some() || dir(cx).is_none() {
            return;
        }
        let timer = cx.background_executor().timer(SAVE_AFTER);
        self._persist = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                this._persist = None;
                this.save_cache(cx);
            });
        }));
    }

    /// Saves the rows of a connection checked against this name's pinned node; never an unpinned or refused one.
    fn save_cache(&self, cx: &mut Context<Self>) {
        let (Some(dir), Some(_), Some(node)) = (dir(cx), &self.settings_path, &self.trusted) else {
            return;
        };
        if self.state != ConnState::Connected {
            return;
        }
        let cached = cache::Cached::new(
            &self.host,
            node,
            self.projects.clone(),
            self.threads.clone(),
        );
        let began = epoch(&self.host);
        cx.background_spawn(async move { save_blocking(&dir, &cached, began) })
            .detach();
    }

    /// Forgets the rows shown and saved for this name: its connection isn't trusted now.
    pub(super) fn drop_cache(&mut self, cx: &mut Context<Self>) {
        self._cache = None;
        self._persist = None;
        forget_cache(&self.host, cx);
    }
}
