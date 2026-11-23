//! A parent can't read, check or message its own child once the child's project is bound to another account:
//! `check_drive` holds on the connection thread (`drivable`) and again on the UI thread (`delegated_send`).
#![cfg(unix)]

use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use super::orchestrate;
use super::thread_model::Record;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};
use skyelab_core::Binding;

#[gpui::test]
fn a_child_rebound_to_another_account_is_out_of_reach(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config.clone(), cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    host_call(
        cx,
        caller(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "hold".into(),
            worktree: false,
        },
    )
    .expect("same-account child");
    let child = store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids")[0]
        .id;
    wait_until(cx, "the child running", |cx| {
        ws.read_with(cx, |ws, cx| ws.open_view(child, cx))
            .is_some_and(|v| v.read_with(cx, |v, _| v.session.is_some() && !v.starting))
    });
    host_call(cx, caller(), Tool::ThreadStatus { thread_id: child }).expect("reachable before");

    let work_b = std::fs::canonicalize(rig._home.path())
        .expect("canonicalize")
        .join("work-b");
    let mut rebound = config.clone();
    rebound.remove_binding(&work_b).expect("unbind");
    rebound
        .add_binding(Binding::new(work_b, "me".into()))
        .expect("rebind");

    // The connection thread's config sees the rebind.
    cx.update(|_, cx| orchestrate::set_config(cx, &rebound));
    for tool in [
        Tool::ThreadStatus { thread_id: child },
        Tool::ReadThread {
            thread_id: child,
            since_seq: 0,
        },
        Tool::SendMessage {
            thread_id: child,
            text: "hi from the old account".into(),
        },
    ] {
        let what = format!("{tool:?}");
        let err = host_call(cx, caller(), tool).expect_err(&what);
        assert!(err.contains("'me'"), "{what}: {err}");
    }

    // Only the UI's config sees it: the UI re-checks before sending.
    cx.update(|_, cx| orchestrate::set_config(cx, &config));
    ws.update(cx, |ws, _| ws.config = rebound.clone());
    let err = host_call(
        cx,
        caller(),
        Tool::SendMessage {
            thread_id: child,
            text: "hi from the old account".into(),
        },
    )
    .expect_err("the UI's check refuses");
    assert!(err.contains("'me'"), "{err}");

    let rows = store
        .lock()
        .expect("lock")
        .events_since(child, 0)
        .expect("events");
    let prompts: Vec<String> = rows
        .iter()
        .filter_map(|e| match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Prompt(p)) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, ["hold"], "the child got no message");
}
