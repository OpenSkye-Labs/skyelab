//! Acceptance: "Add to prompt" puts a terminal selection in the shown thread's composer as a chip, and the send
//! carries it after the typed text as a fenced block.

use super::NewTerminal;
use super::thread_model::Item;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use skyelab_terminal::TerminalEvent;

#[gpui::test]
fn add_to_prompt_sends_fenced_block(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let term = ws.read_with(cx, |ws, cx| ws.terminals[0].read(cx).panes[0].view.clone());

    term.update(cx, |_, cx| {
        cx.emit(TerminalEvent::AddToPrompt(
            "error: boom\nat line 3\n".into(),
        ))
    });
    cx.run_until_parked();
    let chips = thread.read_with(cx, |t, _| t.composer.terminal_contexts.clone());
    assert_eq!(chips.len(), 1);
    assert_eq!(chips[0].label.as_ref(), "Terminal 1");
    assert_eq!(chips[0].text, "error: boom\nat line 3");

    // A blank selection adds nothing.
    term.update(cx, |_, cx| {
        cx.emit(TerminalEvent::AddToPrompt("\n\n".into()))
    });
    cx.run_until_parked();
    assert_eq!(
        thread.read_with(cx, |t, _| t.composer.terminal_contexts.len()),
        1
    );

    send(cx);
    wait_until(cx, "the prompt", |cx| {
        thread.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Prompt(_)))
        })
    });
    let sent = thread.read_with(cx, |t, _| {
        t.items.iter().find_map(|i| match i {
            Item::Prompt(p) => Some(p.clone()),
            _ => None,
        })
    });
    assert_eq!(
        sent.as_deref(),
        Some("fix it\n\nTerminal 1 output:\n```text\nerror: boom\nat line 3\n```")
    );
    assert!(
        thread.read_with(cx, |t, _| t.composer.terminal_contexts.is_empty()),
        "the chip is sent"
    );
}
