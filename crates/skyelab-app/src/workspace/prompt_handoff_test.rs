//! The prompt moving to another window keeps the draft exactly: its text and its files. Its undo history can't move
//! (gpui-base keeps the undo stack private, see docs/gpui.md), so Undo in the new window must not lose the text.

use super::multi_window_test::{in_window, seed};
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::TestAppContext;
use skyelab_agent::Attachment;

#[gpui::test]
fn the_prompt_keeps_its_draft_when_another_window_takes_it(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let id = seed(&f.store, &f.project_dir);
    let (a, cx) = open_workspace(&f, cx);
    let a_window = a.read_with(cx, |ws, _| ws.window_handle);
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();
    let b_window = cx.update(|_, cx| window_handles(cx))[1];
    let b = cx.update(|_, cx| windows::workspaces(cx)[1].clone());
    in_window(a_window, &a, cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    let view = cx
        .update(|_, cx| a.read(cx).open_view(id, cx))
        .expect("open in A");
    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("line one\n  indented, trailing space ");
    let files = vec![
        Attachment::new("notes.txt", b"hello".to_vec()).expect("text file"),
        Attachment::new("more.md", b"# two".to_vec()).expect("text file"),
    ];
    view.update(cx, |t, cx| {
        t.composer.attachments = files.clone();
        cx.notify();
    });
    let typed = view.read_with(cx, |t, cx| t.prompt.read(cx).value());

    in_window(b_window, &b, cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| view.read(cx).window),
        b_window,
        "B took the prompt"
    );
    let (text, kept) = view.read_with(cx, |t, cx| {
        (t.prompt.read(cx).value(), t.composer.attachments.clone())
    });
    assert_eq!(text, typed, "the text, byte for byte");
    assert_eq!(kept, files, "the files, in order");

    // Undo in the new window has nothing of the old window's to replay: it never empties the draft.
    let prompt = view.read_with(cx, |t, _| t.prompt.clone());
    cx.update(|_, cx| {
        b_window
            .update(cx, |_, window, cx| {
                prompt.update(cx, |p, cx| p.focus(window, cx));
                window.dispatch_action(Box::new(gpui_component::input::Undo), cx);
            })
            .expect("B open")
    });
    cx.run_until_parked();
    let after = view.read_with(cx, |t, cx| t.prompt.read(cx).value());
    assert_eq!(after, typed, "Undo kept the draft");
}
