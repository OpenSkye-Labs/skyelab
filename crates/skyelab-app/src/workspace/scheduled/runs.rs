//! One run of a task (bot): started, skipped, refused or paused, and recorded in its history (`bot_runs`) as well as
//! in the task's last-run fields. A GitHub hit was queued with its framed prompt; it leaves the queue here.

use super::super::{SharedStore, Workspace, lock};
use super::{Refused, Scheduler, next_after_run};
use gpui::{AnyWindowHandle, App, Entity};
use skyelab_store::scheduled::{BotRun, NewBotRun, ScheduledTask};

/// What a run's thread is: the task, its history row, and whether GitHub text is in its prompt (no SkyeLab tools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunTag {
    pub task: i64,
    pub bot_run: Option<i64>,
    pub untrusted: bool,
}

/// What started a run.
pub(super) enum How {
    Schedule,
    RunNow,
    /// A queued GitHub hit.
    Hit(BotRun),
}

impl How {
    fn trigger(&self) -> &str {
        match self {
            How::Schedule => "schedule",
            How::RunNow => "run_now",
            How::Hit(run) => &run.trigger,
        }
    }
}

/// A history row for a run that didn't come from the queue.
pub(super) fn add_run(
    store: &SharedStore,
    task: i64,
    now: i64,
    trigger: &str,
    status: &str,
    error: Option<&str>,
) {
    let run = NewBotRun {
        task,
        at: now,
        trigger,
        status,
        error,
        ..Default::default()
    };
    if let Err(e) = lock(store).and_then(|s| s.add_bot_run(&run)) {
        eprintln!("scheduled task {task}: {e:#}");
    }
}

pub(super) fn skip(store: &SharedStore, task: &ScheduledTask, now: i64, why: &str) {
    let saved = lock(store).and_then(|s| {
        s.set_scheduled_next_run(task.id, next_after_run(task, now))?;
        s.record_scheduled_run(task.id, now, "skipped", Some(why))
    });
    if let Err(e) = saved {
        eprintln!("scheduled task {}: {e:#}", task.id);
    }
    add_run(store, task.id, now, "schedule", "skipped", Some(why));
}

/// Turns task `task` off until the user fixes it, with a notice on the hub window when one is open.
pub(super) fn pause(store: &SharedStore, task: &ScheduledTask, now: i64, why: &str, cx: &mut App) {
    if let Some((window, _)) = super::windows::hub_workspace(cx) {
        let _ = window.update(cx, |_, window, cx| {
            let note = crate::ui::toast::Toast::warning(format!(
                "Bot \"{}\" is paused: {why}",
                task.title
            ));
            crate::ui::toast::push(window, note, cx);
        });
    }
    let saved = lock(store).and_then(|s| {
        s.set_scheduled_task_enabled(task.id, false)?;
        s.record_scheduled_run(task.id, now, "paused", Some(why))
    });
    if let Err(e) = saved {
        eprintln!("scheduled task {}: {e:#}", task.id);
    }
    cx.default_global::<Scheduler>().changes += 1;
}

pub(super) fn start_run(
    task: &ScheduledTask,
    now: i64,
    how: How,
    window: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut App,
) {
    let store = ws.read(cx).backend.store.clone();
    let trigger = how.trigger().to_owned();
    let (run_id, prompt, untrusted) = match &how {
        How::Hit(run) => (Some(run.id), run.item_prompt.clone(), true),
        How::Schedule | How::RunNow => {
            let row = NewBotRun {
                task: task.id,
                at: now,
                trigger: &trigger,
                status: "started",
                ..Default::default()
            };
            let id = lock(&store).and_then(|s| s.add_bot_run(&row));
            (
                id.inspect_err(|e| eprintln!("scheduled task {}: {e:#}", task.id))
                    .ok(),
                Some(task.prompt.clone()),
                false,
            )
        }
    };
    let tag = RunTag {
        task: task.id,
        bot_run: run_id,
        untrusted,
    };
    let started = match prompt {
        // A queued hit whose prompt is gone never runs bare.
        None => Err(Refused::Fail("its GitHub item is gone".into())),
        Some(prompt) => window
            .update(cx, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.start_scheduled(task, tag, &prompt, window, cx)
                })
            })
            .unwrap_or_else(|_| Err(Refused::Fail("its window closed".into()))),
    };
    let (status, why) = match &started {
        Ok(view) => {
            let view = view.downgrade();
            cx.default_global::<Scheduler>()
                .launching
                .insert(task.id, view);
            ("started", None)
        }
        Err(Refused::Fail(why)) => ("failed", Some(why.as_str())),
        Err(Refused::Pause(why)) => ("paused", Some(why.as_str())),
    };
    if let Some(id) = run_id {
        let _ = lock(&store)
            .and_then(|s| s.update_bot_run(id, status, why))
            .inspect_err(|e| eprintln!("bot run {id}: {e:#}"));
    }
    if let Err(Refused::Pause(why)) = &started {
        return pause(&store, task, now, why, cx);
    }
    if let Err(e) = lock(&store).and_then(|s| s.record_scheduled_run(task.id, now, status, why)) {
        eprintln!("scheduled task {}: {e:#}", task.id);
    }
}
