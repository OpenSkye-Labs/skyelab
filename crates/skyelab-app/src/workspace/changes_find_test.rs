//! Acceptance: mod+F in a focused Changes tab opens the find bar; it counts matches across the diff, steps with
//! Enter / Shift-Enter and closes with Esc.

use super::changes::ChangesView;
use super::changes_model::{ChangedFile, Changes, LineKind, Row};
use gpui::{Focusable as _, TestAppContext};
use skyelab_git::ChangeKind;
use std::path::PathBuf;

const FIND: &str = if cfg!(target_os = "macos") {
    "cmd-f"
} else {
    "ctrl-f"
};

fn diff() -> Changes {
    let line = |text: &str| Row::Line {
        old: None,
        new: Some(1),
        kind: LineKind::Added,
        text: text.into(),
    };
    Changes {
        files: vec![ChangedFile {
            path: PathBuf::from("src/main.rs"),
            old_path: None,
            kind: ChangeKind::Added,
            binary: false,
            first_row: 0,
        }],
        rows: vec![
            Row::File(0),
            line("fn main() {}"),
            line("// todo"),
            line("main again"),
        ],
        ..Changes::default()
    }
}

#[gpui::test]
fn find_in_diff_counts_steps_and_closes(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let (view, cx) = cx
        .add_window_view(|window, cx| ChangesView::new(path, None, "app".into(), None, window, cx));
    cx.run_until_parked();
    view.update(cx, |v, _| v.set_loaded(diff()));
    view.update_in(cx, |v, window, cx| v.focus_handle(cx).focus(window, cx));

    cx.simulate_keystrokes(FIND);
    assert!(
        view.read_with(cx, |v, _| v.find.input.is_some()),
        "mod+F opens the bar"
    );
    cx.simulate_keystrokes("m a i n");
    // "src/main.rs" on the file header, "main" twice in the lines.
    assert_eq!(view.read_with(cx, |v, _| v.find.matches.clone()), [0, 1, 3]);
    cx.simulate_keystrokes("enter");
    assert_eq!(view.read_with(cx, |v, _| v.find.active), 1);
    cx.simulate_keystrokes("shift-enter shift-enter");
    assert_eq!(
        view.read_with(cx, |v, _| v.find.active),
        2,
        "back past the first wraps"
    );
    cx.simulate_keystrokes("escape");
    assert!(
        view.read_with(cx, |v, _| v.find.input.is_none()),
        "Esc closes it"
    );
}
