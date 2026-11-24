//! A workspace showing another machine opens no picker, search or preview: those files aren't on this machine.

use super::super::env_harness::*;
use super::{OpenFilePicker, SearchFiles};
use std::sync::Arc;

#[gpui::test]
fn remote_workspace_opens_no_file_tools(cx: &mut gpui::TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);

    cx.dispatch_action(OpenFilePicker);
    cx.dispatch_action(SearchFiles);
    cx.run_until_parked();
    // No Root in this window, so a picker dialog would have panicked on open.
    ws.update_in(cx, |ws, window, cx| {
        assert_eq!(ws.files_root(cx), None);
        ws.open_preview("/".into(), "etc/hosts".into(), None, window, cx);
        assert!(ws.files.search.is_none());
        assert!(ws.files.preview.is_none());
    });
    // Reopen Closed View finds nothing to open here, and keeps the closed tab for when the window is local again.
    ws.update_in(cx, |_, window, cx| {
        let closed = super::super::reopen::Closed::Preview {
            root: "/".into(),
            path: "etc/hosts".into(),
        };
        super::super::reopen::remember(window, closed, cx);
    });
    cx.dispatch_action(super::super::reopen::ReopenClosedView);
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        assert!(ws.files.preview.is_none());
        assert_eq!(super::super::reopen::pending(window, cx), 1);
    });
}
