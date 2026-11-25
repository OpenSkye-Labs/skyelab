//! Acceptance: mod+F in a text file preview opens the thread's find bar over the file. It counts the matches, steps
//! through them with Enter / Shift-Enter (wrapping), highlights them in the editor, and Esc closes it while a thread
//! is on screen. An image has no find, and a 2 MB file is searched within a frame.

use super::*;
use crate::workspace::worktree_thread_test::{fixture, open_thread};

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn preview(
    ws: &Entity<crate::workspace::Workspace>,
    cx: &mut gpui::VisualTestContext,
) -> Entity<PreviewPanel> {
    ws.read_with(cx, |ws, _| {
        ws.files.preview.as_ref().and_then(|p| p.upgrade())
    })
    .expect("the Preview tab")
}

/// (the bar is open, its count, the editor highlights matches, the current match's index)
fn state(
    p: &Entity<PreviewPanel>,
    cx: &mut gpui::VisualTestContext,
) -> (bool, String, bool, usize) {
    p.read_with(cx, |p, cx| {
        let s = p.editor.read(cx).search_session();
        (
            p.find.input.is_some(),
            p.find.count(&p.find.query),
            s.is_active(),
            s.matcher.current_match_index(),
        )
    })
}

#[gpui::test]
fn find_in_a_preview_counts_steps_and_closes(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    std::fs::write(
        f.project.join("notes.txt"),
        "alpha\nbeta needle\nneedle gamma\nNEEDLE\n",
    )
    .expect("write");
    let (ws, _thread, cx) = open_thread(&f, cx);
    let root = f.project.clone();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_preview(root, "notes.txt".into(), None, window, cx)
    });
    cx.run_until_parked();
    let p = preview(&ws, cx);
    assert!(p.read_with(cx, |p, _| matches!(p.body, Body::Text)));
    // A click in the text focuses its editor.
    p.update_in(cx, |p, window, cx| {
        p.editor.update(cx, |e, cx| e.focus(window, cx))
    });

    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(cx.debug_bounds("find-bar").is_some(), "mod+F opens the bar");
    cx.simulate_keystrokes("n e e d l e");
    cx.run_until_parked();
    let (open, count, lit, first) = state(&p, cx);
    assert!(open && lit, "the editor highlights the matches");
    assert_eq!(count, format!("{}/3", first + 1), "case-insensitive, three");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(state(&p, cx).3, (first + 1) % 3);
    cx.simulate_keystrokes("shift-enter shift-enter");
    cx.run_until_parked();
    assert_eq!(
        state(&p, cx).3,
        (first + 2) % 3,
        "back past the first wraps"
    );
    assert_eq!(state(&p, cx).1, format!("{}/3", (first + 2) % 3 + 1));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let (open, _, lit, _) = state(&p, cx);
    assert!(!open && !lit, "Esc closes the bar and the highlights");
    assert!(cx.debug_bounds("find-bar").is_none());
    assert!(
        p.update_in(cx, |p, window, cx| p
            .editor
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)),
        "focus goes back to the file"
    );
}

#[gpui::test]
fn an_image_preview_has_no_find(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode");
    std::fs::write(f.project.join("shot.png"), png.into_inner()).expect("write");
    let (ws, _thread, cx) = open_thread(&f, cx);
    let root = f.project.clone();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_preview(root, "shot.png".into(), None, window, cx)
    });
    cx.run_until_parked();
    let p = preview(&ws, cx);
    assert!(p.read_with(cx, |p, _| matches!(p.body, Body::Image(_))));
    p.update_in(cx, |p, window, cx| p.focus.focus(window, cx));
    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(!state(&p, cx).0, "nothing to find in an image");
    assert!(cx.debug_bounds("find-bar").is_none());
}

/// A query over a 2 MB file (the preview's cap) counts within a frame: the editor's matcher runs on the UI thread.
#[gpui::test]
fn find_in_a_large_preview_is_quick(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let line = "let value = compute(input, other_argument); // a line of ordinary code\n";
    let body = line.repeat(skyelab_core::files::MAX_TEXT_BYTES as usize / line.len());
    std::fs::write(f.project.join("big.rs"), &body).expect("write");
    let (ws, _thread, cx) = open_thread(&f, cx);
    let root = f.project.clone();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_preview(root, "big.rs".into(), None, window, cx)
    });
    cx.run_until_parked();
    let p = preview(&ws, cx);
    assert!(
        p.read_with(cx, |p, _| matches!(p.body, Body::Text)),
        "under the cap"
    );
    p.update_in(cx, |p, window, cx| {
        p.editor.update(cx, |e, cx| e.focus(window, cx))
    });
    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    let started = std::time::Instant::now();
    // One character, the worst case: a match on every line and then some. Counted and drawn.
    cx.simulate_keystrokes("e");
    cx.run_until_parked();
    let took = started.elapsed();
    let n = p.read_with(cx, |p, _| p.find.matches.len());
    assert!(n > body.lines().count(), "{n} matches");
    // A debug build on a loaded machine: a frame is 16 ms in release; leave room, but catch a per-line relayout.
    assert!(took.as_millis() < 250, "a query took {took:?}");
}
