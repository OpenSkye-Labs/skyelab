//! Scheduled tasks: a saved prompt that starts a new thread in a project on a schedule, while SkyeLab runs. One
//! scheduler for the whole app: a `Global` with one tick task, started by the first window that serves the app-wide
//! work, and each tick acts through that window (`windows::hub_workspace`), never one per window. The rules (when a
//! task is due, which account a run may use) live in `skyelab_core::schedule`; the tasks live in the store.

mod compose;
mod form;
mod page;
mod page_render;
mod start;
pub(crate) use page::ScheduledPage;
pub(super) use start::Refused;
#[cfg(test)]
mod account_test;
#[cfg(test)]
mod badge_test;
#[cfg(test)]
mod bypass_test;
#[cfg(test)]
mod guard_test;
#[cfg(test)]
mod missed_test;
#[cfg(test)]
mod no_window_test;
#[cfg(test)]
mod page_test;
#[cfg(test)]
mod palette_test;
#[cfg(test)]
mod review_test;
#[cfg(test)]
mod test_util;
#[cfg(test)]
mod windows_test;

use super::thread::ThreadView;
use super::{SharedStore, Workspace, lock, windows};
use chrono::{Local, TimeZone as _, Utc};
use gpui::{AnyWindowHandle, App, AppContext as _, Entity, Global, Task, WeakEntity};
use skyelab_core::schedule::{Due, MISSED_GRACE_MS, Schedule, due};
use skyelab_core::{AppSettings, ScheduledSettings, delegation::is_busy};
use skyelab_store::scheduled::ScheduledTask;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TICK: Duration = Duration::from_secs(15);
const MISSED: &str = "Missed while SkyeLab wasn't running; skipped (Settings → Scheduled tasks).";
const STILL_RUNNING: &str = "Its last run was still going, so this one was skipped.";

/// Opens a workspace window, as the dock does with none open.
pub(crate) type Opener = Rc<dyn Fn(&mut App)>;

#[derive(Default)]
pub(crate) struct Scheduler {
    settings: ScheduledSettings,
    settings_path: Option<PathBuf>,
    /// Tests set the clock. Otherwise the wall clock: a sleeping machine stops `Instant`-based timers.
    fake_now: Option<i64>,
    /// Runs started here whose row may not exist yet, by task.
    launching: HashMap<i64, WeakEntity<ThreadView>>,
    /// The last tick's time: a task can only have been missed if no tick ran for a while (start, or a wake).
    last_tick: Option<i64>,
    /// Bumped whenever a tick or a run changed a task, so an open task list reads the store again.
    pub(crate) changes: u64,
    /// The store the hub window served, so ticks still read the tasks when every window is closed (macOS).
    store: Option<SharedStore>,
    /// Opens a window (as the dock does) for a run that comes due while none is open.
    opener: Option<Opener>,
    _tick: Option<Task<()>>,
}

impl Global for Scheduler {}

/// Loads the scheduler's settings from `settings.toml` (a missing or unreadable file gives the defaults).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let settings = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .unwrap_or_default()
        .scheduled;
    let s = cx.default_global::<Scheduler>();
    s.settings = settings;
    s.settings_path = path;
}

/// How a tick opens a window when a run comes due with none open.
pub(crate) fn set_opener(open: Opener, cx: &mut App) {
    cx.default_global::<Scheduler>().opener = Some(open);
}

/// Starts the one tick loop on `store`; later calls (every window that claims the app-wide work) only keep the store.
pub(super) fn start(store: SharedStore, cx: &mut App) {
    let s = cx.default_global::<Scheduler>();
    s.store = Some(store);
    if s._tick.is_some() {
        return;
    }
    let task = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(TICK).await;
            cx.update(tick);
        }
    });
    cx.global_mut::<Scheduler>()._tick = Some(task);
}

pub(crate) fn now_ms(cx: &App) -> i64 {
    cx.try_global::<Scheduler>()
        .and_then(|s| s.fake_now)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        })
}

#[cfg(test)]
pub(super) fn set_now(ms: i64, cx: &mut App) {
    cx.default_global::<Scheduler>().fake_now = Some(ms);
}

pub(crate) fn settings(cx: &App) -> ScheduledSettings {
    cx.try_global::<Scheduler>()
        .map_or_else(ScheduledSettings::default, |s| s.settings)
}

/// Uses `settings` from now on and saves them into the file as it is on disk (other settings untouched).
pub(crate) fn save_settings(settings: ScheduledSettings, cx: &mut App) -> Task<anyhow::Result<()>> {
    let s = cx.default_global::<Scheduler>();
    s.settings = settings;
    let path = s.settings_path.clone();
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.scheduled = settings).map(drop),
            None => Ok(()),
        }
    })
}

/// The first run of `schedule` after `now_ms`, read in the machine's own time zone.
pub(crate) fn next_run(schedule: &str, now_ms: i64) -> Option<i64> {
    let schedule: Schedule = schedule.parse().ok()?;
    let now = Utc.timestamp_millis_opt(now_ms).single()?;
    Some(schedule.next_after(now, &Local)?.timestamp_millis())
}

/// The run after task `task`'s slot, handled at `now`: an interval keeps its beat, a wall time is read in the
/// machine's zone.
fn next_after_run(task: &ScheduledTask, now: i64) -> Option<i64> {
    let schedule: Schedule = task.schedule.parse().ok()?;
    let at = |ms: i64| Utc.timestamp_millis_opt(ms).single();
    let slot = at(task.next_run_at.unwrap_or(now))?;
    Some(
        schedule
            .next_after_run(slot, at(now)?, &Local)?
            .timestamp_millis(),
    )
}

/// Task `task`'s next run moved to the machine's zone now, when that changed since it was computed.
fn rezoned(task: &ScheduledTask, now: i64) -> Option<i64> {
    let schedule: Schedule = task.schedule.parse().ok()?;
    let at = |ms: i64| Utc.timestamp_millis_opt(ms).single();
    let moved = schedule.rezoned(at(task.next_run_at?)?, at(now)?, &Local)?;
    Some(moved.timestamp_millis())
}

/// The hub window, opening one first when none is (macOS keeps running with every window closed).
fn hub_or_open(cx: &mut App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    if let Some(hub) = windows::hub_workspace(cx) {
        return Some(hub);
    }
    let open = cx.try_global::<Scheduler>()?.opener.clone()?;
    open(cx);
    windows::hub_workspace(cx)
}

/// One pass over the tasks: runs the due ones, skips missed or overlapping ones, holds the rest for a free slot.
pub(super) fn tick(cx: &mut App) {
    let hub = windows::hub_workspace(cx);
    let store = match &hub {
        Some((_, ws)) => ws.read(cx).backend.store.clone(),
        None => match cx.try_global::<Scheduler>().and_then(|s| s.store.clone()) {
            Some(store) => store,
            None => return,
        },
    };
    let (now, settings) = (now_ms(cx), settings(cx));
    let read = lock(&store).and_then(|s| Ok((s.scheduled_tasks()?, s.last_scheduled_runs()?)));
    let (tasks, runs) = match read {
        Ok(read) => read,
        Err(e) => return eprintln!("scheduled tasks: {e:#}"),
    };
    let mut busy = launching(cx);
    busy.extend(runs.iter().filter(|r| is_busy(&r.2)).map(|r| r.0));
    // A task late only because the cap held it while ticks ran wasn't missed; it still gets the next free slot.
    let last = cx.default_global::<Scheduler>().last_tick.replace(now);
    let skip_missed = settings.skip_missed && last.is_none_or(|t| now - t > MISSED_GRACE_MS);
    // A hand-edited 0 would hold every task forever.
    let cap = settings.max_concurrent.max(1) as usize;
    let mut changed = false;
    let (mut hub, mut tried_open) = (hub, false);
    for task in tasks.iter().filter(|t| t.enabled) {
        // The machine's zone changed since its next run was computed: it moves, and isn't due on the old time.
        if let Some(moved) = rezoned(task, now) {
            if let Err(e) =
                lock(&store).and_then(|s| s.set_scheduled_next_run(task.id, Some(moved)))
            {
                eprintln!("scheduled task {}: {e:#}", task.id);
            }
            changed = true;
            continue;
        }
        match due(task.next_run_at, now, skip_missed) {
            Due::NotYet => continue,
            Due::SkipMissed => skip(&store, task, now, MISSED),
            Due::Run if busy.contains(&task.id) => skip(&store, task, now, STILL_RUNNING),
            // Waits, still due, until a run ends.
            Due::Run if busy.len() >= cap => continue,
            Due::Run => {
                // One try a tick: a window that can't open isn't retried for every due task.
                if hub.is_none() && !tried_open {
                    tried_open = true;
                    hub = hub_or_open(cx);
                }
                // No window could open: it stays due for the next tick.
                let Some((window, ws)) = hub.clone() else {
                    continue;
                };
                run(task, now, window, &ws, cx);
                busy.insert(task.id);
            }
        }
        changed = true;
    }
    if changed {
        cx.default_global::<Scheduler>().changes += 1;
    }
}

/// Runs task `id` now, outside its schedule (its next run stays). Refused while its last run is still going.
pub(super) fn run_now(id: i64, cx: &mut App) -> Result<(), String> {
    let (window, ws) = windows::hub_workspace(cx).ok_or("no window is open")?;
    let store = ws.read(cx).backend.store.clone();
    let read = lock(&store).and_then(|s| Ok((s.scheduled_task(id)?, s.last_scheduled_runs()?)));
    let (task, runs) = read.map_err(|e| format!("{e:#}"))?;
    let task = task.ok_or("the task was deleted")?;
    let running = runs.iter().any(|r| r.0 == id && is_busy(&r.2));
    if running || launching(cx).contains(&id) {
        return Err(STILL_RUNNING
            .replace(", so this one was skipped", "")
            .to_string());
    }
    let now = now_ms(cx);
    start_run(&task, now, window, &ws, cx);
    cx.default_global::<Scheduler>().changes += 1;
    Ok(())
}

/// Tasks whose run this app started and whose thread has no row yet (a worktree being made).
fn launching(cx: &mut App) -> HashSet<i64> {
    let s = cx.default_global::<Scheduler>();
    let views: Vec<_> = s.launching.drain().collect();
    let mut live = HashSet::new();
    for (task, view) in views {
        let Some(v) = view.upgrade() else {
            continue;
        };
        if v.read(cx).thread.is_some() {
            // Its row exists now: an open task list can link it.
            cx.default_global::<Scheduler>().changes += 1;
        } else if v.read(cx).starting {
            live.insert(task);
            cx.default_global::<Scheduler>()
                .launching
                .insert(task, view);
        }
    }
    live
}

fn skip(store: &SharedStore, task: &ScheduledTask, now: i64, why: &str) {
    let saved = lock(store).and_then(|s| {
        s.set_scheduled_next_run(task.id, next_after_run(task, now))?;
        s.record_scheduled_run(task.id, now, "skipped", Some(why))
    });
    if let Err(e) = saved {
        eprintln!("scheduled task {}: {e:#}", task.id);
    }
}

/// A scheduled run: its next slot is set first, so a run that fails or pauses never fires again every tick.
fn run(
    task: &ScheduledTask,
    now: i64,
    window: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut App,
) {
    let store = ws.read(cx).backend.store.clone();
    let next = next_after_run(task, now);
    if let Err(e) = lock(&store).and_then(|s| s.set_scheduled_next_run(task.id, next)) {
        return eprintln!("scheduled task {}: {e:#}", task.id);
    }
    start_run(task, now, window, ws, cx);
}

fn start_run(
    task: &ScheduledTask,
    now: i64,
    window: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut App,
) {
    let started = window
        .update(cx, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.start_scheduled(task, window, cx))
        })
        .unwrap_or_else(|_| Err(Refused::Fail("its window closed".into())));
    let store = ws.read(cx).backend.store.clone();
    let saved = match &started {
        Ok(view) => {
            let view = view.downgrade();
            cx.default_global::<Scheduler>()
                .launching
                .insert(task.id, view);
            lock(&store).and_then(|s| s.record_scheduled_run(task.id, now, "started", None))
        }
        Err(Refused::Fail(why)) => {
            lock(&store).and_then(|s| s.record_scheduled_run(task.id, now, "failed", Some(why)))
        }
        Err(Refused::Pause(why)) => {
            let _ = window.update(cx, |_, window, cx| {
                let note = gpui_component::notification::Notification::warning(format!(
                    "Scheduled task \"{}\" is paused: {why}",
                    task.title
                ));
                super::manage::push_note(window, note, cx);
            });
            lock(&store).and_then(|s| {
                s.set_scheduled_task_enabled(task.id, false)?;
                s.record_scheduled_run(task.id, now, "paused", Some(why))
            })
        }
    };
    if let Err(e) = saved {
        eprintln!("scheduled task {}: {e:#}", task.id);
    }
}
