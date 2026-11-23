//! A refused resume keeps the composer's terminal chips, like it keeps attachments and the typed text.

use super::resume::{rebind, reopen, send_text, stopped_thread};
use super::*;

#[gpui::test]
fn refused_resume_keeps_terminal_contexts(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    rebind(&f, None);
    let view = reopen(&f, &row, cx);
    view.update(cx, |t, cx| {
        t.add_terminal_context("Terminal 1".into(), "error: boom", cx)
    });
    send_text(&view, "again", cx);
    view.read_with(cx, |t, _| {
        assert!(t.resume_refused.is_some(), "resume refused");
        assert!(t.session.is_none(), "nothing started");
        assert_eq!(t.composer.terminal_contexts.len(), 1, "terminal chip kept");
    });
}
