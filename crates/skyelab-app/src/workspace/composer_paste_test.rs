//! Long pastes become a text-file chip (with "Paste as text" to undo), and a prompt over the limit (chips included)
//! is refused with its text kept.

use super::composer_review_test::view_with;
use super::thread::SendPrompt;
use gpui::{ClipboardItem, VisualTestContext};

fn prompt(view: &gpui::Entity<super::thread::ThreadView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string())
}

#[gpui::test]
fn long_paste_becomes_a_chip_and_can_go_back_to_text(cx: &mut gpui::TestAppContext) {
    let (_home, _dir, _config, view, cx) = view_with(cx, None);
    let long = "line\n".repeat(skyelab_core::prompt::LONG_PASTE_LINES);
    let taken = view.update(cx, |t, cx| {
        t.attach_clipboard(&ClipboardItem::new_string("short".into()), cx)
    });
    assert!(!taken, "a short paste goes into the prompt as usual");
    let taken = view.update(cx, |t, cx| {
        t.attach_clipboard(&ClipboardItem::new_string(long.clone()), cx)
    });
    assert!(taken, "a long paste is taken as a file");
    let names = view.read_with(cx, |t, _| {
        t.composer
            .attachments
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, ["pasted-text.txt"]);
    assert_eq!(prompt(&view, cx), "", "none of it lands in the prompt");
    // A second long paste gets its own name.
    view.update(cx, |t, cx| {
        t.attach_clipboard(&ClipboardItem::new_string(long.clone()), cx)
    });
    let n = view.read_with(cx, |t, _| t.composer.attachments.len());
    assert_eq!(n, 2);
    view.update_in(cx, |t, window, cx| t.paste_as_text(window, cx));
    let names = view.read_with(cx, |t, _| {
        t.composer
            .attachments
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, ["pasted-text.txt"], "only the last paste goes back");
    assert_eq!(prompt(&view, cx), long);
    assert!(view.read_with(cx, |t, _| t.composer.pasted.is_none()));
}

#[gpui::test]
fn a_long_paste_with_no_room_for_a_chip_goes_into_the_prompt(cx: &mut gpui::TestAppContext) {
    let (_home, _dir, _config, view, cx) = view_with(cx, None);
    let long = "line\n".repeat(skyelab_core::prompt::LONG_PASTE_LINES);
    for _ in 0..skyelab_agent::MAX_ATTACHMENTS {
        view.update(cx, |t, cx| {
            t.attach_clipboard(&ClipboardItem::new_string(long.clone()), cx)
        });
    }
    let taken = view.update(cx, |t, cx| {
        t.attach_clipboard(&ClipboardItem::new_string(long.clone()), cx)
    });
    assert!(
        !taken,
        "a paste no chip can hold goes in as text, not nowhere"
    );
    let n = view.read_with(cx, |t, _| t.composer.attachments.len());
    assert_eq!(n, skyelab_agent::MAX_ATTACHMENTS);
}

#[gpui::test]
fn send_over_the_limit_is_refused_and_keeps_the_text(cx: &mut gpui::TestAppContext) {
    let (_home, _dir, _config, view, cx) = view_with(cx, None);
    let text = "x".repeat(skyelab_core::prompt::MAX_PROMPT_CHARS + 5);
    view.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value(text.clone(), window, cx));
        t.send_prompt(&SendPrompt, window, cx);
    });
    cx.run_until_parked();
    let (notice, started) = view.read_with(cx, |t, _| {
        (
            t.composer.notice.clone(),
            t.session.is_some() || t.thread.is_some(),
        )
    });
    assert!(!started, "nothing was sent");
    assert!(
        notice
            .as_deref()
            .is_some_and(|n| n.starts_with("Prompt is 5 characters over")),
        "{notice:?}"
    );
    assert_eq!(prompt(&view, cx), text);
}

#[gpui::test]
fn chips_count_toward_the_limit(cx: &mut gpui::TestAppContext) {
    use super::composer_context::{ChipKind, ContextChip};
    let (_home, _dir, _config, view, cx) = view_with(cx, None);
    let chip = |id| ContextChip {
        id,
        kind: ChipKind::Terminal,
        label: "Terminal".into(),
        text: "y".repeat(skyelab_core::prompt::MAX_PROMPT_CHARS / 2),
        open: false,
        bound: None,
        thread: None,
    };
    view.update_in(cx, |t, window, cx| {
        t.composer.contexts = vec![chip(1), chip(2)];
        t.prompt.update(cx, |p, cx| p.set_value("hi", window, cx));
        t.send_prompt(&SendPrompt, window, cx);
    });
    cx.run_until_parked();
    let (notice, chips, started) = view.read_with(cx, |t, _| {
        (
            t.composer.notice.clone(),
            t.composer.contexts.len(),
            t.session.is_some() || t.thread.is_some(),
        )
    });
    assert!(!started, "nothing was sent");
    assert!(
        notice
            .as_deref()
            .is_some_and(|n| n.starts_with("Prompt is")),
        "{notice:?}"
    );
    assert_eq!(chips, 2, "the chips stay with the text");
    assert_eq!(prompt(&view, cx), "hi");
}
