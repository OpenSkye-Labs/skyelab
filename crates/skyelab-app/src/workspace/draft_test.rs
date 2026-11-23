//! Acceptance: an unsent prompt and its files are kept per thread in the store: they come back after the tab is
//! closed and reopened (a restart reads the same store) and sending clears them.

use crate::workspace::manage_harness::rig;
use crate::workspace::threads_test::wait_until;
use gpui::TestAppContext;
use skyelab_agent::Attachment;

#[gpui::test]
fn a_draft_survives_reopening_and_send_clears_it(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("drafty", cx);
    let other = r.thread("other", cx);
    // A session to resume, so the send at the end starts the fake agent.
    r.f.store
        .lock()
        .expect("lock")
        .set_agent_session_id(id, Some("fake-sess-1"))
        .expect("session");
    let view = r.open(id, cx);
    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("half a thought");
    view.update(cx, |t, cx| {
        t.composer
            .attachments
            .push(Attachment::new("notes.txt", b"hello".to_vec()).expect("text file"));
        cx.notify();
    });
    let owner = format!("thread:{id}");
    let stored = |cx: &mut gpui::VisualTestContext| {
        let _ = cx;
        r.f.store
            .lock()
            .expect("lock")
            .draft(&owner)
            .expect("draft")
    };
    wait_until(cx, "the draft stored", |cx| {
        let d = stored(cx);
        d.text == "half a thought" && d.files.len() == 1
    });

    // Switch away and close the tab, then open it again: a new view reads it back.
    let _ = r.open(other, cx);
    r.ws.update_in(cx, |ws, window, cx| ws.close_thread(&view, window, cx));
    cx.run_until_parked();
    let view = r.open(id, cx);
    wait_until(cx, "the draft back in the composer", |cx| {
        view.read_with(cx, |t, cx| {
            t.prompt.read(cx).value() == "half a thought" && t.composer.attachments.len() == 1
        })
    });
    assert!(
        r.f.store
            .lock()
            .expect("lock")
            .draft(&format!("thread:{other}"))
            .expect("draft")
            .is_empty(),
        "drafts are per thread"
    );

    // Sending empties the composer, and with it the draft.
    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "the draft cleared", |cx| stored(cx).is_empty());
}
