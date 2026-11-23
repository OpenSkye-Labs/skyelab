//! The sidebar shows each project's bound account, resolved off the UI thread.

use super::panels::RowAccount;
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::TestAppContext;

fn project_row_account(ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> Option<String> {
    ws.read_with(cx, |ws, cx| {
        match ws.sidebar.read(cx).sections[0].items[0].account.as_ref() {
            Some(RowAccount::Bound(a)) => Some(a.id.clone()),
            Some(RowAccount::Unbound) => Some("unbound".into()),
            Some(RowAccount::Hidden) | None => None,
        }
    })
}

#[gpui::test]
fn project_rows_show_their_bound_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let dir = f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    cx.run_until_parked();
    assert_eq!(project_row_account(&ws, cx).as_deref(), Some("acme"));
}

#[gpui::test]
fn an_unbound_project_says_so(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(false);
    let (ws, cx) = open_workspace(&f, cx);
    let dir = f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    cx.run_until_parked();
    assert_eq!(project_row_account(&ws, cx).as_deref(), Some("unbound"));
}
