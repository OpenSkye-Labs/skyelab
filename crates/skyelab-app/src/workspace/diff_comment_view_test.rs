//! Acceptance: a press on a line's gutter and a drag to another line open a draft under them, in the unified and
//! the split layout; ⌘/Ctrl-Enter keeps it as a comment that follows its lines across a reload; Escape drops a draft;
//! "Send" puts the comments in the thread's composer as one chip.

use crate::workspace::changes::ChangesView;
use crate::workspace::changes_model::{ChangedFile, Changes, LineKind, Row, Scope};
use crate::workspace::thread::composer::composer_context::ChipKind;
use crate::workspace::worktree_thread_test::{fixture, open_thread};
use gpui::{Modifiers, MouseButton, VisualTestContext};
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

fn diff(shift: u32) -> Changes {
    let file = |path: &str, first_row| ChangedFile {
        path: PathBuf::from(path),
        old_path: None,
        kind: ChangeKind::Modified,
        binary: false,
        first_row,
    };
    let mut rows = vec![Row::File(0), Row::Hunk("@@ -10,3 +10,3 @@".into())];
    rows.extend((0..shift).map(|i| line(None, Some(10 + i), LineKind::Added, "use x;")));
    rows.extend([
        line(Some(10), Some(10 + shift), LineKind::Context, "fn main() {"),
        line(Some(11), None, LineKind::Removed, "    old();"),
        line(None, Some(11 + shift), LineKind::Added, "    new();"),
        line(Some(12), Some(12 + shift), LineKind::Context, "}"),
    ]);
    let b = rows.len();
    rows.extend([Row::File(1), line(None, Some(1), LineKind::Added, "b")]);
    Changes {
        files: vec![file("src/a.rs", 0), file("src/b.rs", b)],
        rows,
        ..Default::default()
    }
}

fn gutter(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Point<gpui::Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center()
}

const SAVE: &str = if cfg!(target_os = "macos") {
    "cmd-enter"
} else {
    "ctrl-enter"
};

#[gpui::test]
fn gutter_drag_comments_follow_reloads_and_send_as_one_chip(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (_ws, thread, cx) = open_thread(&f, cx);
    let dir = f.project.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ChangesView::new(dir, None, "app".into(), Some(&thread), window, cx)
    });
    cx.run_until_parked(); // the folder's own (empty) diff loads first
    view.update(cx, |v, cx| {
        v.set_loaded(diff(0));
        cx.notify();
    });

    // Press on line 3's gutter (old(); removed), drag to line 5 (the closing brace), let go.
    let (from, to) = (gutter(cx, "diff-gutter-3"), gutter(cx, "diff-gutter-5"));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.draft.as_ref().map(|d| d.rows())),
        Some(3..6)
    );
    cx.simulate_input("rename new() to run()");
    cx.simulate_keystrokes(SAVE);
    cx.run_until_parked();
    let labels = |cx: &mut VisualTestContext| {
        thread.read_with(cx, |t, _| {
            let r = &t.composer.review;
            r.items
                .iter()
                .map(|c| (c.label(), c.rows.clone()))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(labels(cx), [("src/a.rs:11-12".to_owned(), 3..6)]);
    assert!(view.read_with(cx, |v, _| v.draft.is_none()));

    // A reload with two lines added above: the comment moves down with its lines.
    view.update(cx, |v, cx| {
        let moved = diff(2);
        v.reanchor_comments(&moved, cx);
        v.set_loaded(moved);
        cx.notify();
    });
    assert_eq!(labels(cx), [("src/a.rs:13-14".to_owned(), 5..8)]);

    // Split: the removed line's gutter is on the left half, the added one's on the right; a click on the right one
    // comments on that line alone. Escape drops the draft.
    view.update(cx, |v, cx| v.set_prefs(|p| p.split = true, cx));
    let (left, right) = (gutter(cx, "diff-gutter-l-5"), gutter(cx, "diff-gutter-6"));
    assert!(left.x < right.x, "{left:?} {right:?}");
    assert_eq!(left.y, right.y, "one split row");
    cx.simulate_mouse_down(right, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(right, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.draft.as_ref().map(|d| d.rows())),
        Some(6..7)
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.draft.is_none()));

    // Send: one chip in the composer with the lines and the note; the diff's comments are gone.
    view.update(cx, |v, cx| v.send_comments(cx));
    thread.read_with(cx, |t, _| {
        assert_eq!(t.composer.review.count(Scope::Working), 0);
        let [chip] = &t.composer.contexts[..] else {
            panic!("one chip: {:?}", t.composer.contexts)
        };
        assert_eq!(chip.kind, ChipKind::Review);
        assert_eq!(chip.label.as_ref(), "1 diff comment");
        assert!(
            chip.text.contains(
                "src/a.rs:13-14\n-     old();\n+     new();\n  }\nComment: rename new() to run()"
            ),
            "{}",
            chip.text
        );
    });
}
