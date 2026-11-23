//! A message the agent refused goes back into the prompt once it is empty, from the prompt's own change event, not
//! from the thread's render.

use super::threads_test::{fixture, open_workspace, project_thread, setup};
use gpui::TestAppContext;

#[gpui::test]
fn a_refused_message_waits_for_an_empty_prompt(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("n e w");
    thread.update(cx, |t, cx| {
        t.composer.restore = Some(("refused".into(), Vec::new()));
        cx.notify();
    });
    cx.run_until_parked();
    let prompt = |cx: &mut gpui::VisualTestContext| {
        thread.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string())
    };
    assert_eq!(prompt(cx), "new", "a redraw keeps what the user typed");
    cx.simulate_keystrokes("backspace backspace backspace");
    cx.run_until_parked();
    assert_eq!(
        prompt(cx),
        "refused",
        "RESTORE_MISSED once the prompt emptied"
    );
    assert!(thread.read_with(cx, |t, _| t.composer.restore.is_none()));
}
