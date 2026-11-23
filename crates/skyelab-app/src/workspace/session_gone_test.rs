//! A resumed session the agent no longer has: the thread says so and runs the prompt again in a new session, with
//! no second prompt row, no `--resume`, and the new session's id on the row.

use super::resume::{env_reports, reopen, send_text, stopped_thread};
use super::*;
use crate::workspace::claude_folded_turns_test::wait_until;
use crate::workspace::thread_model::SESSION_GONE_NOTE;

#[gpui::test]
fn a_gone_session_continues_in_a_new_one(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    // From now on the fake has no record of fake-sess-1.
    std::fs::write(f.project_dir.join(".fake-session-gone"), "").expect("marker");
    let view = reopen(&f, &row, cx);
    send_text(&view, "again", cx);
    wait_until(cx, "the second agent's env report", |_| {
        env_reports(&f.store, row.id).len() == 2
    });
    let fresh = env_reports(&f.store, row.id)[1]["args"]
        .as_str()
        .expect("args")
        .to_owned();
    assert!(!fresh.contains("--resume"), "started fresh: {fresh}");
    wait_until(cx, "the new session's id on the row", |_| {
        f.store
            .lock()
            .expect("lock")
            .thread(row.id)
            .expect("read")
            .and_then(|t| t.agent_session_id)
            .as_deref()
            == Some("fake-sess-new")
    });
    view.read_with(cx, |t, _| {
        let prompts = t
            .items
            .iter()
            .filter(|i| matches!(i, Item::Prompt(p) if p == "again"))
            .count();
        assert_eq!(prompts, 1, "the prompt shows once: {:?}", t.items);
        assert!(
            t.items
                .iter()
                .any(|i| matches!(i, Item::Notice(n) if n == SESSION_GONE_NOTE)),
            "the thread says it continued in a new session"
        );
        assert!(t.session.is_some(), "the new session runs");
    });
    let stored_prompts = stored_records(&f.store, row.id)
        .into_iter()
        .filter(|r| matches!(r, Record::Prompt(p) if p == "again"))
        .count();
    assert_eq!(stored_prompts, 1, "the prompt is stored once");
}
