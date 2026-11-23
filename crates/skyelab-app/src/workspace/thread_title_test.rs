//! Acceptance: after the first turn, the thread's own account's agent names it (no tools, no saved session), and
//! the sidebar shows the name.

use crate::workspace::thread_extras::prefs::ThreadPrefs;
use crate::workspace::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::ThreadSettings;

pub(super) fn titles(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        cx.set_global(ThreadPrefs {
            path: None,
            settings: ThreadSettings {
                generate_titles: on,
                ..Default::default()
            },
        })
    });
}

#[gpui::test]
fn the_first_turn_names_the_thread_under_its_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    titles(true, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    let want = "Fake title from .claude-acme";
    wait_until(cx, "the generated title", |cx| {
        thread.read_with(cx, |t, _| t.title() == want)
    });
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    assert_eq!(
        f.store
            .lock()
            .expect("lock")
            .thread(id)
            .expect("read")
            .expect("row")
            .title,
        want
    );
    ws.read_with(cx, |ws, _| {
        let row = ws.threads.iter().find(|t| t.id == id).expect("listed");
        assert_eq!(row.title, want, "the sidebar row shows it");
    });

    let log =
        std::fs::read_to_string(f.project_dir.join(".fake-title-call")).expect("one title call");
    assert_eq!(log.matches("\n---\n").count(), 1, "one call: {log}");
    assert!(
        log.contains(&format!("config_dir={}", f.account_dir.display())),
        "{log}"
    );
    for flag in [
        "arg=[--tools]\narg=[]\n",
        "arg=[--no-session-persistence]",
        "arg=[--model]\narg=[haiku]",
    ] {
        assert!(log.contains(flag), "missing {flag:?}: {log}");
    }
    assert!(log.contains("hi\nthere"), "this thread's prompt: {log}");
    assert!(
        log.contains("I'll look at the project layout first."),
        "this thread's reply: {log}"
    );

    // A later turn never renames it again.
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("more");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    cx.run_until_parked();
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    let log = std::fs::read_to_string(f.project_dir.join(".fake-title-call")).expect("log");
    assert_eq!(log.matches("\n---\n").count(), 1, "still one call");
}
