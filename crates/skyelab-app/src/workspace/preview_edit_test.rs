//! Acceptance: a text preview edits after its Edit toggle, mod+S saves it, a dot marks unsaved edits, and leaving
//! them (Esc, Edit off, another file, the tab's Close, quitting) asks Save / Discard / Cancel. A file that changed on
//! disk is never overwritten silently: the tab offers Reload or Overwrite. Images have no Edit; find still works.

use super::*;
use crate::workspace::quit_guard::{Blocker, blockers};
use crate::workspace::worktree_thread_test::{fixture, open_thread_with};
use gpui::VisualTestContext;

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn preview(
    ws: &Entity<crate::workspace::Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<PreviewPanel> {
    ws.read_with(cx, |ws, _| {
        ws.files.preview.as_ref().and_then(|p| p.upgrade())
    })
    .expect("the Preview tab")
}

/// The fixture's project with `name` holding `text`, open in the Preview tab.
fn open<'a>(
    name: &str,
    text: &str,
    cx: &'a mut gpui::TestAppContext,
) -> (
    crate::workspace::worktree_thread_test::Fixture,
    Entity<crate::workspace::Workspace>,
    Entity<PreviewPanel>,
    &'a mut VisualTestContext,
) {
    let f = fixture();
    std::fs::write(f.project.join(name), text).expect("write");
    let (ws, _thread, cx) = open_thread_with(&f, true, cx); // a Root window: the question is a dialog
    let root = f.project.clone();
    let path = name.to_owned();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_preview(root, path.into(), None, window, cx)
    });
    cx.run_until_parked();
    let p = preview(&ws, cx);
    (f, ws, p, cx)
}

fn edit_on(p: &Entity<PreviewPanel>, cx: &mut VisualTestContext) {
    p.update_in(cx, |p, window, cx| p.toggle_edit(window, cx));
    cx.run_until_parked();
}

fn text(p: &Entity<PreviewPanel>, cx: &mut VisualTestContext) -> String {
    p.read_with(cx, |p, cx| p.editor.read(cx).value().to_string())
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(crate::ui::has_active_dialog)
}

#[gpui::test]
fn only_text_previews_edit(cx: &mut gpui::TestAppContext) {
    let (f, ws, p, cx) = open("notes.txt", "hello\n", cx);
    let root = f.project.clone();
    assert!(p.read_with(cx, |p, _| p.can_edit()), "a text file can edit");
    // Read-only until Edit is on: typing changes nothing.
    p.update_in(cx, |p, window, cx| {
        p.editor.update(cx, |e, cx| e.focus(window, cx))
    });
    cx.simulate_keystrokes("x");
    cx.run_until_parked();
    assert_eq!(text(&p, cx), "hello\n", "read-only before Edit");

    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode");
    std::fs::write(root.join("shot.png"), png.into_inner()).expect("write");
    std::fs::write(root.join("blob.bin"), [0u8, 1, 2]).expect("write");
    for name in ["shot.png", "blob.bin"] {
        let r = root.clone();
        ws.update_in(cx, |ws, window, cx| {
            ws.open_preview(r, name.into(), None, window, cx)
        });
        cx.run_until_parked();
        assert!(!p.read_with(cx, |p, _| p.can_edit()), "{name} has no Edit");
        p.update_in(cx, |p, window, cx| p.toggle_edit(window, cx));
        assert!(
            !p.read_with(cx, |p, _| p.edit.on),
            "{name}: the toggle does nothing"
        );
    }
}

#[gpui::test]
fn editing_marks_dirty_and_saves(cx: &mut gpui::TestAppContext) {
    let (f, _ws, p, cx) = open("notes.txt", "hello\n", cx);
    let root = f.project.clone();
    edit_on(&p, cx);
    cx.simulate_keystrokes("a b");
    cx.run_until_parked();
    assert!(
        p.read_with(cx, |p, _| p.unsaved()),
        "typing leaves unsaved edits"
    );
    assert!(
        p.read_with(cx, |p, _| p.title_text().ends_with(" •")),
        "the dot marks it"
    );
    assert!(
        !p.read_with(cx, gpui_component::dock::BasePanel::closable),
        "no × while dirty"
    );
    cx.simulate_keystrokes("backspace backspace");
    cx.run_until_parked();
    assert!(
        !p.read_with(cx, |p, _| p.unsaved()),
        "typed back to the saved text: clean"
    );

    cx.simulate_keystrokes("z");
    cx.simulate_keystrokes(&format!("{M}-s"));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).expect("read"),
        "zhello\n"
    );
    assert!(!p.read_with(cx, |p, _| p.unsaved()), "saved: clean");
    assert!(
        p.read_with(cx, |p, _| p.title_text() == p.name()),
        "the dot goes"
    );
    assert!(
        p.read_with(cx, |p, _| p.edit.on),
        "still editing after a save"
    );
}

#[gpui::test]
fn leaving_with_edits_asks(cx: &mut gpui::TestAppContext) {
    let (f, ws, p, cx) = open("notes.txt", "hello\n", cx);
    let root = f.project.clone();
    std::fs::write(root.join("other.txt"), "other\n").expect("write");
    edit_on(&p, cx);
    cx.simulate_keystrokes("x");
    cx.run_until_parked();

    // Esc asks; Cancel keeps editing and the edits.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(dialog_open(cx), "Esc with edits asks");
    p.update_in(cx, |p, window, cx| p.answer_leave(None, window, cx));
    cx.run_until_parked();
    assert!(
        p.read_with(cx, |p, _| p.edit.on && p.unsaved()),
        "Cancel keeps both"
    );

    // Edit off asks; Discard drops the edits and leaves editing; nothing is written.
    p.update_in(cx, |p, window, cx| p.toggle_edit(window, cx));
    cx.run_until_parked();
    assert!(dialog_open(cx), "Edit off with edits asks");
    p.update_in(cx, |p, window, cx| p.answer_leave(Some(false), window, cx));
    cx.run_until_parked();
    assert_eq!(text(&p, cx), "hello\n", "Discard restores the text");
    assert!(p.read_with(cx, |p, _| !p.edit.on && !p.unsaved()));
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).expect("read"),
        "hello\n"
    );

    // Opening another file asks; Save writes, then the other file shows.
    edit_on(&p, cx);
    cx.simulate_keystrokes("y");
    cx.run_until_parked();
    let r = root.clone();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_preview(r, "other.txt".into(), None, window, cx)
    });
    cx.run_until_parked();
    assert!(dialog_open(cx), "another file with edits asks");
    assert!(
        p.read_with(cx, |p, _| p.path == std::path::Path::new("notes.txt")),
        "not yet"
    );
    p.update_in(cx, |p, window, cx| p.answer_leave(Some(true), window, cx));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).expect("read"),
        "hyello\n",
        "the caret stayed where Discard left it"
    );
    assert_eq!(text(&p, cx), "other\n", "then the other file shows");
    assert!(
        p.read_with(cx, |p, _| !p.edit.on),
        "another file opens read-only"
    );

    // Esc with no edits just leaves editing.
    edit_on(&p, cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!dialog_open(cx) && p.read_with(cx, |p, _| !p.edit.on));
}

#[gpui::test]
fn find_works_while_editing(cx: &mut gpui::TestAppContext) {
    let (f, _ws, p, cx) = open("notes.txt", "a needle\nneedle\n", cx);
    let _root = f.project.clone();
    edit_on(&p, cx);
    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(
        p.read_with(cx, |p, _| p.find.input.is_some()),
        "mod+F opens find while editing"
    );
    cx.simulate_keystrokes("n e e d l e");
    cx.run_until_parked();
    assert_eq!(p.read_with(cx, |p, _| p.find.matches.len()), 2);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        p.read_with(cx, |p, _| p.find.input.is_none() && p.edit.on),
        "Esc closes find, keeps editing"
    );
}

#[gpui::test]
fn a_changed_file_offers_reload_or_overwrite(cx: &mut gpui::TestAppContext) {
    let (f, _ws, p, cx) = open("notes.txt", "hello\n", cx);
    let root = f.project.clone();
    edit_on(&p, cx);
    cx.simulate_keystrokes("x");
    cx.run_until_parked();
    // The agent writes the file meanwhile.
    std::fs::write(root.join("notes.txt"), "agent\n").expect("write");
    cx.simulate_keystrokes(&format!("{M}-s"));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).expect("read"),
        "agent\n",
        "a save over a changed file writes nothing"
    );
    assert!(p.read_with(cx, |p, _| p.edit.banner == Some(edit::Banner::Changed)));

    // Overwrite writes the edits.
    p.update_in(cx, |p, window, cx| p.save(true, None, window, cx));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).expect("read"),
        "xhello\n"
    );
    assert!(p.read_with(cx, |p, _| p.edit.banner.is_none() && !p.unsaved()));

    // The folder watch notices the next outside write; Reload shows it and drops the edits.
    cx.simulate_keystrokes("q");
    std::fs::write(root.join("notes.txt"), "git\n").expect("write");
    p.update(cx, |p, cx| p.disk_changed(cx));
    cx.run_until_parked();
    assert!(
        p.read_with(cx, |p, _| p.edit.banner == Some(edit::Banner::Changed)),
        "the watch flags it"
    );
    p.update_in(cx, |p, window, cx| p.reload(window, cx));
    cx.run_until_parked();
    assert_eq!(text(&p, cx), "git\n");
    assert!(p.read_with(cx, |p, _| p.edit.banner.is_none() && !p.unsaved()));
}

#[gpui::test]
fn quit_lists_unsaved_edits(cx: &mut gpui::TestAppContext) {
    let (f, _ws, p, cx) = open("notes.txt", "hello\n", cx);
    let _root = f.project.clone();
    assert!(
        !cx.update(|_, cx| blockers(cx))
            .iter()
            .any(|(_, b)| *b == Blocker::Unsaved)
    );
    edit_on(&p, cx);
    cx.simulate_keystrokes("x");
    cx.run_until_parked();
    let items = cx.update(|_, cx| blockers(cx));
    assert!(
        items
            .iter()
            .any(|(n, b)| *b == Blocker::Unsaved && n.as_ref() == "notes.txt"),
        "the quit guard lists the file: {items:?}"
    );
    let (_, body) =
        crate::workspace::quit_guard::question(&[("notes.txt".into(), Blocker::Unsaved)]);
    assert_eq!(
        body.as_ref(),
        "1 file has unsaved edits. Quitting loses it."
    );
}

#[test]
fn typing_in_a_big_file_is_cheap() {
    use gpui_component::input::Rope;
    // 2 MB, the preview's cap: the unsaved check runs on every edit.
    let line = "fn main() { println!(\"hello world\"); } // 0123456789\n";
    let big: String = line.repeat(2 * 1024 * 1024 / line.len());
    let saved = Rope::from_str(&big);
    let mut typed = saved.clone();
    typed.insert_char(1000, 'x');
    let mut same_len = saved.clone();
    same_len.remove(500..501);
    same_len.insert_char(500, 'y');
    let start = std::time::Instant::now();
    for _ in 0..100 {
        assert!(edit::differs(&typed, &saved));
    }
    let insert = start.elapsed() / 100;
    let start = std::time::Instant::now();
    for _ in 0..10 {
        assert!(edit::differs(&same_len, &saved));
    }
    let replace = start.elapsed() / 10;
    eprintln!("unsaved check on 2 MB: insert {insert:?}, same-length replace {replace:?}");
    // A frame is ~8 ms at 120 Hz; the check must take a small part of it even unoptimized.
    assert!(insert < std::time::Duration::from_micros(100), "{insert:?}");
    assert!(replace < std::time::Duration::from_millis(4), "{replace:?}");
}

#[gpui::test]
fn closing_the_tab_or_window_with_edits_asks(cx: &mut gpui::TestAppContext) {
    let (_f, ws, p, cx) = open("notes.txt", "hello\n", cx);
    edit_on(&p, cx);
    cx.simulate_keystrokes("x");
    cx.run_until_parked();
    // The window's close asks and stays open.
    assert!(
        !cx.update(crate::workspace::quit_guard::may_close),
        "the window stays"
    );
    assert!(dialog_open(cx), "closing the window asks");
    p.update_in(cx, |p, window, cx| p.answer_leave(None, window, cx));
    cx.run_until_parked();

    // The tab's Close (its menu) asks; Discard then closes it.
    cx.dispatch_action(gpui_component::dock::ClosePanel);
    cx.run_until_parked();
    assert!(dialog_open(cx), "the tab's Close asks");
    p.update_in(cx, |p, window, cx| p.answer_leave(Some(false), window, cx));
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws
            .files
            .preview
            .as_ref()
            .and_then(|p| p.upgrade()))
            .is_none_or(|p| !p.read_with(cx, |p, _| p.unsaved())),
        "the edits are gone"
    );
}
