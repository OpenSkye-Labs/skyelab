//! Applying a staged update: immediately when idle, or after active threads finish ("Update when threads finish").
//! While `background::is_busy(cx)` is true, Restart to update opens a choice dialog instead of killing in-flight work.

use super::{Step, UpdateStatus, Updates, set_step, shown};
use crate::ui::Button;
use crate::workspace::quit_guard::{self, Blocker};
use crate::workspace::{background, window_handles};
use gpui::{App, AppContext as _, Global, SharedString, Task, Window, prelude::*};
use gpui_component::{ActiveTheme as _, WindowExt as _, v_flex};
use skyelab_core::update_install::{self as install, Staged};

/// The most busy threads the confirmation dialog lists before "and N more".
const SHOWN: usize = 6;

#[cfg(all(test, unix))]
pub(crate) type Apply = std::sync::Arc<dyn Fn(&Staged) -> anyhow::Result<()> + Send + Sync>;

#[derive(Default)]
struct Defer {
    watcher: Option<Task<()>>,
    applying: bool,
    #[cfg(all(test, unix))]
    applier: Option<Apply>,
}

impl Global for Defer {}

#[cfg(all(test, unix))]
pub(crate) fn set_applier(applier: Option<Apply>, cx: &mut App) {
    cx.default_global::<Defer>().applier = applier;
}

/// Whether a staged update is armed to apply once active work finishes.
pub fn is_deferred(cx: &App) -> bool {
    matches!(
        cx.try_global::<Updates>().map(|u| &u.status),
        Some(UpdateStatus::Updating {
            step: Step::Deferred,
            ..
        })
    )
}

/// Restart to update: reveals a package in its folder, swaps and relaunches immediately when idle, or asks the user
/// to defer or force-restart when threads are active.
pub fn restart(cx: &mut App) {
    let Some((tag, _)) = shown(cx) else { return };
    match cx.try_global::<Updates>().and_then(|u| u.staged.as_ref()) {
        Some(Staged::Folder(file)) => {
            let file = file.clone();
            cx.reveal_path(&file);
            return;
        }
        Some(Staged::Swap { .. } | Staged::Installer { .. }) => {}
        None => return,
    }
    if !background::is_busy(cx) {
        restart_now(cx);
        return;
    }
    let items: Vec<_> = quit_guard::blockers(cx)
        .into_iter()
        .filter(|(_, b)| b.is_work())
        .collect();
    let handles = window_handles(cx);
    let target = cx
        .active_window()
        .filter(|w| handles.contains(w))
        .or_else(|| handles.first().copied());
    let Some(target) = target else {
        return; // busy with no visible window: never silently kill in-flight work
    };
    // Deferred: the popover's click runs inside its window's update, where that window can't be updated again.
    cx.defer(move |cx| {
        let _ = target.update(cx, |_, window, cx| open_busy_dialog(tag, items, window, cx));
    });
}

/// Arms the staged update to apply once active work has been idle for `background::IDLE_GRACE`. If nothing is busy
/// now, applies right away.
pub fn defer_until_idle(cx: &mut App) {
    let Some((tag, _)) = shown(cx) else { return };
    let can_swap = matches!(
        cx.try_global::<Updates>().and_then(|u| u.staged.as_ref()),
        Some(Staged::Swap { .. } | Staged::Installer { .. })
    );
    if !can_swap {
        return;
    }
    if !background::is_busy(cx) {
        restart_now(cx);
        return;
    }
    set_step(&tag, Step::Deferred, cx);
    let watcher = cx.spawn(async move |cx| {
        let mut idle_since = None;
        loop {
            cx.background_executor().timer(background::IDLE_POLL).await;
            let ready = cx.update(|cx| {
                if !is_deferred(cx) {
                    return None;
                }
                let now = cx.background_executor().now();
                if background::is_busy(cx) {
                    idle_since = None;
                    return Some(false);
                }
                Some(now - *idle_since.get_or_insert(now) >= background::IDLE_GRACE)
            });
            match ready {
                None => return,
                Some(false) => continue,
                Some(true) => {
                    cx.update(|cx| {
                        let _ = apply_if_deferred(cx);
                    });
                    return;
                }
            }
        }
    });
    cx.default_global::<Defer>().watcher = Some(watcher);
}

fn clear_watcher(cx: &mut App) {
    if cx.has_global::<Defer>() {
        cx.global_mut::<Defer>().watcher = None;
    }
}

/// Disarms a deferred update and returns the notice to Ready to restart.
pub fn cancel_deferred(cx: &mut App) {
    if cx.try_global::<Defer>().is_some_and(|d| d.applying) {
        return;
    }
    clear_watcher(cx);
    if is_deferred(cx)
        && let Some((tag, _)) = shown(cx)
    {
        set_step(&tag, Step::Ready, cx);
    }
}

/// Applies the staged update if deferred update is armed. Returns `true` when it took over quitting (for
/// `background.rs`'s idle quit), `false` otherwise.
pub fn apply_if_deferred(cx: &mut App) -> bool {
    if cx.try_global::<Defer>().is_some_and(|d| d.applying) {
        return true;
    }
    if !is_deferred(cx) {
        return false;
    }
    clear_watcher(cx);
    apply_now(cx)
}

/// Swaps the verified download in immediately, starts the relaunch and quits. A failed swap leaves the old app in
/// place and surfaces the error.
pub fn restart_now(cx: &mut App) {
    clear_watcher(cx);
    if let Some(Staged::Folder(file)) = cx.try_global::<Updates>().and_then(|u| u.staged.as_ref()) {
        let file = file.clone();
        cx.reveal_path(&file);
        return;
    }
    let _ = apply_now(cx);
}

fn apply_now(cx: &mut App) -> bool {
    let Some((tag, _)) = shown(cx) else {
        return false;
    };
    // Taken, so a second click or idle tick can't swap twice.
    let Some(staged) = cx.global_mut::<Updates>().staged.take() else {
        return false;
    };
    cx.default_global::<Defer>().applying = true;
    #[cfg(all(test, unix))]
    let applier = cx.try_global::<Defer>().and_then(|d| d.applier.clone());
    let job = cx.background_spawn(async move {
        #[cfg(all(test, unix))]
        if let Some(apply) = applier {
            return apply(&staged);
        }
        apply_real(&staged)
    });
    let task = cx.spawn(async move |cx| {
        let result = job.await;
        cx.update(|cx| match result {
            Ok(()) => quit_guard::quit_now(cx),
            Err(e) => {
                if cx.has_global::<Defer>() {
                    cx.global_mut::<Defer>().applying = false;
                }
                set_step(&tag, Step::Failed(format!("{e:#}")), cx);
            }
        });
    });
    cx.global_mut::<Updates>()._install = Some(task);
    true
}

fn apply_real(staged: &Staged) -> anyhow::Result<()> {
    install::recheck(staged)?;
    if let Staged::Swap { new, target, .. } = staged {
        install::swap(new, target)?;
    }
    install::finish(staged, std::process::id())
        .map_err(anyhow::Error::from)
        .map_err(|e| e.context("updated, but couldn't restart; quit and reopen SkyeLab"))
}

fn busy_summary(items: &[(SharedString, Blocker)]) -> SharedString {
    let working = items.iter().filter(|(_, b)| *b == Blocker::Working).count();
    let waiting = items
        .iter()
        .filter(|(_, b)| matches!(b, Blocker::Waiting(_)))
        .count();
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
    match parts.as_slice() {
        [] => "A scheduled task is starting or due shortly. Restarting now interrupts it.".into(),
        [one] => format!(
            "{one}. Restarting now stops {}.",
            if items.len() == 1 { "it" } else { "them" }
        )
        .into(),
        [a, b] => format!("{a} and {b}. Restarting now stops them.").into(),
        _ => "Threads are active. Restarting now stops them.".into(),
    }
}

fn restart_now_label(count: usize) -> SharedString {
    match count {
        0 => "Restart now".into(),
        1 => "Restart now (stops 1 thread)".into(),
        n => format!("Restart now (stops {n} threads)").into(),
    }
}

fn open_busy_dialog(
    tag: String,
    items: Vec<(SharedString, Blocker)>,
    window: &mut Window,
    cx: &mut App,
) {
    let version = skyelab_core::update::display_version(&tag).to_owned();
    let title: SharedString = format!("Restart to update to {version}?").into();
    let summary = busy_summary(&items);
    let now_label = restart_now_label(items.len());
    crate::ui::open_dialog(window, cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        let rows = items.iter().take(SHOWN).map(|(name, b)| {
            gpui::div()
                .text_color(muted)
                .child(format!("{name} · {}", quit_guard::label(*b)))
        });
        let more = items.len().saturating_sub(SHOWN);
        dialog
            .w(gpui::px(440.))
            .title(title.clone())
            .child(
                v_flex()
                    .gap_3()
                    .child(summary.clone())
                    .when(!items.is_empty(), |d| {
                        d.child(v_flex().gap_1().children(rows).when(more > 0, |d| {
                            d.child(
                                gpui::div()
                                    .text_color(muted)
                                    .child(format!("and {more} more")),
                            )
                        }))
                    }),
            )
            .footer(
                crate::ui::dialog_footer()
                    .child(
                        Button::new("update-busy-cancel")
                            .outline()
                            .small()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        gpui::div()
                            .debug_selector(|| "update-restart-now".into())
                            .child(
                                Button::new("update-busy-now")
                                    .danger()
                                    .outline()
                                    .small()
                                    .label(now_label.clone())
                                    .on_click(|_, window, cx| {
                                        window.close_dialog(cx);
                                        restart_now(cx);
                                    }),
                            ),
                    )
                    .child(
                        gpui::div()
                            .debug_selector(|| "update-when-idle".into())
                            .child(
                                Button::new("update-busy-defer")
                                    .primary()
                                    .small()
                                    .label("Update when threads finish")
                                    .on_click(|_, window, cx| {
                                        window.close_dialog(cx);
                                        defer_until_idle(cx);
                                    }),
                            ),
                    ),
            )
    });
}
