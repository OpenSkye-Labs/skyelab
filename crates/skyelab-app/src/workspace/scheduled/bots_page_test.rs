//! The Bots page: the editor caps a GitHub-triggered bot at Plan or Ask, a starter prefills a bot, a card's history
//! lists its runs, and the Bots rail item counts paused bots.

use super::super::center::Page;
use super::super::{Workspace, lock};
use super::form::Kind;
use super::page::{Pick, ScheduledPage};
use super::test_util::{NOW, open, rebind, rig, task, tick};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_agent::Mode;
use skyelab_core::bots::Trigger;
use skyelab_store::scheduled::NewBotRun;

fn page(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<ScheduledPage> {
    ws.update_in(cx, |ws, window, cx| ws.show(Page::Bots, window, cx));
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        ws.bots.as_ref().map(|b| b.read(cx).list.clone())
    })
    .expect("the page")
}

#[gpui::test]
fn a_github_trigger_caps_the_editor_at_plan_or_ask(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (ws, cx) = open(&rig, cx);
    let page = page(&ws, cx);
    page.update_in(cx, |p, window, cx| {
        p.new_task(Some("look at it".into()), Some(rig.project), window, cx);
        p.pick(Pick::Mode(Mode::Bypass), window, cx);
        p.update_draft(|d| d.bypass_confirmed = true, cx);
        p.pick(Pick::Trigger(Trigger::PrOpened), window, cx);
    });
    let mode = page.read_with(cx, |p, _| p.editor.as_ref().map(|e| e.draft.mode));
    assert_eq!(
        mode,
        Some(Mode::Plan),
        "ticking a trigger drops Full access to Plan"
    );
    // Forced back (a stale menu): the save refuses it.
    page.update_in(cx, |p, window, cx| {
        p.pick(Pick::Mode(Mode::Bypass), window, cx)
    });
    page.update(cx, |p, cx| p.save(cx));
    let why = page.read_with(cx, |p, _| p.editor.as_ref().and_then(|e| e.error.clone()));
    assert!(why.expect("refused").contains("Plan or Ask"));
    // GitHub only, in Plan: saved with no schedule.
    page.update_in(cx, |p, window, cx| {
        p.pick(Pick::Mode(Mode::Plan), window, cx);
        p.pick(Pick::Kind(Kind::Never), window, cx);
    });
    page.update(cx, |p, cx| p.save(cx));
    let saved = lock(&rig.f.store)
        .expect("store")
        .scheduled_tasks()
        .expect("tasks");
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].schedule.as_str(), saved[0].triggers.as_str()),
        ("", "pr_opened")
    );
    assert_eq!(
        (saved[0].permission_mode.as_str(), saved[0].next_run_at),
        ("plan", None)
    );
}

#[gpui::test]
fn no_schedule_and_no_trigger_is_refused(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (ws, cx) = open(&rig, cx);
    let page = page(&ws, cx);
    page.update_in(cx, |p, window, cx| {
        p.new_task(Some("x".into()), Some(rig.project), window, cx);
        p.pick(Pick::Kind(Kind::Never), window, cx);
    });
    page.update(cx, |p, cx| p.save(cx));
    let why = page.read_with(cx, |p, _| p.editor.as_ref().and_then(|e| e.error.clone()));
    assert!(why.expect("refused").contains("schedule"));
}

#[gpui::test]
fn a_starter_prefills_a_plan_mode_bot_and_history_lists_runs(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (ws, cx) = open(&rig, cx);
    let page = page(&ws, cx);
    assert!(
        cx.debug_bounds("sched-empty").is_some(),
        "the empty state with its starters"
    );
    page.update_in(cx, |p, window, cx| {
        p.new_from("PR review", "Review it.", Trigger::PrOpened, window, cx)
    });
    let draft = page
        .read_with(cx, |p, _| p.editor.as_ref().map(|e| e.draft.clone()))
        .expect("open");
    assert_eq!(
        (draft.mode, draft.kind, draft.triggers),
        (Mode::Plan, Kind::Never, vec![Trigger::PrOpened])
    );
    page.update(cx, |p, cx| p.cancel(cx));

    let t = task(&rig, "daily 09:00", NOW + 1);
    let run = NewBotRun {
        task: t.id,
        at: NOW,
        trigger: "issue_opened",
        status: "rate_limited",
        ..Default::default()
    };
    lock(&rig.f.store)
        .expect("store")
        .add_bot_run(&run)
        .expect("run");
    page.update(cx, |p, cx| p.reload(cx));
    page.update(cx, |p, cx| p.toggle_history(t.id, cx));
    cx.run_until_parked();
    let runs = page
        .read_with(cx, |p, _| p.rows[0].runs.clone())
        .expect("expanded");
    assert_eq!(runs.len(), 1);
    let sel: &'static str = format!("bot-history-{}", t.id).leak();
    assert!(cx.debug_bounds(sel).is_some(), "drawn");
}

#[gpui::test]
fn the_rail_counts_paused_bots(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 1);
    let (ws, cx) = open(&rig, cx);
    let count = |cx: &mut VisualTestContext| ws.read_with(cx, |ws, cx| ws.bots_needing_you(cx));
    assert_eq!(count(cx), 0);
    rebind(&rig, Some("home"));
    tick(cx);
    cx.run_until_parked();
    assert_eq!(count(cx), 1, "paused by the rebind");
    assert!(
        cx.debug_bounds("rail-bots-count").is_some(),
        "the count is drawn"
    );
    rebind(&rig, Some("acme"));
    let page = page(&ws, cx);
    page.update(cx, |p, cx| p.set_enabled(t.id, true, cx));
    cx.run_until_parked();
    assert_eq!(count(cx), 0, "turned back on");
}
