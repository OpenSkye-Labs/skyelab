//! Looking at a turn keeps the files ticked for the working tree's commit: an empty selection means "commit all",
//! so a turn's file list must never clear it.

use super::changes_model::Scope;
use super::thread_checkpoints::key;
use super::thread_worktree::OpenChanges;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::TestAppContext;
use std::path::PathBuf;

#[gpui::test]
fn a_turn_scope_keeps_the_commit_selection(cx: &mut TestAppContext) {
    let f = fixture();
    std::fs::write(f.project.join(".fake-agent-edits"), "").expect("marker");
    std::fs::write(f.project.join("mine.txt"), "user\n").expect("w");
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
    wait_until(cx, "the working tree listed", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes.as_ref().is_some_and(|c| c.files.len() >= 2)
        })
    });
    let mine = PathBuf::from("mine.txt");
    changes.update(cx, |c, _| {
        c.selected.insert(mine.clone());
    });
    changes.update(cx, |c, cx| c.set_scope(Scope::Turn(1), cx));
    wait_until(cx, "turn 1 listed", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes
                .as_ref()
                .is_some_and(|c| c.files.iter().all(|f| f.path != mine))
        })
    });
    changes.update(cx, |c, cx| c.set_scope(Scope::Working, cx));
    wait_until(cx, "the working tree again", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes
                .as_ref()
                .is_some_and(|c| c.files.iter().any(|f| f.path == mine))
        })
    });
    changes.read_with(cx, |c, _| {
        assert!(
            c.selected.contains(&mine),
            "SELECTION_CLEARED: {:?}",
            c.selected
        )
    });
}
