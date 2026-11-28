//! Stage 5b acceptance: a settings.toml written by main before our dock engine (`8147abe11`, captured from
//! `schedule_layout_save` in a 1400x900 window) restores the same layout on it: the right dock open at 520 px, the
//! Welcome page, two terminal tabs (the first split) behind the drawer's strip, and the docks where main drew them.

use super::threads_test::{fixture, open_workspace, setup};
use super::worktree_thread_test::wait_until;
use crate::ui::dock::DockPlacement;
use gpui::{TestAppContext, point, px, size};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/settings-main-8147abe11.toml"
);

#[gpui::test]
fn a_layout_saved_by_main_restores_the_same_docks(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    std::fs::create_dir_all(f.project_dir.join("web")).expect("mkdir");
    let text = std::fs::read_to_string(FIXTURE).expect("fixture");
    let path = f.project_dir.parent().expect("root").join("settings.toml");
    let project = f.project_dir.display().to_string();
    std::fs::write(&path, text.replace("@PROJECT@", &project)).expect("write settings");
    let saved = skyelab_core::AppSettings::load(&path).expect("main's file loads");
    let layout = saved
        .saved_windows()
        .into_iter()
        .next()
        .expect("one window");

    let (ws, cx) = open_workspace(&f, cx);
    cx.simulate_resize(size(px(1400.), px(900.)));
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.restore_layout(Some(path.clone()), Some(&layout), window, cx)
    });
    wait_until(cx, "both saved terminals", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    cx.run_until_parked();

    ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        let state = |at| (dock.is_dock_open(at), dock.dock_size(at));
        assert_eq!(
            state(DockPlacement::Right),
            (true, Some(px(520.))),
            "the right dock"
        );
        assert_eq!(
            state(DockPlacement::Left),
            (true, Some(px(256.))),
            "the sidebar"
        );
        assert_eq!(
            state(DockPlacement::Bottom),
            (false, Some(px(280.))),
            "the drawer, closed"
        );
        assert_eq!(ws.page.key(cx), "welcome");
        assert_eq!(ws.terminals.len(), 2, "both terminal tabs");
        // Each tab restores in its own task, so which comes back first varies (on main too).
        let mut panes: Vec<_> = ws
            .terminals
            .iter()
            .map(|t| t.read(cx).panes.len())
            .collect();
        panes.sort_unstable();
        assert_eq!(panes, [1, 2], "one of them split");
    });
    // Where main drew the closed drawer's strip: right of the rail and sidebar, left of the right dock.
    let strip = cx
        .debug_bounds("drawer-collapsed")
        .expect("the drawer's strip");
    assert_eq!(strip.origin, point(px(304.), px(839.)));
    assert_eq!(strip.size, size(px(576.), px(30.)));

    cx.dispatch_action(super::ToggleBottomPanel);
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert!(ws.dock.read(cx).is_dock_open(DockPlacement::Bottom));
        assert_eq!(ws.terminals.len(), 2, "opening the drawer adds no shell");
        let shown = |i: usize| {
            let id = ws.terminals[i].entity_id().into();
            ws.dock_shows(DockPlacement::Bottom, id, cx)
        };
        assert_eq!((shown(0), shown(1)), (false, true), "the last tab shows");
    });
    let kept = ws.update_in(cx, |ws, window, _| ws.focus.is_focused(window));
    assert!(
        kept,
        "a restore and the toggle leave the focus on the window"
    );
    let first = cx
        .debug_bounds("dock-tab-0")
        .expect("the first terminal tab");
    let second = cx
        .debug_bounds("dock-tab-1")
        .expect("the second terminal tab");
    assert_eq!(
        first.origin,
        point(px(304.), px(588.)),
        "the drawer is 280 px"
    );
    assert_eq!(first.size.height, px(32.));
    assert_eq!(second.origin.y, px(588.));
    assert!(second.origin.x > first.origin.x, "side by side");
}
