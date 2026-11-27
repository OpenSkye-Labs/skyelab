//! A parent's busy remote children count against the cap for a local start too.

use super::orchestrate::Orchestration;
use super::remote_delegation_a_harness::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Host as _};

#[gpui::test]
fn a_local_start_counts_remote_children(cx: &mut TestAppContext) {
    let (a, cx) = a_side(true, cx);
    busy_local_children(&a, 3);
    known_child(&a, 900); // busy on B
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let caller = Caller {
        thread_id: a.parent,
        account_id: "work".into(),
    };
    let tool = start(a.project.id); // no `machine`: a local start
    let call = std::thread::spawn(move || host.call(&caller, tool));
    wait_until(cx, "the local start", |_| call.is_finished());
    let answer = call.join().expect("host call panicked");
    let e = answer.expect_err("3 local + 1 remote busy children: a 5th local start must be refused");
    assert!(e.contains("running children"), "{e}");
}
