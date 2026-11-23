//! The client side of device approval: a machine that hasn't approved this device says so, with this device's
//! code, and connects once it is allowed there.

use super::env_harness::*;
use super::*;
use gpui::TestAppContext;
use skyelab_daemon::testing::NODE_NAME;
use skyelab_protocol::{device_code, device_key_hash};

#[gpui::test]
fn waiting_for_approval_shows_the_code_then_connects(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let daemon = remote.daemon.as_ref().expect("running");
    daemon.set_approved(vec![]);
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    switch(&ws, Some(NODE_NAME), cx);
    let code = device_code(&device_key_hash(skyelab_client::testing::DEVICE_KEY));
    wait_until(cx, "waiting for approval", |cx| {
        let label = state(&ws, cx);
        label.contains("approv") && label.contains(&code)
    });
    assert!(
        sidebar(&ws, cx)
            .first()
            .is_none_or(|(_, rows)| rows.is_empty())
    );
    let request = daemon.requests().remove(0);
    assert_eq!(request.code, code);
    daemon.set_approved(vec![skyelab_daemon::Approval {
        node_id: request.node_id,
        key_hash: request.key_hash,
    }]);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
}

#[gpui::test]
fn a_denied_device_says_so(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let daemon = remote.daemon.as_ref().expect("running");
    daemon.set_approved(vec![]);
    daemon.deny(
        "nPhone",
        &device_key_hash(skyelab_client::testing::DEVICE_KEY),
    );
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "denied", |cx| state(&ws, cx).contains("denied"));
}
