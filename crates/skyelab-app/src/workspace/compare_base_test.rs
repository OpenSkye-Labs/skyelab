//! Every model of a comparison in worktrees starts from the base branch picked in the draft.

use super::changes_model_test::git;
use super::compare::Compare;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_until,
};
use gpui::TestAppContext;

#[gpui::test]
fn every_model_starts_from_the_picked_base(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    std::fs::write(f.project_dir.join("README"), "hi\n").expect("file");
    git(&f.project_dir, &["init", "-q", "-b", "main"]);
    git(&f.project_dir, &["add", "."]);
    git(&f.project_dir, &["commit", "-q", "-m", "init"]);
    git(&f.project_dir, &["checkout", "-q", "-b", "dev"]);
    std::fs::write(f.project_dir.join("README"), "dev\n").expect("file");
    git(&f.project_dir, &["commit", "-q", "-am", "dev"]);
    git(&f.project_dir, &["checkout", "-q", "main"]);
    let (ws, cx) = open_workspace(&f, cx);
    let draft = project_thread(&f, &ws, cx);
    draft.update(cx, |v, cx| {
        v.branch_picker
            .update(cx, |b, _| b.base = Some("dev".into()));
        v.composer.compare = Some(Compare {
            models: vec!["opus".into(), "sonnet".into()],
            worktrees: true,
        });
    });
    type_and_send(&draft, cx);
    let rows = || {
        let s = f.store.lock().expect("lock");
        let p = s.list_projects().expect("p")[0].id;
        s.list_threads(p)
            .expect("t")
            .into_iter()
            .filter(|t| t.compare_group.is_some())
            .collect::<Vec<_>>()
    };
    wait_until(cx, "both rows", |_| rows().len() == 2);
    let bases: std::collections::HashSet<_> = rows().into_iter().map(|t| t.worktree_base).collect();
    assert_eq!(
        bases.len(),
        1,
        "models started from different bases: {bases:?}"
    );
}
