//! A local thread tab that opens on its own (a delegated child, or a start from another device) while the window
//! shows another machine waits instead: the window stays remote, the sidebar says a tab is waiting here, and
//! switching back to this machine shows it.

use super::env_harness::*;
use super::thread::ThreadView;
use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn in_dock(ws: &Entity<Workspace>, view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    ws.read_with(cx, |ws, cx| {
        ws.dock.read(cx).panel(view.entity_id().into()).is_some()
    })
}

fn background_tab(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<ThreadView> {
    ws.update_in(cx, |ws, window, cx| {
        let project = skyelab_store::Project {
            id: 1,
            name: "local".into(),
            path: std::env::temp_dir(),
            created_at: 0,
        };
        let (config, backend) = (ws.config.clone(), ws.backend.clone());
        let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
        ws.add_thread_tab(view.clone(), false, window, cx);
        view
    })
}

#[gpui::test]
fn a_background_tab_waits_while_another_machine_is_shown(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);

    let view = background_tab(&ws, cx);
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws.is_remote()),
        "still on the remote machine"
    );
    assert!(
        !in_dock(&ws, &view, cx),
        "no local tab among the remote ones"
    );
    let sections = sidebar(&ws, cx);
    let here = sections
        .iter()
        .find(|(title, _)| title == "This Mac")
        .unwrap_or_else(|| panic!("no This Mac section: {sections:?}"));
    assert_eq!(here.1, vec!["1 new thread here".to_owned()]);

    click_row(&ws, "1 new thread here", cx);
    wait_until(cx, "back on this machine", |cx| {
        ws.read_with(cx, |ws, _| !ws.is_remote())
    });
    assert!(in_dock(&ws, &view, cx), "the waiting tab is shown");
    assert!(
        !sidebar(&ws, cx)
            .iter()
            .any(|(title, _)| title == "This Mac"),
        "nothing waits any more"
    );
}

#[gpui::test]
fn a_background_tab_on_this_machine_opens_at_once(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let view = background_tab(&ws, cx);
    cx.run_until_parked();
    assert!(in_dock(&ws, &view, cx));
}
