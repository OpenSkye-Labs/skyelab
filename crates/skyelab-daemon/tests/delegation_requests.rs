//! A `delegate` call this machine refuses for want of a link lists the caller for the user here, like a device's
//! hello does: at most 16 requests, 2 per node. Allow lets the next call through, Deny refuses it unlisted. The
//! caller is named by the connection (whois and the approved key), whatever the message says.

mod common;

use common::*;
use skyelab_daemon::Approval;
use skyelab_daemon::testing::{Env, Peer};
use skyelab_protocol::{
    Call, DelegateOp, Delegator, ErrorCode, Login, Reply, ServerMessage, device_key_hash,
};

fn caller(account: &str) -> Delegator {
    Delegator {
        store_id: "s1".into(),
        thread_id: 7,
        title: "parent".into(),
        account_id: account.into(),
        login: Login {
            provider: "claude".into(),
            email: "me@acme.com".into(),
            org_id: "org-acme".into(),
        },
        depth: 0,
    }
}

fn delegate(ws: &mut Client, id: u64, account: &str) -> Result<serde_json::Value, ErrorCode> {
    send(
        ws,
        id,
        Call::Delegate {
            caller: caller(account),
            op: DelegateOp::ListProjects,
        },
    );
    match recv(ws) {
        Some(ServerMessage::Response {
            result: Reply::Delegated(v),
            ..
        }) => Ok(v),
        Some(ServerMessage::Error { error, .. }) => Err(error.code),
        other => panic!("unexpected {other:?}"),
    }
}

fn addr(env: &Env) -> std::net::SocketAddr {
    env.daemon.as_ref().expect("served").local_addrs()[0]
}

#[test]
fn a_refusal_lists_one_request_and_allow_admits_the_next_call() {
    let env = Env::start();
    let daemon = env.daemon.as_ref().expect("served");
    let mut ws = connect_hello(addr(&env));
    assert_eq!(
        delegate(&mut ws, 2, "x"),
        Err(ErrorCode::DelegationNotAllowed)
    );
    assert_eq!(
        delegate(&mut ws, 3, "x"),
        Err(ErrorCode::DelegationNotAllowed)
    );
    let requests = daemon.delegation_requests();
    assert_eq!(requests.len(), 1, "one request per caller: {requests:?}");
    let r = &requests[0];
    assert_eq!(
        r.origin.node_id, "nPhone",
        "the node whois named, not the message"
    );
    assert_eq!(r.origin.device, "phone.example.ts.net");
    assert_eq!(r.origin.key_hash, device_key_hash(KEY));
    assert_eq!(
        (r.account_id.as_str(), r.email.as_str(), r.org_id.as_str()),
        ("x", "me@acme.com", "org-acme")
    );
    // Allow: the app saves the link, which its backend then reads, and the request goes.
    env.agents.delegation_links.lock().expect("links").push((
        r.origin.node_id.clone(),
        r.origin.key_hash.clone(),
        "x".into(),
    ));
    daemon.forget_delegation_request(&r.origin.node_id, &r.origin.key_hash, "x");
    let ok = delegate(&mut ws, 4, "x").expect("linked now");
    assert_eq!(ok["origin"], "nPhone");
    assert!(daemon.delegation_requests().is_empty());
}

#[test]
fn deny_refuses_unlisted_until_the_daemon_restarts() {
    let mut env = Env::start();
    let mut ws = connect_hello(addr(&env));
    assert_eq!(
        delegate(&mut ws, 2, "y"),
        Err(ErrorCode::DelegationNotAllowed)
    );
    let r = env
        .daemon
        .as_ref()
        .expect("served")
        .delegation_requests()
        .remove(0);
    env.daemon.as_ref().expect("served").deny_delegation(
        &r.origin.node_id,
        &r.origin.key_hash,
        "y",
    );
    // Even a link saved later doesn't pass while the Deny holds.
    env.agents.delegation_links.lock().expect("links").push((
        r.origin.node_id.clone(),
        r.origin.key_hash.clone(),
        "y".into(),
    ));
    let before = env.asked().iter().filter(|a| **a == "delegate").count();
    assert_eq!(
        delegate(&mut ws, 3, "y"),
        Err(ErrorCode::DelegationNotAllowed)
    );
    assert!(
        env.daemon
            .as_ref()
            .expect("served")
            .delegation_requests()
            .is_empty(),
        "not listed again"
    );
    assert_eq!(
        env.asked().iter().filter(|a| **a == "delegate").count(),
        before,
        "a denied caller never reaches the backend"
    );
    drop(ws);
    env.serve();
    let mut ws = connect_hello(addr(&env));
    assert!(
        delegate(&mut ws, 2, "y").is_ok(),
        "the Deny ended with the daemon"
    );
}

#[test]
fn requests_are_capped_per_node_and_in_all() {
    const NODES: [&str; 9] = ["n1", "n2", "n3", "n4", "n5", "n6", "n7", "n8", "n9"];
    let env = Env::start();
    let daemon = env.daemon.as_ref().expect("served");
    daemon.set_approved(
        NODES
            .iter()
            .map(|n| Approval {
                node_id: (*n).into(),
                key_hash: device_key_hash(KEY),
            })
            .collect(),
    );
    for node in NODES {
        *env.tailnet.peer.lock().expect("peer") = Peer::OwnNode(node);
        let mut ws = connect_hello(addr(&env));
        for (i, account) in ["a", "b", "c"].iter().enumerate() {
            assert_eq!(
                delegate(&mut ws, 2 + i as u64, account),
                Err(ErrorCode::DelegationNotAllowed)
            );
        }
    }
    let requests = daemon.delegation_requests();
    assert_eq!(requests.len(), 16, "at most 16 in all");
    for node in NODES {
        let n = requests.iter().filter(|r| r.origin.node_id == node).count();
        assert!(n <= 2, "{node} has {n}");
    }
    let n1: Vec<&str> = requests
        .iter()
        .filter(|r| r.origin.node_id == "n1")
        .map(|r| r.account_id.as_str())
        .collect();
    assert_eq!(n1, ["b", "c"], "a full node's newest replaces its oldest");
}
