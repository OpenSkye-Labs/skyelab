//! A deleted thread's checkpoint refs go when its row is purged (after the undo window), and stay through an undo
//! or an archive.

use super::manage::{ManageThread, ThreadOp, UNDO_WINDOW};
use super::manage_harness::rig;
use super::thread_checkpoints::key;
use gpui::{TestAppContext, VisualTestContext};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}");
}

fn act(id: i64, op: ThreadOp, cx: &mut VisualTestContext) {
    cx.dispatch_action(ManageThread { id, op });
    cx.run_until_parked();
}

#[gpui::test]
fn purge_drops_checkpoints_undo_and_archive_keep_them(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let dir = r.f.project_dir.clone();
    if !dir.join(".git").exists() {
        git(&dir, &["init", "-q", "-b", "main"]);
    }
    let refs = |id: i64| {
        let row = r.row(id).expect("row");
        skyelab_git::checkpoints(&dir, &key(&row))
            .expect("list")
            .len()
    };
    let checkpointed = |title: &str, cx: &mut VisualTestContext| {
        let id = r.thread(title, cx);
        let row = r.row(id).expect("row");
        for turn in 0..2 {
            skyelab_git::capture_checkpoint(&dir, &key(&row), turn).expect("capture");
        }
        (id, key(&row))
    };
    let (kept, _) = checkpointed("kept", cx);
    let (archived, _) = checkpointed("archived", cx);
    let (doomed, doomed_key) = checkpointed("doomed", cx);

    act(kept, ThreadOp::Delete, cx);
    r.ws.update_in(cx, |ws, window, cx| ws.undo_delete(kept, false, window, cx));
    cx.run_until_parked();
    act(archived, ThreadOp::Archive, cx);
    act(doomed, ThreadOp::Delete, cx);
    assert_eq!(refs(doomed), 2, "kept through the undo window");

    cx.executor().advance_clock(UNDO_WINDOW);
    cx.run_until_parked();
    assert!(r.row(doomed).is_none(), "purged");
    assert!(
        skyelab_git::checkpoints(&dir, &doomed_key)
            .expect("list")
            .is_empty(),
        "CHECKPOINTS_LEFT_AFTER_PURGE"
    );
    assert_eq!(refs(kept), 2, "an undone delete keeps them");
    assert_eq!(refs(archived), 2, "archive keeps them");
}
