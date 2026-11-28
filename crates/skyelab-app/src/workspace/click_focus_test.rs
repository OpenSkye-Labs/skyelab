//! Acceptance: a mouse click on a button runs its action wherever the focus is: in a terminal, or on a menu that
//! just closed (its focus handle is no longer drawn). `Window::dispatch_action` started from that focus and fell
//! back to the window's root, above the Workspace, so the click did nothing (QA 2026-11-26 #1).

use super::center::Page;
use super::manage_harness::{Rig, rig};
use super::threads_test::wait_until;
use super::*;
use gpui::{FocusHandle, Modifiers, TestAppContext, VisualTestContext, px, size};
use crate::ui::dock::DockPlacement;

/// Where the focus is when the button is clicked.
#[derive(Clone, Copy, Debug)]
enum Focus {
    Terminal,
    ClosedMenu,
}

/// Puts the focus there; the returned handle keeps a closed menu's focus alive but undrawn.
fn focus(how: Focus, r: &Rig, cx: &mut VisualTestContext) -> FocusHandle {
    match how {
        Focus::Terminal => {
            cx.dispatch_action(NewTerminal);
            wait_until(cx, "a terminal", |cx| {
                r.ws.read_with(cx, |ws, _| !ws.terminals.is_empty())
            });
            let panel = r.ws.read_with(cx, |ws, _| ws.terminals[0].clone());
            let term = panel.read_with(cx, |p, _| p.panes[0].view.clone());
            let handle = term.read_with(cx, |t, cx| t.focus_handle(cx));
            cx.update(|window, cx| window.focus(&handle, cx));
            cx.run_until_parked();
            assert!(
                cx.update(|window, _| handle.is_focused(window)),
                "the terminal holds focus"
            );
            handle
        }
        Focus::ClosedMenu => {
            let handle = cx.update(|window, cx| {
                let h = cx.focus_handle();
                window.focus(&h, cx);
                h
            });
            cx.run_until_parked();
            handle
        }
    }
}

fn click(sel: &'static str, cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds(sel)
        .unwrap_or_else(|| panic!("{sel} is drawn"))
        .center();
    // A mouse click: down and up at the same point (GPUI's keyboard "click" fires on key-up instead).
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn start(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(1400.), px(900.)));
    cx.run_until_parked();
    (r, cx)
}

fn right_open(r: &Rig, cx: &mut VisualTestContext) -> bool {
    r.ws.read_with(cx, |ws, cx| {
        ws.dock.read(cx).is_dock_open(DockPlacement::Right)
    })
}

#[gpui::test]
fn header_toggle_runs_from_a_terminal(cx: &mut TestAppContext) {
    header_toggle(Focus::Terminal, cx);
}

#[gpui::test]
fn header_toggle_runs_after_a_menu_closed(cx: &mut TestAppContext) {
    header_toggle(Focus::ClosedMenu, cx);
}

fn header_toggle(how: Focus, cx: &mut TestAppContext) {
    let (r, cx) = start(cx);
    let _focus = focus(how, &r, cx);
    let before = right_open(&r, cx);
    click("toggle-right", cx);
    assert_ne!(right_open(&r, cx), before, "{how:?}: the page header's panel toggle ran");
}

#[gpui::test]
fn rail_button_runs_from_a_terminal(cx: &mut TestAppContext) {
    rail_button(Focus::Terminal, cx);
}

#[gpui::test]
fn rail_button_runs_after_a_menu_closed(cx: &mut TestAppContext) {
    rail_button(Focus::ClosedMenu, cx);
}

fn rail_button(how: Focus, cx: &mut TestAppContext) {
    let (r, cx) = start(cx);
    let _focus = focus(how, &r, cx);
    click("rail-bots", cx);
    assert!(
        r.ws.read_with(cx, |ws, _| matches!(ws.page, Page::Bots)),
        "{how:?}: the left rail's Bots button ran"
    );
}

#[gpui::test]
fn sidebar_button_runs_from_a_terminal(cx: &mut TestAppContext) {
    sidebar_button(Focus::Terminal, cx);
}

#[gpui::test]
fn sidebar_button_runs_after_a_menu_closed(cx: &mut TestAppContext) {
    sidebar_button(Focus::ClosedMenu, cx);
}

/// The sidebar's Refresh lists a thread the store gained behind the workspace's back.
fn sidebar_button(how: Focus, cx: &mut TestAppContext) {
    let (r, cx) = start(cx);
    r.f.store
        .lock()
        .expect("lock")
        .create_thread(&skyelab_store::NewThread {
            project_id: r.project,
            title: "Behind its back",
            cwd: &r.f.project_dir,
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    let _focus = focus(how, &r, cx);
    assert!(!r.listed(cx).iter().any(|t| t == "Behind its back"));
    click("sidebar-refresh", cx);
    assert!(
        r.listed(cx).iter().any(|t| t == "Behind its back"),
        "{how:?}: the sidebar's Refresh ran"
    );
}

/// The right dock's tabs, counted the way F6 finds them.
fn right_tabs(r: &Rig, cx: &mut VisualTestContext) -> usize {
    r.ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        let mut n = 0;
        if let Some(layout) = dock.layout(DockPlacement::Right) {
            layout.root().walk(&mut |node| {
                if let crate::ui::dock::PaneRef::Tabs { panels, .. } = node.kind() {
                    n += panels.len();
                }
            });
        }
        n
    })
}

/// T3's mod+w: with the composer focused it closes the right panel's active tab, and the window stays.
#[gpui::test]
fn cmd_w_closes_the_right_panel_tab_not_the_window(cx: &mut TestAppContext) {
    let (r, cx) = start(cx);
    let id = r.thread("Fix it", cx);
    let view = r.open(id, cx);
    view.update(cx, |_, cx| cx.emit(thread::ThreadViewEvent::OpenChanges));
    cx.run_until_parked();
    let tabs = right_tabs(&r, cx);
    assert!(tabs >= 1, "Changes opened in the right panel");
    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes(layout::CLOSE_TAB_KEY);
    cx.run_until_parked();
    assert_eq!(right_tabs(&r, cx), tabs - 1, "mod+w closed the active tab");
    assert_eq!(
        cx.update(|_, cx| cx.windows().len()),
        1,
        "the window stays open"
    );
}

/// In a terminal mod+w stays the terminal's (macOS: CloseTerminal; elsewhere ctrl-w is the shell's): the right
/// panel's tab stays.
#[gpui::test]
fn cmd_w_in_a_terminal_leaves_the_right_panel_alone(cx: &mut TestAppContext) {
    let (r, cx) = start(cx);
    let id = r.thread("Fix it", cx);
    let view = r.open(id, cx);
    view.update(cx, |_, cx| cx.emit(thread::ThreadViewEvent::OpenChanges));
    cx.run_until_parked();
    let tabs = right_tabs(&r, cx);
    let _focus = focus(Focus::Terminal, &r, cx);
    cx.simulate_keystrokes(layout::CLOSE_TAB_KEY);
    cx.run_until_parked();
    assert_eq!(right_tabs(&r, cx), tabs, "the right panel kept its tab");
}

