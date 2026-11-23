use super::DiffComments;
use crate::workspace::changes_model::{ChangedFile, Changes, LineKind, Row, Scope};
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

fn file(path: &str, first_row: usize) -> ChangedFile {
    ChangedFile {
        path: PathBuf::from(path),
        old_path: None,
        kind: ChangeKind::Modified,
        binary: false,
        first_row,
    }
}

/// Two files: `src/a.rs` (rows 0..=5) and `src/b.rs` (rows 6..=8).
fn diff() -> Changes {
    Changes {
        files: vec![file("src/a.rs", 0), file("src/b.rs", 6)],
        rows: vec![
            Row::File(0),
            Row::Hunk("@@ -10,3 +10,3 @@".into()),
            line(Some(10), Some(10), LineKind::Context, "fn main() {"),
            line(Some(11), None, LineKind::Removed, "    old();"),
            line(None, Some(11), LineKind::Added, "    new();"),
            line(Some(12), Some(12), LineKind::Context, "}"),
            Row::File(1),
            Row::Hunk("@@ -1 +1 @@".into()),
            line(None, Some(1), LineKind::Added, "b"),
        ],
        ..Default::default()
    }
}

#[test]
fn a_range_stays_in_its_file_and_the_chip_names_lines_excerpt_and_note() {
    let changes = diff();
    let mut c = DiffComments::default();
    // A drag from row 3 into the next file stops at the end of `src/a.rs`.
    let id = c
        .add(&changes, Scope::Working, 3, 8, "rename new() to run()")
        .expect("lines");
    let item = c.items.iter().find(|i| i.id == id).expect("added");
    assert_eq!(item.path, PathBuf::from("src/a.rs"));
    assert_eq!(item.rows, 3..6);
    assert_eq!(item.label(), "src/a.rs:11-12");
    // Rows without lines (a header) make no comment.
    assert!(c.add(&changes, Scope::Working, 0, 1, "x").is_none());
    // A removed line alone names its old number.
    let id = c.add(&changes, Scope::Working, 3, 3, "why?").expect("line");
    let removed = c.items.iter().find(|i| i.id == id).expect("added");
    assert_eq!(removed.label(), "src/a.rs:11 (old)");

    let text = c.chip_text(Scope::Working).expect("fits");
    assert!(
        text.contains(
            "src/a.rs:11-12\n-     old();\n+     new();\n  }\nComment: rename new() to run()"
        ),
        "{text}"
    );
    assert!(
        text.contains("src/a.rs:11 (old)\n-     old();\nComment: why?"),
        "{text}"
    );
    assert_eq!(c.count(Scope::Working), 2);
    assert_eq!(
        c.count(Scope::SinceStart),
        0,
        "comments belong to the diff they were made on"
    );
}

#[test]
fn comments_follow_their_lines_across_a_reload_and_drop_when_the_lines_are_gone() {
    let mut c = DiffComments::default();
    let kept = c.add(&diff(), Scope::Working, 3, 4, "keep").expect("lines");
    let gone = c.add(&diff(), Scope::Working, 8, 8, "gone").expect("line");

    // The reload: two lines were added above the hunk in a.rs, and b.rs's line is no longer changed.
    let mut moved = diff();
    moved.rows.truncate(6);
    moved.files.truncate(1);
    moved
        .rows
        .insert(2, line(None, Some(10), LineKind::Added, "use x;"));
    moved
        .rows
        .insert(3, line(None, Some(11), LineKind::Added, "use y;"));
    if let Row::Line { new, .. } = &mut moved.rows[6] {
        *new = Some(13);
    }
    c.reanchor(&moved, Scope::Working);

    let item = c.items.iter().find(|i| i.id == kept).expect("kept");
    assert_eq!(item.rows, 5..7, "it moved down with its lines");
    assert_eq!(item.label(), "src/a.rs:13", "and names their new numbers");
    assert!(c.items.iter().all(|i| i.id != gone), "its line is gone");
    assert_eq!(c.dropped, 1);
    assert_eq!(
        c.dropped_note().as_deref(),
        Some("1 comment was dropped: its lines are no longer in the diff.")
    );
    // Another scope's reload leaves them alone.
    c.reanchor(&Changes::default(), Scope::SinceStart);
    assert_eq!(c.count(Scope::Working), 1);
}
