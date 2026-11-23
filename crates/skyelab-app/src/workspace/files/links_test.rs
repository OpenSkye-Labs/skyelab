//! File links in replies: what gets linked, what stays text, and a click opening the preview at the line.

use super::super::thread::ThreadViewEvent;
use super::super::timeline_text::file_link;
use super::super::worktree_thread_test::{fixture, open_thread, wait_until};
use super::{link_line, linkify};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    if cfg!(windows) {
        "C:/repo".into()
    } else {
        "/repo".into()
    }
}

#[test]
fn linkify_links_paths_in_the_folder() {
    let r = root();
    assert_eq!(
        linkify("see src/a.rs:12, then", &r),
        "see [src/a.rs:12](src/a.rs:12), then"
    );
    assert_eq!(
        linkify("(src/a.rs:3:4)", &r),
        "([src/a.rs:3:4](src/a.rs:3:4))"
    );
    assert_eq!(
        linkify("open `src/a.rs`", &r),
        "open [`src/a.rs`](src/a.rs)"
    );
    let abs = format!("{}/lib.rs:9", r.display());
    assert_eq!(linkify(&abs, &r), format!("[{abs}]({abs})"));
    // A bare path needs a line to count; prose with colons and code that isn't a path stay as they are.
    for same in [
        "src/a.rs",
        "at 12:30 today",
        "`Vec::new()`",
        "`x.len()`",
        "https://x.dev/a.rs:3",
        "[docs](src/a.rs:2)",
    ] {
        assert_eq!(linkify(same, &r), same);
    }
}

#[test]
fn linkify_leaves_outside_paths_as_text() {
    let r = root();
    for same in [
        "../up.rs:3",
        "/etc/passwd:1",
        "~/.ssh/id:1",
        "`../x.rs`",
        "`/etc/hosts`",
    ] {
        assert_eq!(linkify(same, &r), same);
    }
    // A markdown link to a file elsewhere keeps only its label.
    assert_eq!(
        linkify("read [the key](/etc/ssh/key) now", &r),
        "read the key now"
    );
    assert_eq!(linkify("[up](../x.rs:2)", &r), "up");
    // Web links stay links.
    assert_eq!(
        linkify("[site](https://x.dev)", &r),
        "[site](https://x.dev)"
    );
}

#[test]
fn linkify_skips_fenced_code() {
    let text = "```\nsrc/a.rs:1\n```\nsrc/b.rs:2\n";
    assert_eq!(
        linkify(text, &root()),
        "```\nsrc/a.rs:1\n```\n[src/b.rs:2](src/b.rs:2)\n"
    );
}

#[test]
fn link_line_reads_the_line() {
    assert_eq!(link_line("src/a.rs:12"), Some(12));
    assert_eq!(link_line("src/a.rs:12:3"), Some(12));
    assert_eq!(link_line("src/a.rs"), None);
    assert_eq!(file_link("src/a.rs:12").as_deref(), Some("src/a.rs"));
}

#[gpui::test]
fn reply_link_opens_the_preview_at_the_line(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    std::fs::write(f.project.join("notes.txt"), "a\nb\nc\nd\n").expect("write");
    let (ws, thread, cx) = open_thread(&f, cx);
    thread.update(cx, |_, cx| {
        cx.emit(ThreadViewEvent::Preview("notes.txt".into(), Some(4)))
    });
    let tab = ws
        .read_with(cx, |ws, _| {
            ws.files.preview.as_ref().and_then(|p| p.upgrade())
        })
        .expect("preview");
    wait_until(cx, "notes.txt at line 4", |cx| {
        tab.read_with(cx, |t, cx| {
            let e = t.editor.read(cx);
            e.value().starts_with("a\nb") && e.cursor_position().line == 3
        })
    });
    assert_eq!(tab.read_with(cx, |t, _| t.root.clone()), f.project);
    assert_eq!(
        tab.read_with(cx, |t, _| t.path.clone()),
        Path::new("notes.txt")
    );
}
