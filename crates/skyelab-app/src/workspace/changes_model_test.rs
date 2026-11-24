//! The Changes model against a real temp repo: every kind of change, line numbers, binaries.

use super::changes_model::{LineKind, Row, load};
use skyelab_git::ChangeKind;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs git for test setup with a fixed identity, so the user's config can't break commits.
pub(super) fn git(dir: &Path, args: &[&str]) {
    let out = skyelab_core::process::output(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args),
        None,
    )
    .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A canonical temp dir holding `repo/`: a repo on `main` with one commit.
pub(super) fn repo(files: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path())
        .expect("canonicalize")
        .join("repo");
    std::fs::create_dir(&root).expect("mkdir");
    git(&root, &["init", "-q", "-b", "main"]);
    for (name, body) in files {
        std::fs::write(root.join(name), body).expect("write");
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    (tmp, root)
}

#[test]
fn lists_every_kind_of_change_with_line_numbers() {
    let (_tmp, root) = repo(&[
        ("a.txt", b"one\ntwo\nthree\n"),
        ("gone.txt", b"bye\n"),
        ("old.txt", b"alpha\nbeta\ngamma\ndelta\n"),
        ("blob.bin", b"\x00\x01\x02"),
    ]);
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").expect("modify");
    std::fs::remove_file(root.join("gone.txt")).expect("delete");
    git(&root, &["mv", "old.txt", "new.txt"]);
    std::fs::write(root.join("blob.bin"), b"\x00\x09\x09\x09").expect("binary change");
    std::fs::write(root.join("fresh.txt"), "hello\nworld\n").expect("untracked add");

    let c = load(&root, None).expect("load");
    let kinds: Vec<(String, ChangeKind, bool)> = c
        .files
        .iter()
        .map(|f| (f.path.display().to_string(), f.kind, f.binary))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("a.txt".into(), ChangeKind::Modified, false),
            ("blob.bin".into(), ChangeKind::Modified, true),
            ("fresh.txt".into(), ChangeKind::Added, false),
            ("gone.txt".into(), ChangeKind::Deleted, false),
            ("new.txt".into(), ChangeKind::Renamed, false),
        ]
    );
    let renamed = &c.files[4];
    assert_eq!(renamed.old_path.as_deref(), Some(Path::new("old.txt")));

    // Each file's header row is where the file list jumps to.
    for (i, f) in c.files.iter().enumerate() {
        assert_eq!(c.rows[f.first_row], Row::File(i));
    }
    let rows_of = |i: usize| {
        let end = c.files.get(i + 1).map_or(c.rows.len(), |n| n.first_row);
        &c.rows[c.files[i].first_row + 1..end]
    };
    let line = |old, new, kind, text: &str| Row::Line {
        old,
        new,
        kind,
        text: text.into(),
    };

    assert_eq!(
        rows_of(0),
        &[
            Row::Hunk("@@ -1,3 +1,3 @@ ".into()),
            line(Some(1), Some(1), LineKind::Context, "one"),
            line(Some(2), None, LineKind::Removed, "two"),
            line(None, Some(2), LineKind::Added, "TWO"),
            line(Some(3), Some(3), LineKind::Context, "three"),
        ]
    );
    assert_eq!(rows_of(1), &[Row::Placeholder("Binary file")]);
    assert_eq!(
        rows_of(2),
        &[
            line(None, Some(1), LineKind::Added, "hello"),
            line(None, Some(2), LineKind::Added, "world"),
        ]
    );
    assert_eq!(
        rows_of(3)[1..],
        [line(Some(1), None, LineKind::Removed, "bye")]
    );
    // A pure rename has no hunks, and shows nothing but its header.
    assert!(rows_of(4).is_empty(), "{:?}", rows_of(4));
}

#[test]
fn untracked_binary_and_links_show_placeholders() {
    let (_tmp, root) = repo(&[("a.txt", b"a\n")]);
    std::fs::write(root.join("img.png"), b"\x89PNG\x00\x00").expect("binary");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/hosts", root.join("link")).expect("symlink");

    let c = load(&root, None).expect("load");
    let png = c
        .files
        .iter()
        .find(|f| f.path == Path::new("img.png"))
        .expect("png listed");
    assert!(png.binary);
    assert_eq!(c.rows[png.first_row + 1], Row::Placeholder("Binary file"));
    #[cfg(unix)]
    {
        let link = c
            .files
            .iter()
            .find(|f| f.path == Path::new("link"))
            .expect("link listed");
        // Never the target's contents: a link can point at any file on disk.
        assert_eq!(
            c.rows[link.first_row + 1],
            Row::Placeholder("Not a regular file")
        );
    }
}

/// A worktree's diff is against its base, so it lists files already committed; Commit counts only what isn't.
#[test]
fn counts_only_uncommitted_files_against_a_base() {
    let (_tmp, root) = repo(&[("a.txt", b"one\n")]);
    std::fs::write(root.join("done.txt"), "committed\n").expect("write");
    git(&root, &["add", "done.txt"]);
    git(&root, &["commit", "-q", "-m", "work"]);
    let c = load(&root, Some("main~1")).expect("load");
    assert_eq!(c.files.len(), 1, "the committed file is still in the diff");
    assert_eq!(c.uncommitted, 0, "nothing is left to commit");

    std::fs::write(root.join("a.txt"), "two\n").expect("modify");
    std::fs::write(root.join("new.txt"), "fresh\n").expect("untracked");
    let c = load(&root, Some("main~1")).expect("load");
    assert_eq!((c.files.len(), c.uncommitted), (3, 2));
    assert_eq!(load(&root, None).expect("load").uncommitted, 2);

    // A committed file deleted again: the base diff no longer lists it, but there is a change to commit.
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "more"]);
    std::fs::remove_file(root.join("done.txt")).expect("rm");
    assert_eq!(load(&root, Some("main~2")).expect("load").uncommitted, 1);
}
