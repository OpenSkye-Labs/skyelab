//! GPUI tests for right-click context menus on terminal tabs and right-panel dock tabs.
//!
//! Uses real `MouseButton::Right` clicks to verify painted non-empty bounds, terminal tab actions (Rename, Split
//! right, Split down, New terminal, Close tab, Close other tabs with busy shell confirmation), and right-panel dock
//! tab actions (Close tab, Close other tabs, Close all tabs, Surfaces non-closable safety, and unsaved Preview
//! Save/Discard/Cancel confirmation).

use super::surfaces::OpenThreadChanges;
use super::thread_extras::prefs;
use super::worktree_thread_test::{fixture, open_thread_with, wait_until};
use super::*;
use gpui::{Modifiers, MouseButton, Point, Pixels, TestAppContext, VisualTestContext, px};
use skyelab_core::terminal_layout::{Axis, PaneTree};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    draw(cx);
}

fn assert_menu_painted(cx: &mut VisualTestContext) -> gpui::Bounds<Pixels> {
    let bounds = cx
        .debug_bounds("tab-context-menu")
        .expect("tab context menu painted");
    assert!(
        bounds.size.width > px(0.) && bounds.size.height > px(0.),
        "menu must paint with non-empty bounds: {bounds:?}"
    );
    bounds
}

fn no_confirm(cx: &mut VisualTestContext) {
    let off = skyelab_core::TerminalSettings {
        confirm_close: false,
    };
    cx.update(|_, cx| prefs::save_terminal(off, cx).detach());
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(crate::ui::has_active_dialog)
}

fn has_axis<T>(tree: &PaneTree<T>, target: Axis) -> bool {
    match tree {
        PaneTree::Leaf(_) => false,
        PaneTree::Split { axis, children, .. } => {
            *axis == target || children.iter().any(|c| has_axis(c, target))
        }
    }
}

fn run_sleep(panel: &Entity<TerminalPanel>, at: usize, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |p, _| p.panes[at].view.clone());
    let probe = view
        .read_with(cx, |v, _| v.terminal().probe())
        .expect("a live shell");
    wait_until(cx, "the prompt", |cx| {
        !probe.is_busy() && view.read_with(cx, |v, _| !v.terminal().text().trim().is_empty())
    });
    view.read_with(cx, |v, _| v.terminal().write(b"sleep 30\n".as_slice()));
    wait_until(cx, "sleep in front", |_| {
        probe.foreground().command.as_deref() == Some("sleep")
    });
}

fn terminal_tab_at(cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.debug_bounds("terminal-title")
        .expect("terminal tab title drawn")
        .center()
}

#[gpui::test]
fn tab_context_menu_terminal_tab_actions(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread_with(&f, true, cx);
    no_confirm(cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    draw(cx);
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());

    // 1. Rename (1st item)
    let at = terminal_tab_at(cx);
    right_click(cx, at);
    assert!(ws.read_with(cx, |ws, _| ws.tab_menu.is_open()));
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert!(
        panel.read_with(cx, |p, _| p.renaming.is_some()),
        "Rename opens inline rename input"
    );
    panel.update(cx, |p, cx| p.finish_rename(false, cx));
    draw(cx);

    // 2. Split right (2nd item)
    let at = terminal_tab_at(cx);
    right_click(cx, at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down enter");
    wait_until(cx, "split right", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 2 && has_axis(&p.tree, Axis::Row))
    });
    draw(cx);

    // 3. Split down (3rd item)
    let at = terminal_tab_at(cx);
    right_click(cx, at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down enter");
    wait_until(cx, "split down", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 3 && has_axis(&p.tree, Axis::Column))
    });
    draw(cx);

    // 4. New terminal (4th item)
    let at = terminal_tab_at(cx);
    right_click(cx, at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down down enter");
    wait_until(cx, "second terminal tab", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    draw(cx);

    // 5. Close other tabs on tab 0 (6th item, shown only when >1 terminal tab exists)
    let first_tab = cx
        .debug_bounds("dock-tab-0")
        .expect("first terminal tab drawn")
        .center();
    right_click(cx, first_tab);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down down down down enter");
    draw(cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 1);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals[0].entity_id()),
        panel.entity_id(),
        "kept the right-clicked tab"
    );

    // 6. Close tab (5th item)
    let at = terminal_tab_at(cx);
    right_click(cx, at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down down down enter");
    draw(cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 0);
    assert!(ws.read_with(cx, |ws, _| ws.terminals_empty.is_some()));
}

#[gpui::test]
fn tab_context_menu_terminal_close_others_confirms_busy_shell(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread_with(&f, true, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "first terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "second terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    let first = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let second = ws.read_with(cx, |ws, _| ws.terminals[1].clone());
    run_sleep(&second, 0, cx);
    draw(cx);

    // Right-click tab 0 and choose Close other tabs -> must ask before killing busy tab 1.
    let first_tab = cx
        .debug_bounds("dock-tab-0")
        .expect("first terminal tab drawn")
        .center();
    right_click(cx, first_tab);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down down down down enter");
    draw(cx);
    assert!(dialog_open(cx), "busy shell confirmation dialog opens");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals.len()),
        2,
        "busy shell is not killed silently"
    );

    // Confirming closes the busy tab and keeps tab 0.
    ws.update_in(cx, |ws, window, cx| {
        ws.confirm_pending_terminal_close(window, cx)
    });
    draw(cx);
    assert!(!dialog_open(cx));
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 1);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals[0].entity_id()),
        first.entity_id()
    );
}

#[gpui::test]
fn tab_context_menu_right_panel_respects_closable_and_unsaved_preview(cx: &mut TestAppContext) {
    let f = fixture();
    std::fs::write(f.project.join("notes.txt"), "hello\n").expect("write");
    let (ws, _thread, cx) = open_thread_with(&f, true, cx);
    let root = f.project.clone();

    // Open Changes, Devices, and Preview -> right dock tabs: [0: Surfaces, 1: Changes, 2: Devices, 3: Preview]
    cx.dispatch_action(OpenThreadChanges);
    cx.dispatch_action(OpenDevices);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_preview(root, "notes.txt".into(), None, window, cx)
    });
    cx.run_until_parked();
    let preview = ws
        .read_with(cx, |ws, _| ws.preview_tab())
        .expect("preview tab open");
    // Make Preview dirty (unsaved edits).
    preview.update_in(cx, |p, window, cx| p.toggle_edit(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("x");
    draw(cx);
    assert!(preview.read_with(cx, |p, _| p.unsaved()));
    assert_eq!(ws.read_with(cx, |ws, cx| ws.right_tab_count(cx)), 4);

    // 1. Right-click Surfaces (dock-tab-0): menu paints, Escape dismisses, Surfaces stays unclosable.
    let surfaces_at = cx.debug_bounds("dock-tab-0").expect("surfaces tab").center();
    right_click(cx, surfaces_at);
    assert!(ws.read_with(cx, |ws, _| ws.tab_menu.is_open()));
    assert_menu_painted(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!ws.read_with(cx, |ws, _| ws.tab_menu.is_open()));

    // 2. Right-click Changes (dock-tab-1) -> Close tab (1st item).
    let changes_at = cx.debug_bounds("dock-tab-1").expect("changes tab").center();
    right_click(cx, changes_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert_eq!(ws.read_with(cx, |ws, cx| ws.right_tab_count(cx)), 3);

    // 3. Right-click dirty Preview (now dock-tab-2) -> Close tab must ask Save/Discard/Cancel.
    let preview_at = cx.debug_bounds("dock-tab-2").expect("preview tab").center();
    right_click(cx, preview_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert!(dialog_open(cx), "closing dirty preview asks first");
    assert_eq!(ws.read_with(cx, |ws, cx| ws.right_tab_count(cx)), 3);
    // Cancel keeps Preview and its edits.
    preview.update_in(cx, |p, window, cx| p.answer_leave(None, window, cx));
    draw(cx);
    assert!(!dialog_open(cx));
    assert!(preview.read_with(cx, |p, _| p.unsaved()));
    assert_eq!(ws.read_with(cx, |ws, cx| ws.right_tab_count(cx)), 3);

    // 4. Right-click Devices (dock-tab-1) -> Close all tabs (3rd item):
    //    Devices closes immediately, dirty Preview asks, Discard closes it, Surfaces stays.
    let devices_at = cx.debug_bounds("dock-tab-1").expect("devices tab").center();
    right_click(cx, devices_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down enter");
    draw(cx);
    assert!(dialog_open(cx), "Close all tabs prompts for dirty preview");
    assert_eq!(
        ws.read_with(cx, |ws, cx| ws.right_tab_count(cx)),
        2,
        "clean Devices closed, Surfaces + dirty Preview remain"
    );
    preview.update_in(cx, |p, window, cx| p.answer_leave(Some(false), window, cx));
    draw(cx);
    assert!(!dialog_open(cx));
    assert_eq!(
        ws.read_with(cx, |ws, cx| ws.right_tab_count(cx)),
        1,
        "only unclosable Surfaces tab remains"
    );
}
