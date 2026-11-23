//! Acceptance: every agent turn, run through the real ThreadView session with the fake agent editing a file, leaves
//! a checkpoint (0 before the first turn, then one per turn); Changes shows "Turn N" and "Since thread start"; and
//! the rewind confirm names every file it would change.

use super::changes_model::{self, Scope};
use super::thread::Stop;
use super::thread_checkpoints::key;
use super::thread_worktree::OpenChanges;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::TestAppContext;
use std::path::{Path, PathBuf};

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[gpui::test]
fn each_turn_leaves_a_checkpoint_and_changes_can_rewind_it(cx: &mut TestAppContext) {
    let f = fixture();
    std::fs::write(f.project.join(".fake-agent-edits"), "").expect("marker");
    let (ws, thread, cx) = open_thread(&f, cx);
    let count = |cx: &mut gpui::VisualTestContext| {
        thread
            .read_with(cx, |t, _| t.thread.clone())
            .and_then(|t| skyelab_git::checkpoints(&t.cwd, &key(&t)).ok())
            .map_or(0, |c| c.len())
    };
    let stop = |cx: &mut gpui::VisualTestContext| {
        thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
        wait_until(cx, "the agent to stop", |cx| {
            thread.read_with(cx, |t, _| t.session.is_none() && !t.starting)
        });
    };
    send(cx);
    wait_until(cx, "turn 1's checkpoint", |cx| count(cx) == 2);
    stop(cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    send(cx); // resumes: a new agent run, so a second turn
    wait_until(cx, "turn 2's checkpoint", |cx| count(cx) == 3);

    let row = thread.read_with(cx, |t, _| t.thread.clone().expect("row"));
    let k = key(&row);
    let list = skyelab_git::checkpoints(&row.cwd, &k).expect("list");
    assert_eq!(list.iter().map(|c| c.turn).collect::<Vec<_>>(), [0, 1, 2]);
    let refs = git_out(
        &f.project,
        &["for-each-ref", "--format=%(refname)", "refs/skyelab/"],
    );
    assert!(
        refs.lines()
            .all(|r| r.starts_with(&format!("refs/skyelab/checkpoints/{k}/"))),
        "{refs}"
    );
    assert!(
        git_out(&f.project, &["for-each-ref", "refs/heads"])
            .lines()
            .count()
            == 1,
        "no branch was added"
    );

    let turn2 = changes_model::load_scope(&row.cwd, None, Scope::Turn(2), Some(&k)).expect("turn");
    let paths: Vec<_> = turn2.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(paths, [PathBuf::from("agent-edit.txt")]);
    assert_eq!(turn2.files[0].kind, skyelab_git::ChangeKind::Modified);
    assert_eq!(changes_model::turns(&turn2.checkpoints), [1, 2]);
    let since =
        changes_model::load_scope(&row.cwd, None, Scope::SinceStart, Some(&k)).expect("all");
    assert_eq!(since.files.len(), 1);
    assert_eq!(since.files[0].kind, skyelab_git::ChangeKind::Added);
    assert_eq!(since.tree, None, "nothing in a turn scope is committable");

    // The confirm text lists exactly what happens to each file.
    let plan = skyelab_git::RewindPlan {
        restore: vec!["b.txt".into(), "a.txt".into()],
        remove: vec!["new.txt".into()],
        ignored: vec![".env".into()],
        ..Default::default()
    };
    assert_eq!(
        changes_model::rewind_message(&plan, false),
        "Restores a.txt, b.txt. Deletes new.txt. Leaves ignored .env."
    );
    assert_eq!(
        changes_model::rewind_message(&plan, true),
        "Restores a.txt, b.txt. Deletes new.txt. Overwrites ignored .env."
    );

    // Through the tab: refused while the agent runs, then rewinds turn 2 once stopped.
    cx.dispatch_action(OpenChanges);
    let changes = ws
        .read_with(cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    changes.update(cx, |c, cx| c.set_scope(Scope::Turn(2), cx));
    wait_until(cx, "turn 2 loaded", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes.as_ref().is_some_and(|c| c.files.len() == 1)
        })
    });
    if thread.read_with(cx, |t, _| t.session.is_some()) {
        changes.update(cx, |c, cx| c.ask_rewind(2, cx));
        cx.run_until_parked();
        changes.read_with(cx, |c, _| {
            assert!(c.rewind.is_none());
            assert!(c.publish_error.is_some(), "refused while the agent runs");
        });
        stop(cx);
    }
    changes.update(cx, |c, cx| c.ask_rewind(2, cx));
    wait_until(cx, "the rewind confirm", |cx| {
        changes.read_with(cx, |c, _| c.rewind.is_some())
    });
    let shown = changes.read_with(cx, |c, _| {
        let p = &c.rewind.as_ref().expect("pending").plan;
        changes_model::rewind_message(p, false)
    });
    assert_eq!(shown, "Restores agent-edit.txt.");
    changes.update(cx, |c, cx| c.confirm_rewind(cx));
    wait_until(cx, "the rewind", |cx| {
        changes.read_with(cx, |c, _| {
            c.publish_note.is_some() || c.publish_error.is_some()
        })
    });
    changes.read_with(cx, |c, _| assert_eq!(c.publish_error, None));
    assert_eq!(
        std::fs::read_to_string(f.project.join("agent-edit.txt")).expect("read"),
        "turn\n",
        "back to how turn 1 left it"
    );
}
