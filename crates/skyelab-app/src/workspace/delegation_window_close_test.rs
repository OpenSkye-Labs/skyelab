//! A delegated start whose handling window closes before the child's row exists: the call still gets its child,
//! and a retry with the same request id, now served by the other window, gets the same child, never a second one.

use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use super::orchestrate::{OrchCall, StartRequest};
use super::windows::{self, NewWindow, window_handles};
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn a_start_survives_its_window_closing(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (a, vcx) = open(&rig, accounts, config, cx);
    let parent = start(&a, rig.work_a, "plain work", vcx);
    let parent_id = thread_id(&parent, vcx);
    vcx.dispatch_action(NewWindow);
    vcx.run_until_parked();
    let handles = vcx.update(|_, cx| window_handles(cx));
    assert_eq!(handles.len(), 2, "a second window");
    let b = vcx.update(|_, cx| windows::workspaces(cx)[1].clone());
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };

    // A takes the start, then closes in the same turn: the child's row can't exist yet.
    let (reply, answer) = flume::bounded(1);
    let request = StartRequest {
        caller: caller(),
        depth: 0,
        project_id: rig.work_b,
        prompt: "hold".into(),
        worktree: false,
        request_id: Some("r1".into()),
    };
    a.update_in(vcx, |ws, window, cx| {
        ws.orchestrate(OrchCall::Start { request, reply }, window, cx);
        assert!(
            cx.try_global::<windows::Windows>().is_some(),
            "the registry is up"
        );
        window.remove_window();
    });
    let mut bcx = VisualTestContext::from_window(handles[1], cx);
    let mut got = None;
    wait_until(&mut bcx, "the first call's answer", |_| {
        if let Ok(r) = answer.try_recv() {
            got = Some(r);
        }
        got.is_some()
    });
    let first = got
        .expect("waited")
        .expect("the start answered with its child");
    assert!(
        bcx.update(|_, cx| b.read(cx).is_hub(cx)),
        "B serves the calls now"
    );

    // The retry reaches B, which never saw the first call.
    let again = host_call(
        &mut bcx,
        caller(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "hold".into(),
            worktree: false,
            request_id: Some("r1".into()),
        },
    )
    .expect("retry");
    assert_eq!(again["thread_id"].as_i64(), Some(first), "the same child");
    let kids = rig
        .store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids")
        .len();
    assert_eq!(kids, 1, "one child for one request id");
}
