//! Settings → General's update channel: switching drops the old channel's offer and check, checks again on the new
//! one, and saves the choice.

use super::{UpdateStatus, Updates, check_with, set_status};
use gpui::{App, AppContext as _, Task};
use skyelab_core::AppSettings;
use skyelab_core::update::Channel;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// Switches to `channel`. A check still running on the old channel is dropped, so its answer can't land as an offer
/// the new channel wouldn't make; with the update check on, the new channel is checked at once (a version skipped
/// there stays hidden).
pub fn set_channel(channel: Channel, cx: &mut App) -> Task<anyhow::Result<()>> {
    switch(channel, Some(channel), cx)
}

/// Restore defaults: the build's own channel ([`Channel::of_build`]), and settings.toml's choice unset so it keeps
/// following the build.
pub fn restore_channel(cx: &mut App) -> Task<anyhow::Result<()>> {
    let Some(u) = cx.try_global::<Updates>() else {
        return Task::ready(Ok(()));
    };
    switch(Channel::of_build(&u.current), None, cx)
}

/// Shows `channel` and saves `saved` as settings.toml's choice.
fn switch(channel: Channel, saved: Option<Channel>, cx: &mut App) -> Task<anyhow::Result<()>> {
    let Some(u) = cx.try_global::<Updates>() else {
        return Task::ready(Ok(()));
    };
    let (path, enabled, changed) = (u.path.clone(), u.enabled, u.channel != channel);
    if changed {
        let u = cx.global_mut::<Updates>();
        u.channel = channel;
        if matches!(u.status, UpdateStatus::Checking) {
            u._check = None;
        }
        if matches!(
            u.status,
            UpdateStatus::Checking | UpdateStatus::Available { .. }
        ) {
            set_status(UpdateStatus::Idle, cx);
        }
        if enabled {
            check_with(true, true, cx);
        }
    }
    // Two quick clicks save in click order: a save that is no longer the newest skips itself under the lock.
    static NEWEST: AtomicU64 = AtomicU64::new(0);
    static SAVING: Mutex<()> = Mutex::new(());
    let mine = NEWEST.fetch_add(1, Ordering::SeqCst) + 1;
    cx.background_spawn(async move {
        let _held = SAVING.lock().unwrap_or_else(PoisonError::into_inner);
        match path {
            Some(p) if NEWEST.load(Ordering::SeqCst) == mine => {
                AppSettings::update(&p, |s| s.updates.channel = saved).map(drop)
            }
            _ => Ok(()),
        }
    })
}
