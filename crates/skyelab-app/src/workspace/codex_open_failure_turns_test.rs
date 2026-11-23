//! Codex refusing to open the thread while a follow-up waits answers every queued prompt, so the row leaves
//! `running` though `codex app-server` is still alive.

use super::claude_folded_turns_test::{one_view, row, wait_until};
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn a_refused_open_with_a_queued_follow_up_leaves_the_row_idle(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, Some(".fake-codex-refuse-open"));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("first", window, cx))
        .expect("started");
    wait_until(cx, "the session", |cx| {
        one.view.read_with(cx, |v, _| v.session.is_some())
    });
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("second", window, cx))
        .expect("queued while codex opens the thread");
    wait_until(cx, "the refusal's result", |cx| {
        row(&one, cx).is_some_and(|(_, n)| n >= 1)
    });
    wait_until(cx, "the row to leave running", |cx| {
        row(&one, cx).is_some_and(|(status, _)| status == "idle")
    });
    assert!(
        one.view.read_with(cx, |v, _| v.session.is_some()),
        "codex is still alive"
    );
}
