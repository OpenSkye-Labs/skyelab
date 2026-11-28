//! Acceptance: saved terminal tabs come back in their saved order, whichever shell starts first, and the last saved
//! tab is the one the drawer shows (as when it was saved). Run over many scheduler seeds: the shells start off the
//! UI thread, so their finishing order varies.

use super::threads_test::{fixture, open_workspace, setup};
use super::worktree_thread_test::wait_until;
use crate::ui::dock::DockPlacement;
use gpui::TestAppContext;
use skyelab_core::terminal_layout::{Axis, PaneTree, SavedTerminalTab};

#[gpui::test(iterations = 30)]
fn saved_tabs_reopen_in_saved_order(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let web = f.project_dir.join("web");
    std::fs::create_dir_all(&web).expect("mkdir");
    let tab = |name: &str, panes| SavedTerminalTab {
        thread: None,
        name: Some(name.into()),
        panes,
    };
    let mut split = PaneTree::Leaf(f.project_dir.clone());
    split.split(&f.project_dir, web.clone(), Axis::Row);
    let saved = vec![
        tab("one", split),
        tab("two", PaneTree::Leaf(f.project_dir.clone())),
        tab("three", PaneTree::Leaf(web.clone())),
        tab("four", PaneTree::Leaf(f.project_dir.clone())),
    ];
    let (ws, cx) = open_workspace(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.restore_terminals(saved.clone(), window, cx)
    });
    wait_until(cx, "every saved tab", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == saved.len())
    });
    cx.run_until_parked();
    cx.dispatch_action(super::ToggleBottomPanel);
    cx.run_until_parked();

    ws.read_with(cx, |ws, cx| {
        let names: Vec<_> = ws
            .terminals
            .iter()
            .map(|t| t.read(cx).name.clone().map(|n| n.to_string()))
            .collect();
        let want: Vec<_> = saved.iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, want, "the saved order");
        let dock = ws.dock.read(cx);
        let group = dock.tabs(DockPlacement::Bottom).expect("the drawer's tabs");
        let ids: Vec<_> = ws.terminals.iter().map(|t| t.entity_id().into()).collect();
        assert_eq!(group.panels, ids, "the drawer's tabs in the saved order");
        let last = ws.terminals.last().expect("a tab").entity_id().into();
        assert!(
            ws.dock_shows(DockPlacement::Bottom, last, cx),
            "the last saved tab shows"
        );
        assert_eq!(ws.terminal_layouts(cx), saved, "saved again as it was");
        for p in &ws.terminals {
            p.read(cx).shutdown(cx);
        }
    });
}
