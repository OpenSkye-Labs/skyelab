//! Acceptance: a Changes tab with 10,000 changed files builds only the visible sidebar rows, and a very long diff
//! line is clipped for display.

use super::changes::{ChangesView, file_rows_built};
use super::changes_model::{self, ChangedFile, Changes, Row};
use gpui::{AppContext as _, IntoElement as _, TestAppContext, point, px, size};
use skyelab_git::ChangeKind;
use std::path::PathBuf;

#[gpui::test]
fn ten_thousand_changed_files_build_only_visible_sidebar_rows(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let mut changes = Changes::default();
    for i in 0..10_000 {
        changes.files.push(ChangedFile {
            path: PathBuf::from(format!("src/file{i}.rs")),
            old_path: None,
            kind: ChangeKind::Added,
            binary: false,
            first_row: changes.rows.len(),
        });
        changes.rows.push(Row::File(i));
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| ChangesView::new(path, None, "app".into(), None, window, cx))
    });
    cx.run_until_parked();
    view.update(cx, |v, _| v.set_loaded(changes));
    let before = file_rows_built();
    cx.draw(point(px(0.), px(0.)), size(px(800.), px(600.)), |_, _| {
        view.clone().into_any_element()
    });
    let built = file_rows_built() - before;
    assert!(built > 0 && built < 100, "sidebar built {built} rows");
}

#[test]
fn a_very_long_diff_line_is_clipped_for_display() {
    let dir = tempfile::tempdir().expect("tempdir");
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env_remove("GIT_DIR")
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("min.js"), "x".repeat(50_000)).expect("write");
    let changes = changes_model::load(dir.path(), None).expect("load");
    let text = changes
        .rows
        .iter()
        .find_map(|r| match r {
            Row::Line { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("a line row");
    assert!(
        text.chars().count() <= 2_001,
        "{} chars",
        text.chars().count()
    );
    assert!(text.ends_with('…'));
}
