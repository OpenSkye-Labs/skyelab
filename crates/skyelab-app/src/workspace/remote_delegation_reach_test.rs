//! Criterion 7: a caller reaches only the threads it started here: same origin node, store, parent and account.
//! An idle child keeps its agent past the idle-release delay, so the caller can still send to it.

use super::remote_delegation_harness::*;
use super::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_daemon::Approval;
use skyelab_daemon::testing::{DEVICE_KEY, NODE_NAME, Peer, ca};
use skyelab_protocol::{DelegateOp, ErrorCode, device_key_hash};
use std::time::Duration;

#[gpui::test]
fn only_the_parent_reaches_its_child(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            ..Setup::default()
        },
        cx,
    );
    let child = started(&b, &caller(), start_op(b.project("work-a"), "r1"), cx);
    wait_until(cx, "the child's turn to end", |_| {
        lock(&b.store).and_then(|s| s.thread(child)).ok().flatten().is_some_and(|t| t.status == "idle")
    });
    let ops = |id: i64| {
        [
            DelegateOp::Status { thread_id: id },
            DelegateOp::Read {
                thread_id: id,
                since_seq: 0,
            },
            DelegateOp::Wait {
                thread_id: id,
                timeout_s: 1,
            },
        ]
    };
    for op in ops(child) {
        call(&b, &caller(), op, cx).expect("the parent reaches its child");
    }
    let waited = call(&b, &caller(), DelegateOp::Wait { thread_id: child, timeout_s: 1 }, cx).expect("wait");
    assert_eq!(waited["done"], true);

    let mut other_parent = caller();
    other_parent.thread_id = 8;
    let mut other_store = caller();
    other_store.store_id = "fresh-data-dir".into();
    let local = {
        let s = lock(&b.store).expect("lock");
        let p = s.project(b.project("work-a")).expect("read").expect("project");
        s.create_thread(&skyelab_store::NewThread {
            project_id: p.id,
            title: "the user's own",
            cwd: &p.path,
            account_id: "work",
            status: "idle",
        })
        .expect("thread")
        .id
    };
    for (who, id) in [(&other_parent, child), (&other_store, child), (&caller(), local)] {
        for op in ops(id) {
            let e = call(&b, who, op, cx).expect_err("not theirs");
            assert_eq!(e.code, ErrorCode::NotFound, "{}", e.message);
        }
    }

    // Another of the user's devices, allowed and linked too, still can't reach this caller's child.
    b.remote.read_with(cx, |r, _| {
        r.with_daemon(|d| {
            d.set_approved(
                ["nPhone", "nOther"]
                    .map(|n| Approval {
                        node_id: n.into(),
                        key_hash: device_key_hash(DEVICE_KEY),
                    })
                    .to_vec(),
            )
        })
    });
    skyelab_core::AppSettings::update(&b.settings, |s| {
        s.link_delegation(skyelab_core::delegation_remote::DelegationLink {
            node_id: "nOther".into(),
            ..link("work")
        })
    })
    .expect("link");
    *b.tailnet.peer.lock().expect("peer") = Peer::OwnNode("nOther");
    let port = b.remote.read_with(cx, |r, _| match &r.status {
        crate::remote::Status::Sharing { port, .. } => Some(*port),
        _ => None,
    });
    let other = skyelab_client::Client::connect(
        NODE_NAME,
        skyelab_client::testing::options(port.expect("port"), ca().root()),
    )
    .expect("client");
    wait_until(cx, "the other device's hello", |_| other.hello().is_some());
    for op in ops(child) {
        let e = call_with(&other, &caller(), op, cx).expect_err("another node");
        assert_eq!(e.code, ErrorCode::NotFound, "{}", e.message);
    }

    // Idle past the release delay: the agent is kept, and the parent's send still lands.
    cx.executor().advance_clock(Duration::from_secs(11 * 60));
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    let view = b.ws.read_with(cx, |ws, cx| ws.open_view(child, cx)).expect("its tab");
    assert!(view.read_with(cx, |v, _| v.session.is_some()), "a delegated child keeps its agent");
    let sent = call(&b, &caller(), DelegateOp::Send { thread_id: child, text: "more".into() }, cx);
    assert_eq!(sent.expect("the parent's send lands")["ok"], true);
}
