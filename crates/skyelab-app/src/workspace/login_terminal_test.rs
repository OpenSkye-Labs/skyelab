//! A Log in terminal is named after the account it logs in, and isn't reopened after a restart: it ran one
//! command, and a restored shell in the home folder would only show as "No account".

use super::LogInAccount;
use super::worktree_thread_test::{fixture, open_thread};

#[gpui::test]
fn login_terminal_is_named_for_its_account_and_not_saved(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    let before = ws.read_with(cx, |ws, cx| ws.terminal_layouts(cx).len());
    ws.update_in(cx, |ws, window, cx| {
        ws.log_in_account(&LogInAccount { id: "acme".into() }, window, cx)
    });
    cx.run_until_parked();
    let name = ws.read_with(cx, |ws, cx| {
        ws.terminals
            .last()
            .and_then(|p| p.read(cx).name.clone())
            .map(|n| n.to_string())
    });
    assert_eq!(name.as_deref(), Some("Log in: acme"));
    let after = ws.read_with(cx, |ws, cx| ws.terminal_layouts(cx).len());
    assert_eq!(after, before, "the login tab is left out of the saved tabs");
}
