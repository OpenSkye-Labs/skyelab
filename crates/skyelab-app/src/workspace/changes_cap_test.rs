//! A huge diff stays a bounded number of rows: past the per-file or total line cap a file shows one
//! "diff too large" row instead of its lines.

use super::changes_model::{Changes, Row, load};
use super::changes_model_test::repo;
use std::path::Path;

fn lines(n: usize, tag: &str) -> String {
    (0..n).map(|i| format!("{tag} {i}\n")).collect()
}

fn rows_of<'a>(c: &'a Changes, name: &str) -> &'a [Row] {
    let i = c
        .files
        .iter()
        .position(|f| f.path == Path::new(name))
        .unwrap_or_else(|| panic!("{name} listed"));
    let end = c.files.get(i + 1).map_or(c.rows.len(), |f| f.first_row);
    &c.rows[c.files[i].first_row + 1..end]
}

fn line_rows(rows: &[Row]) -> usize {
    rows.iter()
        .filter(|r| matches!(r, Row::Line { .. }))
        .count()
}

#[test]
fn a_file_past_the_line_cap_is_one_too_large_row() {
    let (_tmp, root) = repo(&[("big.txt", b"old\n"), ("small.txt", b"a\n")]);
    std::fs::write(root.join("big.txt"), lines(6_000, "new")).expect("rewrite");
    std::fs::write(root.join("small.txt"), "b\n").expect("modify");
    std::fs::write(root.join("dump.txt"), lines(6_000, "dump")).expect("untracked");

    let c = load(&root, None).expect("load");
    // 6,000 added plus the one removed line.
    assert_eq!(rows_of(&c, "big.txt"), &[Row::TooLarge(6_001)]);
    assert_eq!(rows_of(&c, "dump.txt"), &[Row::TooLarge(6_000)]);
    assert_eq!(
        line_rows(rows_of(&c, "small.txt")),
        2,
        "a small file keeps its lines"
    );
}

#[test]
fn the_total_line_cap_holds_across_files() {
    let names: Vec<String> = (0..11).map(|i| format!("f{i:02}.txt")).collect();
    let committed: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x\n"[..])).collect();
    let (_tmp, root) = repo(&committed);
    for n in &names {
        std::fs::write(root.join(n), lines(4_900, n)).expect("rewrite");
    }
    let c = load(&root, None).expect("load");
    let shown = line_rows(&c.rows);
    assert!(shown <= 50_000, "{shown} line rows");
    assert!(shown >= 40_000, "files under the cap still show: {shown}");
    let too_large = c
        .rows
        .iter()
        .filter(|r| matches!(r, Row::TooLarge(_)))
        .count();
    assert!(too_large >= 1, "the files past the total are placeholders");
    assert_eq!(
        line_rows(rows_of(&c, "f00.txt")),
        4_901,
        "the first file shows in full"
    );
}
