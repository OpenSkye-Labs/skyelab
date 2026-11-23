//! Acceptance 8: Settings → Scheduled tasks' controls do what they say, driven headlessly: pause, run now, edit and
//! save, Full access refused without its confirm, the thread link, and delete.

use super::super::center::Page;
use super::super::threads_test::wait_until;
use super::super::{Workspace, lock};
use super::page::{Pick, ScheduledPage};
use super::test_util::{NOW, Rig, open, read, rig, runs, task};
use crate::settings::Section;
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_agent::Mode;

fn page(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<ScheduledPage> {
    ws.update_in(cx, |ws, window, cx| {
        ws.show(Page::Settings(Section::Scheduled), window, cx)
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        ws.settings
            .as_ref()
            .and_then(|s| s.read(cx).scheduled.clone())
    })
    .expect("the page")
}

fn task_count(rig: &Rig) -> usize {
    lock(&rig.f.store)
        .expect("store")
        .scheduled_tasks()
        .expect("tasks")
        .len()
}

#[gpui::test]
fn the_controls_act_on_the_stored_task(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "daily 09:00", NOW + 1);
    let (ws, cx) = open(&rig, cx);
    let page = page(&ws, cx);
    assert_eq!(page.read_with(cx, |p, _| p.rows.len()), 1, "listed");

    page.update(cx, |p, cx| p.set_enabled(t.id, false, cx));
    assert!(!read(&rig.f.store, t.id).enabled, "paused");

    // Two quick clicks: the second finds the first still starting and is refused.
    page.update(cx, |p, cx| p.run_now(t.id, cx));
    page.update(cx, |p, cx| p.run_now(t.id, cx));
    wait_until(cx, "the run", |_| runs(&rig, t.id).len() == 1);
    cx.run_until_parked();
    assert_eq!(runs(&rig, t.id).len(), 1, "exactly one thread");
    let refused = page.read_with(cx, |p, _| p.error.clone());
    assert!(
        refused
            .expect("the second was refused")
            .contains("still going")
    );

    // The thread link opens the thread it made.
    let thread = runs(&rig, t.id)[0].id;
    page.update(cx, |p, cx| p.reload(cx));
    cx.run_until_parked();
    assert_eq!(t.id, 1, "the selector below names task 1");
    let link = cx
        .debug_bounds("sched-thread-1")
        .expect("the link is drawn");
    cx.simulate_click(link.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let shown = ws.read_with(cx, |ws, cx| match &ws.page {
        Page::Thread(v) => v
            .upgrade()
            .and_then(|v| v.read(cx).thread.as_ref().map(|t| t.id)),
        _ => None,
    });
    assert_eq!(shown, Some(thread));

    // Edit, change the prompt, save.
    let page = self::page(&ws, cx);
    page.update_in(cx, |p, window, cx| p.edit(t.id, window, cx));
    page.update_in(cx, |p, window, cx| {
        let e = p.editor.as_ref().expect("editor");
        e.prompt
            .update(cx, |i, cx| i.set_value("a new prompt", window, cx));
    });
    page.update(cx, |p, cx| p.save(cx));
    assert_eq!(read(&rig.f.store, t.id).prompt, "a new prompt");
    assert!(
        page.read_with(cx, |p, _| p.editor.is_none()),
        "closed on save"
    );

    // Delete takes two clicks.
    page.update(cx, |p, cx| p.delete(t.id, cx));
    assert_eq!(task_count(&rig), 1, "armed only");
    page.update(cx, |p, cx| p.delete(t.id, cx));
    assert_eq!(task_count(&rig), 0);
}

#[gpui::test]
fn full_access_needs_its_confirm_to_save(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (ws, cx) = open(&rig, cx);
    let page = page(&ws, cx);
    page.update_in(cx, |p, window, cx| {
        p.new_task(
            Some("nightly cleanup".into()),
            Some(rig.project),
            window,
            cx,
        );
        p.pick(Pick::Mode(Mode::Bypass), window, cx);
    });
    page.update(cx, |p, cx| p.save(cx));
    let why = page.read_with(cx, |p, _| p.editor.as_ref().and_then(|e| e.error.clone()));
    assert!(why.expect("refused").contains("Full access"));
    assert_eq!(task_count(&rig), 0, "nothing saved");

    page.update(cx, |p, cx| {
        p.update_draft(|d| d.bypass_confirmed = true, cx)
    });
    page.update(cx, |p, cx| p.save(cx));
    let saved = lock(&rig.f.store)
        .expect("store")
        .scheduled_tasks()
        .expect("tasks");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].permission_mode, "bypass");
    assert!(saved[0].bypass_confirmed);
    assert_eq!(
        saved[0].account_id, "acme",
        "saved under the folder's account"
    );
    assert!(
        saved[0].next_run_at.is_some_and(|n| n > NOW),
        "scheduled from now"
    );
}

#[gpui::test]
fn an_unbound_folder_cannot_get_a_task(cx: &mut TestAppContext) {
    let rig = rig(false);
    let (ws, cx) = open(&rig, cx);
    let page = page(&ws, cx);
    page.update_in(cx, |p, window, cx| {
        p.new_task(Some("hi".into()), Some(rig.project), window, cx)
    });
    page.update(cx, |p, cx| p.save(cx));
    let why = page.read_with(cx, |p, _| p.editor.as_ref().and_then(|e| e.error.clone()));
    assert!(why.expect("refused").contains("isn't bound"));
    assert_eq!(task_count(&rig), 0);
}
