//! A reload (a turn ended, a commit) and a layout toggle keep the reader's place in the diff.

use super::changes::ChangesView;
use super::changes_model::{ChangedFile, Changes, LineKind, Row};
use gpui::TestAppContext;
use skyelab_git::ChangeKind;
use std::path::PathBuf;

fn diff() -> Changes {
    let mut rows = vec![Row::File(0)];
    rows.extend((0..200).map(|i| Row::Line {
        old: None,
        new: Some(i),
        kind: LineKind::Added,
        text: format!("l{i}"),
    }));
    Changes {
        files: vec![ChangedFile {
            path: PathBuf::from("a.rs"),
            old_path: None,
            kind: ChangeKind::Added,
            binary: false,
            first_row: 0,
        }],
        rows,
        ..Changes::default()
    }
}

#[gpui::test]
fn reload_and_toggle_keep_diff_scroll(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let (view, cx) = cx.add_window_view(|w, cx| ChangesView::new(path, None, "app".into(), None, w, cx));
    cx.run_until_parked();
    view.update(cx, |v, _| {
        v.set_loaded(diff());
        v.scroll_to_row(50);
    });
    view.update(cx, |v, _| v.set_loaded(diff()));
    assert_eq!(view.read_with(cx, |v, _| v.top_row()), 50, "kept across a reload");
    view.update(cx, |v, cx| v.set_prefs(|p| p.split = true, cx));
    assert_eq!(view.read_with(cx, |v, _| v.top_row()), 50, "kept across split");
}
