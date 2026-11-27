//! Sharing & access's delegation buttons: a refused caller is listed with the local account its login matches,
//! Allow saves that link so the next call passes, and Deny refuses it unlisted.

use super::remote_delegation_harness::*;
use crate::remote::delegation_panel::{AllowDelegation, DenyDelegation, key_of};
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_protocol::{DelegateOp, ErrorCode};

#[gpui::test]
fn allow_links_the_matching_account_and_deny_refuses(cx: &mut TestAppContext) {
    let (b, cx) = b_side(Setup::default(), cx);
    let e = call(&b, &caller(), DelegateOp::ListProjects, cx).expect_err("no link yet");
    assert_eq!(e.code, ErrorCode::DelegationNotAllowed);
    wait_until(cx, "the request, matched to work", |cx| {
        b.remote.update(cx, |r, cx| r.refresh_peers(cx));
        b.remote.read_with(cx, |r, _| {
            r.delegation_requests
                .first()
                .and_then(|q| r.delegation_matches.get(&key_of(q)).cloned().flatten())
                .is_some_and(|(id, _)| id == "work")
        })
    });
    let (node_id, key_hash, account_id) = b.remote.read_with(cx, |r, _| key_of(&r.delegation_requests[0]));
    let stale = AllowDelegation {
        node_id: node_id.clone(),
        key_hash: key_hash.clone(),
        account_id: account_id.clone(),
        local_account: "personal".into(),
    };
    cx.dispatch_action(stale);
    cx.run_until_parked();
    assert!(
        skyelab_core::AppSettings::load(&b.settings).expect("load").delegation.links.is_empty(),
        "an Allow for another account than the request now matches saves nothing"
    );
    cx.dispatch_action(AllowDelegation {
        node_id: node_id.clone(),
        key_hash: key_hash.clone(),
        account_id: account_id.clone(),
        local_account: "work".into(),
    });
    wait_until(cx, "the link saved", |_| {
        !skyelab_core::AppSettings::load(&b.settings).expect("load").delegation.links.is_empty()
    });
    let links = skyelab_core::AppSettings::load(&b.settings).expect("load").delegation.links;
    assert_eq!(links.len(), 1);
    assert_eq!((links[0].local_account.as_str(), links[0].login.org_id.as_str()), ("work", ORG));
    let listed = call(&b, &caller(), DelegateOp::ListProjects, cx).expect("linked now");
    assert_eq!(listed.as_array().map(Vec::len), Some(2));

    let mut other = caller();
    other.account_id = "y".into();
    call(&b, &other, DelegateOp::ListProjects, cx).expect_err("not linked");
    wait_until(cx, "y's request", |cx| {
        b.remote.update(cx, |r, cx| r.refresh_peers(cx));
        b.remote.read_with(cx, |r, _| r.delegation_requests.iter().any(|q| q.account_id == "y"))
    });
    cx.dispatch_action(DenyDelegation {
        node_id,
        key_hash,
        account_id: "y".into(),
    });
    cx.run_until_parked();
    call(&b, &other, DelegateOp::ListProjects, cx).expect_err("denied");
    b.remote.update(cx, |r, cx| r.refresh_peers(cx));
    assert!(b.remote.read_with(cx, |r, _| r.delegation_requests.is_empty()), "not listed again");
}
