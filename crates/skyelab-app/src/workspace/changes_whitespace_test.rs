//! Hiding whitespace never hides a file: one whose only change is whitespace stays listed (Commit still takes it),
//! with a note instead of lines.

use super::changes_model::{self, Row, Scope};
use super::changes_model_test::repo;
use std::path::Path;

#[test]
fn whitespace_only_file_stays_listed_when_hidden() {
    let (_tmp, root) = repo(&[("a.rs", b"fn a() {}\n"), ("b.rs", b"fn b() {}\n")]);
    std::fs::write(root.join("a.rs"), "fn a() {}   \n").expect("write");
    std::fs::write(root.join("b.rs"), "fn c() {}\n").expect("write");
    let c = changes_model::load_scope(&root, None, Scope::Working, None, true).expect("load");
    let paths: Vec<&Path> = c.files.iter().map(|f| f.path.as_path()).collect();
    assert_eq!(
        paths,
        [Path::new("a.rs"), Path::new("b.rs")],
        "both files listed"
    );
    let a_rows = &c.rows[c.files[0].first_row + 1..c.files[1].first_row];
    assert!(
        matches!(a_rows, [Row::Placeholder(_)]),
        "a whitespace-only file shows a note, no lines: {a_rows:?}"
    );
    assert!(
        c.rows
            .iter()
            .any(|r| matches!(r, Row::Line { text, .. } if text == "fn c() {}"))
    );
}
