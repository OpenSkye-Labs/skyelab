//! Criterion 4: the depth cap counts across machines. A depth-2 caller is refused here, and a child started here
//! at depth 2 can't delegate again through this machine's own tools.

use super::orchestrate::{self, Orchestration};
use super::remote_delegation_harness::*;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Host as _, Proxy, Tool};
use skyelab_protocol::ErrorCode;

#[gpui::test]
fn depth_counts_the_callers_chain(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            ..Setup::default()
        },
        cx,
    );
    let mut deep = caller();
    deep.depth = 2;
    let before = b.thread_count();
    let e = call(&b, &deep, start_op(b.project("work-a"), "r1"), cx).expect_err("too deep");
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert!(e.message.contains("depth"), "{}", e.message);
    assert_eq!(b.thread_count(), before);

    let mut one = caller();
    one.depth = 1;
    let child = started(&b, &one, start_op(b.project("work-a"), "r2"), cx);
    wait_for_agent(&b, child, cx);
    let (store, config) = (b.store.clone(), b.ws.read_with(cx, |ws, _| ws.config.clone()));
    let run = b.root.join("run");
    cx.update(|_, cx| {
        let proxy = Proxy {
            program: "/usr/bin/false".into(),
            args: vec![],
        };
        orchestrate::install(cx, &run, store, config, proxy).expect("orchestration")
    });
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let tool = Tool::StartThread {
        project_id: b.project("work-b"),
        prompt: "go deeper".into(),
        worktree: false,
        request_id: None,
    };
    let me = Caller {
        thread_id: child,
        account_id: "work".into(),
    };
    let refused = std::thread::spawn(move || host.call(&me, tool)).join().expect("call");
    let why = refused.expect_err("the child is at the cap");
    assert!(why.contains("depth limit"), "{why}");
}
