//! The sidebar's thread rows show the account their project's folder is bound to, resolved off the UI thread.

use super::panels::RowAccount;
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::TestAppContext;

fn row_account(ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> Option<String> {
    ws.read_with(cx, |ws, cx| {
        let sidebar = ws.sidebar.read(cx);
        let row = sidebar.sections.iter().flat_map(|s| &s.items).next()?;
        match row.account.as_ref() {
            Some(RowAccount::Bound(a)) => Some(a.id.clone()),
            Some(RowAccount::Unbound) => Some("unbound".into()),
            Some(RowAccount::Hidden) | None => None,
        }
    })
}

fn add_project_with_thread(
    f: &super::threads_test::Fixture,
    ws: &Entity<Workspace>,
    cx: &mut gpui::VisualTestContext,
) {
    let dir = f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    cx.run_until_parked();
    let project = ws.read_with(cx, |ws, _| ws.projects[0].id);
    f.store
        .lock()
        .expect("lock")
        .create_thread(&skyelab_store::NewThread {
            project_id: project,
            title: "t",
            cwd: &f.project_dir,
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    ws.update(cx, |ws, cx| ws.load_threads(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn thread_rows_show_their_projects_bound_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    add_project_with_thread(&f, &ws, cx);
    assert_eq!(row_account(&ws, cx).as_deref(), Some("acme"));
}

#[gpui::test]
fn an_unbound_projects_threads_say_so(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(false);
    let (ws, cx) = open_workspace(&f, cx);
    add_project_with_thread(&f, &ws, cx);
    assert_eq!(row_account(&ws, cx).as_deref(), Some("unbound"));
}
