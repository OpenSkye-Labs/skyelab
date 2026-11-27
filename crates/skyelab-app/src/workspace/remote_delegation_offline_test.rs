//! Criterion 12: with B down a start fails at once, a wait answers unreachable at its deadline, and the children
//! cap counts remote children (an unreachable one as busy) with local ones.

use super::remote_delegation_a_harness::*;
use crate::test_wait::wait_until;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::Tool;
use std::time::{Duration, Instant};

fn down(a: &mut A, cx: &mut VisualTestContext) {
    a.b.stop();
    wait_until(cx, "B offline", |cx| {
        a.ws.read_with(cx, |ws, cx| ws.linked_envs().iter().all(|e| e.read(cx).node().is_none()))
    });
}

#[gpui::test]
fn a_start_fails_at_once_and_a_wait_says_unreachable(cx: &mut TestAppContext) {
    let (mut a, cx) = a_side(true, cx);
    known_child(&a, 900);
    down(&mut a, cx);
    let began = Instant::now();
    let e = on_b(&a, start(1), cx).expect_err("offline");
    assert!(began.elapsed() < Duration::from_secs(1), "{:?}", began.elapsed());
    assert!(e.contains("isn't connected"), "{e}");
    let waited = on_b(
        &a,
        Tool::WaitThread {
            thread_id: 900,
            timeout_s: 2,
        },
        cx,
    )
    .expect("an answer");
    assert_eq!(waited["done"], false);
    assert_eq!(waited["status"], "unreachable");
}

#[gpui::test]
fn remote_children_count_against_the_cap(cx: &mut TestAppContext) {
    let (a, cx) = a_side(true, cx);
    busy_local_children(&a, 3);
    known_child(&a, 900); // busy there
    let e = on_b(&a, start(1), cx).expect_err("3 local + 1 remote");
    assert!(e.contains("running children"), "{e}");
    assert!(!delegate_calls(&a).iter().any(|c| c.contains("\"start\"")));
}

#[gpui::test]
fn an_unreachable_remote_child_counts_as_busy(cx: &mut TestAppContext) {
    let (mut a, cx) = a_side(true, cx);
    busy_local_children(&a, 3);
    known_child(&a, 900);
    down(&mut a, cx);
    let e = on_b(&a, start(1), cx).expect_err("capped");
    assert!(e.contains("running children"), "{e}");
}
