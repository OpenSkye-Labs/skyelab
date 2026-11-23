//! Once a parent is stopped, a browser call it had on its way is refused like start and send, and its
//! `wait_thread` stops polling instead of holding the connection for the rest of its timeout.

use super::browser_agent_test::fake_pages;
use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use super::thread::Stop;
use gpui::TestAppContext;
use skyelab_agent::mcp::{BrowserTool, Caller, Host as _, Tool};

#[gpui::test]
fn a_stopped_parent_cannot_open_a_tab_or_keep_waiting(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let _pages = fake_pages(&ws, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let child = host_call(
        cx,
        caller(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "hold".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("a live parent delegates");
    let child = child["thread_id"].as_i64().expect("id");

    parent.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the parent stopped", |cx| {
        parent.read_with(cx, |v, _| v.session.is_none())
    });

    let err = host_call(
        cx,
        caller(),
        Tool::Browser(BrowserTool::Open {
            url: "http://localhost:3000/".into(),
        }),
    )
    .expect_err("a stopped parent opens no tab");
    assert!(err.contains("isn't running"), "{err}");
    assert!(
        ws.read_with(cx, |ws, _| ws
            .browsers
            .iter()
            .all(|b| b.upgrade().is_none())),
        "no agent tab"
    );

    let started = std::time::Instant::now();
    let err = host_call(
        cx,
        caller(),
        Tool::WaitThread {
            thread_id: child,
            timeout_s: 60,
        },
    )
    .expect_err("a stopped parent's wait ends");
    assert!(err.contains("isn't running"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

/// Stop then resume between two polls leaves the row live, but the old grant is gone: its wait ends anyway.
#[gpui::test]
fn a_wait_ends_when_its_grant_is_revoked(cx: &mut TestAppContext) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let caller = Caller {
        thread_id: thread_id(&parent, cx),
        account_id: "work".into(),
    };
    let child = host_call(
        cx,
        caller.clone(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "hold".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("delegated")["thread_id"]
        .as_i64()
        .expect("id");
    let host = cx.update(|_, cx| {
        cx.global::<super::orchestrate::Orchestration>()
            .host
            .clone()
    });
    let granted = Arc::new(AtomicBool::new(true));
    let still = granted.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let wait = Tool::WaitThread {
            thread_id: child,
            timeout_s: 60,
        };
        let _ = tx.send(host.call_granted(&caller, wait, &|| still.load(Ordering::SeqCst)));
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    granted.store(false, Ordering::SeqCst);
    let err = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the wait ended within 5 s of the revocation")
        .expect_err("a revoked caller's wait");
    assert!(err.contains("isn't running"), "{err}");
}
