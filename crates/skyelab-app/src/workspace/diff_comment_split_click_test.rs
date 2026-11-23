//! In the split layout a press on a removed line's gutter, with the pointer moving inside that row, picks that line
//! alone, and a drag down the left gutter stays on the old lines.

use crate::workspace::changes::ChangesView;
use crate::workspace::changes_model::{ChangedFile, Changes, LineKind, Row};
use crate::workspace::worktree_thread_test::{fixture, open_thread};
use gpui::{Modifiers, MouseButton, point, px};
use skyelab_git::ChangeKind;
use std::path::PathBuf;

fn line(old: Option<u32>, new: Option<u32>, kind: LineKind, text: &str) -> Row {
    Row::Line {
        old,
        new,
        kind,
        text: text.into(),
    }
}

#[gpui::test]
fn a_left_gutter_pick_in_split_stays_on_the_old_lines(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (_ws, thread, cx) = open_thread(&f, cx);
    let dir = f.project.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ChangesView::new(dir, None, "app".into(), Some(&thread), window, cx)
    });
    cx.run_until_parked();
    let changes = Changes {
        files: vec![ChangedFile {
            path: PathBuf::from("src/a.rs"),
            old_path: None,
            kind: ChangeKind::Modified,
            binary: false,
            first_row: 0,
        }],
        rows: vec![
            Row::File(0),
            Row::Hunk("@@ -10,4 +10,4 @@".into()),
            line(Some(10), Some(10), LineKind::Context, "fn main() {"),
            line(Some(11), None, LineKind::Removed, "    old();"),
            line(Some(12), None, LineKind::Removed, "    older();"),
            line(None, Some(11), LineKind::Added, "    new();"),
            line(None, Some(12), LineKind::Added, "    newer();"),
            line(Some(13), Some(13), LineKind::Context, "}"),
        ],
        ..Default::default()
    };
    view.update(cx, |v, cx| {
        v.set_loaded(changes);
        cx.notify();
    });
    view.update(cx, |v, cx| v.set_prefs(|p| p.split = true, cx));
    cx.run_until_parked();
    let at = cx
        .debug_bounds("diff-gutter-l-3")
        .expect("left gutter drawn")
        .center();
    let nudge = at + point(px(1.), px(0.));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(nudge, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(nudge, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let rows = view.read_with(cx, |v, _| v.draft.as_ref().map(|d| d.rows()));
    assert_eq!(rows, Some(3..4), "picked the removed line only");

    // A drag down the left gutter over both pairs takes the two removed lines, not the added ones.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.draft.is_none()));
    let (from, to) = (
        cx.debug_bounds("diff-gutter-l-3").expect("drawn").center(),
        cx.debug_bounds("diff-gutter-l-4").expect("drawn").center(),
    );
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let rows = view.read_with(cx, |v, _| v.draft.as_ref().map(|d| d.rows()));
    assert_eq!(rows, Some(3..5), "the old lines only");
}
