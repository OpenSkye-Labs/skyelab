//! The command palette while another machine is shown lists that machine's projects and threads, and acts there.

use super::env_harness::*;
use super::*;
use gpui::VisualTestContext;

fn labels(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
            .iter()
            .map(|e| e.label.to_string())
            .collect()
    })
}

fn run(ws: &Entity<Workspace>, label: &str, cx: &mut VisualTestContext) {
    let entry = ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
            .into_iter()
            .find(|e| e.label == label)
            .unwrap_or_else(|| panic!("no palette entry {label}"))
    });
    cx.update(|window, cx| (entry.run)(window, cx));
}

#[gpui::test]
fn palette_lists_remote_projects_and_threads(cx: &mut gpui::TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);

    let shown = labels(&ws, cx);
    assert!(
        shown.contains(&"Switch to project: app".to_owned()),
        "{shown:?}"
    );

    run(&ws, "Switch to project: app", cx);
    wait_until(cx, "the remote threads in the palette", |cx| {
        labels(&ws, cx).contains(&"Open thread: fix the login bug".to_owned())
    });
    run(&ws, "Open thread: fix the login bug", cx);
    wait_until(cx, "the remote thread's tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });
}
