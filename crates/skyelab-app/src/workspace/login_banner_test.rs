//! Acceptance: a turn that fails because the account is signed out offers Log in on its banner, and Log in opens one
//! terminal tab logging that same account in (never another).

use super::threads_test::{fixture, open_workspace, project_thread, setup, wait_until};
use gpui::TestAppContext;

#[gpui::test]
fn the_signed_out_banner_logs_the_threads_own_account_in(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    thread
        .update_in(cx, |t, window, cx| t.submit_remote("login", window, cx))
        .expect("sent");
    wait_until(cx, "the signed-out failure", |cx| {
        thread.read_with(cx, |t, _| t.shown_failure().is_some_and(|f| f.signed_out))
    });
    cx.run_until_parked();
    let login = cx
        .debug_bounds("failure-login")
        .expect("the banner offers Log in");
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 0);
    cx.simulate_click(login.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let accounts: Vec<_> = ws.read_with(cx, |ws, cx| {
        ws.terminals
            .iter()
            .flat_map(|t| {
                t.read(cx)
                    .panes
                    .iter()
                    .map(|p| p.account.as_ref().map(|a| a.id.clone()))
            })
            .collect()
    });
    assert_eq!(
        accounts,
        vec![Some("acme".to_string())],
        "one login terminal, for the thread's account"
    );
}
