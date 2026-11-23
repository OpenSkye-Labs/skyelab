//! A finished turn lists the files its checkpoint diff changed, under its reply; a file opens the Changes tab on that
//! turn. Dividers map to checkpoint turns only while their counts line up.

use super::changes_model::Scope;
use super::thread_items::ItemsView as _;
use super::thread_model::Item;
use super::turn_files::turn_of;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::TestAppContext;

fn done() -> Item {
    Item::Done {
        is_error: false,
        duration_ms: 1,
        cost_usd: 0.,
        total_usd: 0.,
        usage: None,
        at_ms: None,
        error: None,
        copied: false,
    }
}

#[test]
fn turn_numbers_need_matching_counts() {
    let items = vec![
        Item::Prompt("a".into()),
        done(), // 1
        Item::Prompt("b".into()),
        done(), // 3
    ];
    assert_eq!(turn_of(&items, 1, 2, 2), Some(1));
    assert_eq!(turn_of(&items, 3, 2, 2), Some(2));
    assert_eq!(
        turn_of(&items, 3, 1, 1),
        Some(2),
        "the newest divider may wait for its checkpoint"
    );
    assert_eq!(
        turn_of(&items, 1, 1, 2),
        None,
        "a divider that had no checkpoint at the last load never gets one (a turn before the repo existed)"
    );
    assert_eq!(
        turn_of(&items, 1, 3, 2),
        None,
        "a checkpoint without a divider shifts the numbers"
    );
    assert_eq!(turn_of(&items, 1, 0, 0), None);
    assert_eq!(turn_of(&items, 0, 2, 2), None, "not a divider");
}

#[gpui::test]
fn turn_files_list_each_turns_checkpoint_diff(cx: &mut TestAppContext) {
    let f = fixture();
    std::fs::write(f.project.join(".fake-agent-edits"), "").expect("marker");
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    let files = |cx: &mut gpui::VisualTestContext| {
        thread.read_with(cx, |t, _| {
            let done = t
                .items
                .iter()
                .position(|i| matches!(i, Item::Done { .. }))?;
            let (turn, files) = t.turn_files(done)?;
            Some((
                turn,
                files
                    .iter()
                    .map(|f| (f.rel.clone(), f.added, f.removed))
                    .collect::<Vec<_>>(),
            ))
        })
    };
    wait_until(cx, "turn 1's files", |cx| files(cx).is_some());
    assert_eq!(
        files(cx),
        Some((1, vec![("agent-edit.txt".to_owned(), 1, 0)]))
    );
    assert!(cx.debug_bounds("turn-files").is_some(), "the card is drawn");

    // Opened, a file row opens Changes on turn 1 at that file.
    let key = thread.read_with(cx, |t, _| {
        let done = t.items.iter().position(|i| matches!(i, Item::Done { .. }));
        format!("files:{}", t.items.key(done.expect("divider")))
    });
    thread.update(cx, |t, cx| t.toggle_tool(&key, cx));
    cx.run_until_parked();
    // A click on the file row (not a direct call): the row itself must take it.
    let row = cx.debug_bounds("turn-file").expect("file row");
    cx.simulate_click(row.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let changes = ws
        .read_with(cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    wait_until(cx, "turn 1's diff", |cx| {
        changes.read_with(cx, |c, _| {
            c.scope == Scope::Turn(1) && c.changes.as_ref().is_some_and(|c| c.files.len() == 1)
        })
    });
    changes.read_with(cx, |c, _| {
        assert_eq!(
            c.active_file.as_deref(),
            Some(std::path::Path::new("agent-edit.txt")),
            "revealed at the file"
        )
    });
}
