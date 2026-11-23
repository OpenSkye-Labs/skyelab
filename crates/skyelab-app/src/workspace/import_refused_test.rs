//! Acceptance: importing a session that a project already has a thread for (imported after the picker listed it)
//! makes no second thread, and the notice names it on its own, apart from unreadable or rebound sessions.

use super::import_sessions::{import_picks, scan, summary};
use super::import_sessions_test::session;
use super::manage_harness::rig;
use gpui::TestAppContext;
use skyelab_core::AccountsConfig;
use std::collections::HashSet;

#[gpui::test]
fn a_session_imported_elsewhere_is_refused_by_name(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let f = &rig.f;
    session(
        &f.account_dir,
        &f.project_dir,
        "5f1c2a3b-0000-4000-8000-00000000d1a0",
    );
    let project = rig
        .ws
        .read_with(cx, |ws, _| {
            ws.projects.iter().find(|p| p.id == rig.project).cloned()
        })
        .expect("project");
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    // Listed before either import, as two open pickers would list it.
    let picks: Vec<_> = scan(&config, &project, &HashSet::new())
        .expect("scan")
        .rows
        .into_iter()
        .map(|r| r.t)
        .collect();
    assert_eq!(picks.len(), 1);

    let first = import_picks(&f.store, &config, &project, "acme", &picks).expect("runs");
    assert_eq!(first.threads.len(), 1);
    let again = import_picks(&f.store, &config, &project, "acme", &picks).expect("runs");
    assert_eq!((again.threads.len(), again.already, again.left), (0, 1, 0));
    assert_eq!(
        summary(&again),
        "Imported 0 sessions. 1 already had a thread in a project, so left alone: a session continues in one thread only."
    );
}
