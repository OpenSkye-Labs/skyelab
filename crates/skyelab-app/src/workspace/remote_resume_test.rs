//! A remote or delegated message to a stopped thread whose folder was rebound says why it was refused.
#![cfg(unix)] // the fake agent is a shell script

use super::threads_test::resume::{rebind, reopen, stopped_thread};
use super::threads_test::{fixture, setup};
use gpui::TestAppContext;

fn refused_with(to: Option<&str>, says: &str, cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    rebind(&f, to);
    let view = reopen(&f, &row, cx);
    let why = view
        .update_in(cx, |v, window, cx| v.submit_remote("again", window, cx))
        .expect_err("the resume is refused");
    assert!(why.contains(says), "{why}");
    assert!(!why.contains("still starting"), "{why}");
    view.read_with(cx, |v, _| assert!(v.session.is_none(), "nothing started"));
}

#[gpui::test]
fn remote_resume_into_a_rebound_folder_says_so(cx: &mut TestAppContext) {
    refused_with(Some("other"), "now bound to Other Co", cx);
}

#[gpui::test]
fn remote_resume_into_an_unbound_folder_says_so(cx: &mut TestAppContext) {
    refused_with(None, "isn't bound to acme", cx);
}
