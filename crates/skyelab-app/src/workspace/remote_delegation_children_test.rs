//! Criterion 11: a call for a thread this parent didn't start on that machine sends nothing, and a start whose
//! child can't be recorded here stops it there.

use super::remote_delegation_a_harness::*;
use super::*;
use gpui::TestAppContext;
use skyelab_agent::mcp::Tool;

#[gpui::test]
fn a_thread_it_did_not_start_is_refused_without_a_call(cx: &mut TestAppContext) {
    let (a, cx) = a_side(true, cx);
    for tool in [
        Tool::ThreadStatus { thread_id: 900 },
        Tool::ReadThread {
            thread_id: 900,
            since_seq: 0,
        },
        Tool::SendMessage {
            thread_id: 900,
            text: "hi".into(),
        },
        Tool::WaitThread {
            thread_id: 900,
            timeout_s: 1,
        },
    ] {
        let e = on_b(&a, tool, cx).expect_err("not its child");
        assert!(e.contains("wasn't started by this thread"), "{e}");
    }
    assert!(delegate_calls(&a).is_empty(), "nothing was sent");
    known_child(&a, 900);
    assert!(on_b(&a, Tool::ThreadStatus { thread_id: 900 }, cx).is_ok());
}

#[gpui::test]
fn a_child_that_cant_be_recorded_is_stopped_there(cx: &mut TestAppContext) {
    let (a, cx) = a_side(true, cx);
    let raw = rusqlite::Connection::open(&a.db).expect("raw");
    raw.execute_batch(
        "CREATE TRIGGER no_children BEFORE INSERT ON remote_children BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
    )
    .expect("trigger");
    let e = on_b(&a, start(1), cx).expect_err("not recorded");
    assert!(e.contains("couldn't be recorded"), "{e}");
    assert!(e.contains("stopped there"), "{e}");
    let calls = delegate_calls(&a);
    assert!(calls.iter().any(|c| c.contains("\"start\"")), "{calls:?}");
    assert!(
        calls.iter().any(|c| c.contains("\"interrupt\"") && c.contains("900")),
        "{calls:?}"
    );
    assert!(lock(&a.store).and_then(|s| s.remote_children(a.parent)).expect("read").is_empty());
}
