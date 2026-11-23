//! Delegated sends and retries: the cap under two queued sends, a child still starting, a retry after its tab closed.

use super::delegation_test::{host_call, open, rig, start, statuses, thread_id, wait_until};
use super::orchestrate::Orchestration;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Host as _, Tool};

/// Two sends to idle children that together pass the cap: the parent must hear "no" for one of them, not two
/// quiet oks.
#[gpui::test]
fn two_queued_sends_past_the_cap_are_not_both_ok(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let start_tool = |p: &str| Tool::StartThread {
        project_id: rig.work_b,
        prompt: p.into(),
        worktree: false,
        request_id: None,
    };
    for n in 0..2 {
        host_call(cx, caller(), start_tool(&format!("child {n}"))).expect("under the limit");
    }
    wait_until(cx, "two idle children", |_| {
        let s = statuses(&store, parent_id);
        s.len() == 2 && s.iter().all(|s| s == "idle")
    });
    let idle: Vec<i64> = store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids")
        .into_iter()
        .map(|t| t.id)
        .collect();
    for _ in 0..3 {
        host_call(cx, caller(), start_tool("hold")).expect("three busy");
    }
    wait_until(cx, "three busy children", |_| {
        statuses(&store, parent_id)
            .iter()
            .filter(|s| *s == "running")
            .count()
            == 3
    });

    // Both sends on their way before the UI handles either.
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let calls: Vec<_> = idle
        .iter()
        .map(|&id| {
            let (host, caller) = (host.clone(), caller());
            std::thread::spawn(move || {
                host.call(
                    &caller,
                    Tool::SendMessage {
                        thread_id: id,
                        text: "one more turn".into(),
                    },
                )
            })
        })
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(300));
    wait_until(cx, "both sends", |_| calls.iter().all(|c| c.is_finished()));
    let oks = calls
        .into_iter()
        .map(|c| c.join().expect("no panic"))
        .filter(|r| r.is_ok())
        .count();
    assert!(
        oks < 2,
        "a quiet ok: both sends answered ok with only one free slot"
    );
}

/// A send to a child whose launch is still running says it is starting, not that it ended.
#[gpui::test]
fn a_send_to_a_starting_child_says_it_is_starting(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let reply = host_call(
        cx,
        caller(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "hold".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("started");
    let child = reply["thread_id"].as_i64().expect("thread_id");
    let view = ws
        .read_with(cx, |ws, cx| ws.open_view(child, cx))
        .expect("child tab");
    wait_until(cx, "the child's launch to end", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
    });
    // What the early row leaves while the agent spawns: the row is known, the launch hasn't ended.
    view.update(cx, |v, _| v.starting = true);
    let err = host_call(
        cx,
        caller(),
        Tool::SendMessage {
            thread_id: child,
            text: "hi".into(),
        },
    )
    .expect_err("refused while starting");
    assert!(err.contains("still starting"), "{err}");
}

/// A retry with the same request id after the child's tab was closed still names that child, not a new one.
#[gpui::test]
fn a_retry_after_the_child_tab_closed_returns_the_same_child(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let call = || Tool::StartThread {
        project_id: rig.work_b,
        prompt: "hold".into(),
        worktree: false,
        request_id: Some("r1".into()),
    };
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let first = host_call(cx, caller(), call()).expect("first")["thread_id"]
        .as_i64()
        .expect("id");
    ws.update_in(cx, |w, win, cx| {
        let v = w.open_view(first, cx).expect("child tab");
        w.close_thread(&v, win, cx);
    });
    cx.run_until_parked();
    let again = host_call(cx, caller(), call()).expect("retry")["thread_id"]
        .as_i64()
        .expect("id");
    assert_eq!(first, again, "retry created a second child");
}
