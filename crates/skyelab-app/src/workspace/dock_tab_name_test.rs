//! Each dock tab in the a11y tree is named by its own title, not by its panel type.

use super::NewTerminal;
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use gpui::Role;
use gpui_base::test_support::snapshots;

#[gpui::test]
fn each_terminal_tab_is_named_by_its_title(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    cx.run_until_parked();
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "two terminals", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    cx.run_until_parked();
    let titles: Vec<String> = ws.read_with(cx, |ws, cx| {
        ws.terminals
            .iter()
            .map(|t| t.read(cx).tab_label().to_string())
            .collect()
    });
    let tabs: Vec<String> = cx
        .update(|window, _| snapshots(window))
        .into_iter()
        .filter(|s| s.role() == Some(Role::Tab) && s.visible())
        .filter_map(|s| s.label().map(str::to_owned))
        .collect();
    for title in &titles {
        assert!(
            tabs.contains(title),
            "no dock tab named {title:?}; the a11y tree's tabs are {tabs:?}"
        );
    }
}
