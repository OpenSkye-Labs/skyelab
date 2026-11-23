//! Clicking an edit card's file opens the thread's Changes tab scrolled to that file.

use super::thread_model::{Item, ToolItem};
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use serde_json::json;
use std::path::PathBuf;

#[gpui::test]
fn a_card_path_click_reveals_the_file_in_changes(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    std::fs::write(f.project.join("notes.txt"), "changed\n").expect("write");
    let raw = json!({ "file_path": f.project.join("notes.txt"), "old_string": "", "new_string": "changed" });
    thread.update(cx, |v, cx| {
        v.items = vec![Item::Tool(ToolItem {
            id: "e1".into(),
            name: "Edit".into(),
            input: raw.to_string(),
            raw,
            result: Some("ok".into()),
            is_error: false,
        })];
        v.lists.reset(1);
        cx.notify();
    });
    cx.run_until_parked();
    let path = cx
        .debug_bounds("card-path")
        .expect("the card's path is drawn");
    cx.simulate_click(path.center(), gpui::Modifiers::none());
    wait_until(cx, "the Changes tab on notes.txt", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .is_some_and(|c| {
            c.read_with(cx, |c, _| c.active_file.clone()) == Some(PathBuf::from("notes.txt"))
        })
    });
}
