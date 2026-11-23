//! A long comment keeps its note, and a drag up into the previous file stays in the file it started in.

use super::*;
use crate::workspace::changes_model::{ChangedFile, Changes, LineKind, Row, Scope};
use skyelab_git::ChangeKind;

fn file(path: &str, first_row: usize) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        old_path: None,
        kind: ChangeKind::Modified,
        binary: false,
        first_row,
    }
}

fn added(n: u32) -> Row {
    Row::Line {
        old: None,
        new: Some(n),
        kind: LineKind::Added,
        text: "x".repeat(60),
    }
}

#[test]
fn a_long_comment_keeps_its_note_in_the_chip() {
    let mut rows = vec![Row::File(0), Row::Hunk("@@".into())];
    rows.extend((1..=400).map(added));
    let changes = Changes {
        files: vec![file("a.rs", 0)],
        rows,
        ..Default::default()
    };
    let mut c = DiffComments::default();
    c.add(&changes, Scope::Working, 2, 401, "please rename")
        .expect("added");
    let sent = c.chip_text(Scope::Working).expect("fits");
    assert!(sent.contains("Comment: please rename"), "{sent}");
    assert!(
        sent.contains("more lines"),
        "the excerpt is cut, not the note"
    );
    assert_eq!(
        skyelab_core::terminal_context::clip(&sent),
        sent,
        "nothing clipped"
    );
}

#[test]
fn an_upward_drag_into_the_previous_file_stays_in_its_own() {
    let rows = vec![
        Row::File(0),
        added(1),
        added(2),
        Row::File(1),
        added(1),
        added(2),
    ];
    let changes = Changes {
        files: vec![file("a.rs", 0), file("b.rs", 3)],
        rows,
        ..Default::default()
    };
    let mut c = DiffComments::default();
    let id = c.add(&changes, Scope::Working, 5, 1, "n").expect("added");
    let item = c.items.iter().find(|i| i.id == id).expect("item");
    assert_eq!(
        (item.path.to_str(), item.rows.clone()),
        (Some("b.rs"), 4..6)
    );
}
