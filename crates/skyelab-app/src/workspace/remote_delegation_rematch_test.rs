//! A request that matched no local account is checked again, so a sign-in later offers Allow.

use super::remote_delegation_harness::*;
use crate::remote::delegation_panel::key_of;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_protocol::DelegateOp;

#[gpui::test]
fn a_request_matched_to_no_account_is_checked_again_after_a_sign_in(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            no_org: true,
            ..Setup::default()
        },
        cx,
    );
    call(&b, &caller(), DelegateOp::ListProjects, cx).expect_err("no link yet");
    wait_until(cx, "the request, matched to nothing", |cx| {
        b.remote.update(cx, |r, cx| r.refresh_peers(cx));
        b.remote.read_with(cx, |r, _| {
            r.delegation_requests
                .first()
                .is_some_and(|q| r.delegation_matches.get(&key_of(q)).is_some_and(|m| m.is_none()))
        })
    });
    std::fs::write(b.account_dir("work").join("fake-org"), ORG).expect("org");
    wait_until(cx, "work matched after its sign-in", |cx| {
        b.remote.update(cx, |r, cx| r.refresh_peers(cx));
        b.remote.read_with(cx, |r, _| {
            r.delegation_requests
                .first()
                .and_then(|q| r.delegation_matches.get(&key_of(q)).cloned().flatten())
                .is_some_and(|(id, _)| id == "work")
        })
    });
}
