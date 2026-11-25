//! While the window shows another machine, a local thread can't be continued in the other provider (its new thread
//! couldn't open here), so the Details row, the refusal notice's button and the palette entry are gone; back on this
//! machine they return.

use crate::workspace::env_harness::{
    FakeTailnet, RemoteSideExt as _, connect, remote_side, switch,
};
use crate::workspace::thread_extras::handoff::{
    ContinueInClaude, ContinueInCodex, ContinueInCursor,
};
use crate::workspace::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done,
};
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::sync::Arc;

#[gpui::test]
fn a_remote_window_hides_continue_in(cx: &mut TestAppContext) {
    setup(cx);
    let remote = remote_side(FakeTailnet::default());
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.set_environments(
            Arc::new(FakeTailnet::default()),
            None,
            remote.options(),
            window,
            cx,
        )
    });
    let src = project_thread(&f, &ws, cx);
    type_and_send(&src, cx);
    wait_done(&src, cx);
    let offered = |cx: &mut gpui::VisualTestContext| {
        src.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
        cx.update(|window, cx| {
            window.is_action_available(&ContinueInCodex, cx)
                || window.is_action_available(&ContinueInClaude, cx)
                || window.is_action_available(&ContinueInCursor, cx)
        })
    };
    assert_eq!(
        src.read_with(cx, |t, _| t.handoff_targets()),
        vec![Provider::Codex, Provider::Cursor]
    );
    assert!(offered(cx), "offered on this machine");

    connect(&ws, &remote, cx);
    assert_eq!(
        src.read_with(cx, |t, _| t.handoff_targets()),
        Vec::<Provider>::new(),
        "hidden remotely"
    );
    assert!(!offered(cx), "no palette entry remotely");

    switch(&ws, None, cx);
    cx.run_until_parked();
    assert_eq!(
        src.read_with(cx, |t, _| t.handoff_targets()),
        vec![Provider::Codex, Provider::Cursor],
        "back on this machine"
    );
}
