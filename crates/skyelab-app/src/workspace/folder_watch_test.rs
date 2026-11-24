//! Live folder watching: an edit on disk reaches the footer, the details card and the Changes tab with no turn and
//! no focus change; a reload that finds nothing new draws nothing; one watch serves every view of a folder.

use std::time::Duration;

use gpui::{AppContext as _, Task, TestAppContext};

use super::folder_watch::{self, FolderWatches};
use super::thread::ThreadViewEvent;
use super::worktree_thread_test::{fixture, open_thread, wait_until};

#[gpui::test]
fn an_edit_on_disk_reaches_footer_details_and_changes(cx: &mut TestAppContext) {
    cx.update(folder_watch::init);
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    thread.update(cx, |_, cx| cx.emit(ThreadViewEvent::OpenChanges));
    cx.run_until_parked();
    let changes = ws
        .read_with(&*cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    wait_until(cx, "first loads", |cx| {
        let footer = thread.read_with(cx, |t, _| t.composer.footer.changes);
        let listed = changes.read_with(cx, |c, _| c.changes.as_ref().map(|c| c.files.len()));
        footer == Some(0) && listed == Some(0)
    });
    assert_eq!(
        cx.read(FolderWatches::live),
        1,
        "the thread and its Changes tab share one watch"
    );
    assert!(cx.read(|cx| FolderWatches::ready(&f.project, cx)), "watching");

    std::fs::write(f.project.join("new.txt"), "one\ntwo\n").expect("write");
    wait_until(cx, "the edit shown everywhere", |cx| {
        let (footer, stat) =
            thread.read_with(cx, |t, _| (t.composer.footer.changes, t.details.stat));
        let listed = changes.read_with(cx, |c, _| c.changes.as_ref().map(|c| c.files.len()));
        footer == Some(1) && stat == Some((2, 0)) && listed == Some(1)
    });
}

#[gpui::test]
fn a_reload_that_finds_nothing_new_draws_nothing(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    thread.update(cx, |_, cx| cx.emit(ThreadViewEvent::OpenChanges));
    cx.run_until_parked();
    let changes = ws
        .read_with(&*cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    wait_until(cx, "first loads", |cx| {
        thread.read_with(cx, |t, _| t.composer.footer.changes == Some(0))
            && changes.read_with(cx, |c, _| c.changes.is_some())
    });
    // Let startup work and the caret's idle timeout settle.
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    crate::debug_renders::take_test_count("Workspace");

    thread.update(cx, |t, cx| t.folder_changed(cx));
    changes.update(cx, |c, cx| c.reload_if_changed(cx));
    cx.run_until_parked();
    assert_eq!(
        crate::debug_renders::take_test_count("Workspace"),
        0,
        "nothing changed, so nothing redraws"
    );

    std::fs::write(f.project.join("new.txt"), "x\n").expect("write");
    thread.update(cx, |t, cx| t.folder_changed(cx));
    changes.update(cx, |c, cx| c.reload_if_changed(cx));
    wait_until(cx, "the change read", |cx| {
        changes.read_with(cx, |c, _| c.changes.as_ref().is_some_and(|c| c.files.len() == 1))
    });
    assert!(
        crate::debug_renders::take_test_count("Workspace") > 0,
        "a real change redraws"
    );
}

struct Probe(Option<Task<()>>);

#[gpui::test]
fn one_watch_per_folder_ended_with_its_last_view(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(folder_watch::init);
    let (a, b) = (fixture(), fixture());
    let probe = |dir: std::path::PathBuf, cx: &mut TestAppContext| {
        cx.new(|cx| Probe(folder_watch::watch(&dir, cx, |_: &mut Probe, _| {})))
    };
    let one = probe(a.project.clone(), cx);
    let two = probe(a.project.clone(), cx);
    assert_eq!(cx.read(FolderWatches::live), 1, "same folder, one watch");
    let other = probe(b.project.clone(), cx);
    assert_eq!(cx.read(FolderWatches::live), 2);
    drop(one);
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(cx.read(FolderWatches::live), 2, "a view still shows it");
    drop((two, other));
    cx.update(|_| {}); // releases the dropped entities
    cx.run_until_parked();
    assert_eq!(cx.read(FolderWatches::live), 0, "no view, no watch");
}

#[gpui::test]
fn off_without_the_global(cx: &mut TestAppContext) {
    let a = fixture();
    let p = cx.new(|cx| Probe(folder_watch::watch(&a.project, cx, |_: &mut Probe, _| {})));
    assert!(p.read_with(cx, |p, _| p.0.is_none()));
}

/// Only a folder watch skips a reload that finds the same diff: a scope switch with an equal diff still redraws
/// (its chip, a cleared rewind confirm, the new scope's comments).
#[cfg(unix)]
#[gpui::test]
fn a_scope_with_the_same_diff_still_redraws(cx: &mut TestAppContext) {
    use super::changes_model::Scope;
    use super::thread_checkpoints::key;
    use super::thread_worktree::OpenChanges;
    use super::worktree_thread_test::send;
    use std::{cell::Cell, rc::Rc};

    let f = fixture();
    std::fs::write(f.project.join(".fake-agent-edits"), "").expect("marker");
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "turn 1's checkpoint", |cx| {
        thread
            .read_with(cx, |t, _| t.thread.clone())
            .and_then(|t| skyelab_git::checkpoints(&t.cwd, &key(&t)).ok())
            .is_some_and(|c| c.len() == 2)
    });
    cx.dispatch_action(OpenChanges);
    let changes = ws
        .read_with(cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    changes.update(cx, |c, cx| c.set_scope(Scope::SinceStart, cx));
    cx.run_until_parked();
    let before = changes.read_with(cx, |c, _| c.changes.clone());
    let notified = Rc::new(Cell::new(0));
    let n = notified.clone();
    let _sub = cx.update(|_, cx| cx.observe(&changes, move |_, _| n.set(n.get() + 1)));
    changes.update(cx, |c, cx| c.set_scope(Scope::Turn(1), cx));
    cx.run_until_parked();
    let after = changes.read_with(cx, |c, _| c.changes.clone());
    assert_eq!(before, after, "the two scopes show the same diff");
    assert!(notified.get() > 0, "the scope switch redraws");
}
