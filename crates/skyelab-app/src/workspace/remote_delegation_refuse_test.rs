//! Criterion 2: every case that can't prove the same account is refused through B's real path, creates no thread,
//! and names no local path.

use super::remote_delegation_harness::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_protocol::ErrorCode;

/// Refused with `code`, nothing created, and no part of the project's path in the message.
fn refused(b: &B, from: &skyelab_protocol::Delegator, project: &str, code: ErrorCode, cx: &mut VisualTestContext) {
    let before = b.thread_count();
    let e = call(b, from, start_op(b.project(project), "r"), cx).expect_err("refused");
    assert_eq!(e.code, code, "{project}: {}", e.message);
    assert_eq!(b.thread_count(), before, "{project}: no thread row");
    let path = b.root.join(project);
    let tmp = b.root.file_name().and_then(|n| n.to_str()).expect("temp name");
    for part in [path.to_str().expect("utf-8"), project, tmp] {
        assert!(!e.message.contains(part), "{part} in {:?}", e.message);
    }
    assert!(!e.message.contains('/'), "{:?}", e.message);
}

#[gpui::test]
fn no_link_refuses(cx: &mut TestAppContext) {
    let (b, cx) = b_side(Setup::default(), cx);
    refused(&b, &caller(), "work-a", ErrorCode::DelegationNotAllowed, cx);
}

#[gpui::test]
fn every_other_unproven_case_refuses(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("personal"),
            ..Setup::default()
        },
        cx,
    );
    refused(&b, &caller(), "work-a", ErrorCode::DelegationNotAllowed, cx);
    skyelab_core::AppSettings::update(&b.settings, |s| s.link_delegation(link("work"))).expect("relink");
    refused(&b, &caller(), "loose", ErrorCode::BadRequest, cx);
    let mut other_email = caller();
    other_email.login.email = "you@acme.com".into();
    refused(&b, &other_email, "work-a", ErrorCode::DelegationNotAllowed, cx);
    let mut other_org = caller();
    other_org.login.org_id = "org-personal".into();
    refused(&b, &other_org, "work-a", ErrorCode::DelegationNotAllowed, cx);
    let mut codex_login = caller();
    codex_login.login.provider = "codex".into();
    refused(&b, &codex_login, "work-a", ErrorCode::DelegationNotAllowed, cx);
    skyelab_core::AppSettings::update(&b.settings, |s| s.link_delegation(link("cx"))).expect("link codex");
    refused(&b, &caller(), "codex", ErrorCode::DelegationNotAllowed, cx);
    skyelab_core::AppSettings::update(&b.settings, |s| {
        s.revoke_delegation(A_NODE, &link("cx").key_hash, "x");
    })
    .expect("revoke");
    refused(&b, &caller(), "work-a", ErrorCode::DelegationNotAllowed, cx);
}

#[gpui::test]
fn a_login_that_proves_nothing_refuses(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            no_org: true,
            ..Setup::default()
        },
        cx,
    );
    refused(&b, &caller(), "work-a", ErrorCode::DelegationNotAllowed, cx);
}
