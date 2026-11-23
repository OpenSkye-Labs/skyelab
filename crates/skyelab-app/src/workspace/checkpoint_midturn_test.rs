//! A prompt sent while a turn is still running doesn't overwrite that turn's starting checkpoint, and a turn
//! stopped before its result still gets an end checkpoint (so it can be reviewed and rewound).

use super::thread::Stop;
use super::thread_checkpoints::key;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::TestAppContext;

#[gpui::test]
fn a_mid_turn_prompt_keeps_the_baseline_and_a_stop_ends_the_turn(cx: &mut TestAppContext) {
    let f = fixture();
    for marker in [".fake-agent-edits", ".fake-agent-no-result"] {
        std::fs::write(f.project.join(marker), "").expect("marker");
    }
    let before = skyelab_git::snapshot_tree(&f.project).expect("snapshot");
    let (_ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "the agent's edit, mid-turn", |cx| {
        f.project.join("agent-edit.txt").exists()
            && thread.read_with(cx, |t, _| t.session.is_some())
    });
    let row = thread.read_with(cx, |t, _| t.thread.clone().expect("row"));
    let list = || skyelab_git::checkpoints(&row.cwd, &key(&row)).expect("list");

    // A follow-up steered into the running turn (a plain Send would wait in the composer's queue): no new baseline.
    thread.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("and this", window, cx));
        t.steer_prompt(&super::thread::composer::SteerPrompt, window, cx);
    });
    wait_until(cx, "the follow-up stored", |_| {
        let store = f.store.lock().expect("lock");
        store
            .events_since(row.id, 0)
            .expect("events")
            .iter()
            .filter(|e| e.kind == "steer")
            .count()
            == 1
    });
    let now = list();
    assert_eq!(now.len(), 1, "{now:?}");
    assert_eq!(now[0].tree, before, "turn 1's starting point is intact");

    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "the stopped turn's end checkpoint", |_| {
        list().len() == 2
    });
    let end = &list()[1];
    assert_ne!(
        end.tree, before,
        "the stopped turn's edit is in its end checkpoint"
    );
}
