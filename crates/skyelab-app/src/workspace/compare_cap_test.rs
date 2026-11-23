//! The running cap counts turns in progress, not finished agents still waiting for a prompt.

use super::compare::Compare;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_until,
};
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn finished_comparisons_dont_hold_running_slots(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let draft = project_thread(&f, &ws, cx);
    draft.update(cx, |v, _| {
        v.composer.compare = Some(Compare {
            models: vec!["opus".into(), "sonnet".into()],
            worktrees: false,
        })
    });
    type_and_send(&draft, cx);
    let grouped = |f: &super::threads_test::Fixture| {
        let s = f.store.lock().expect("lock");
        let p = s.list_projects().expect("projects")[0].id;
        s.list_threads(p)
            .expect("threads")
            .into_iter()
            .filter(|t| t.compare_group.is_some())
            .map(|t| t.id)
            .collect::<Vec<_>>()
    };
    wait_until(cx, "both rows", |_| grouped(&f).len() == 2);
    let first = grouped(&f);
    wait_until(cx, "both done", |cx| {
        ws.update(cx, |ws, cx| first.iter().all(|&id| !ws.working(id, cx)))
    });

    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let second = ws
        .read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("a new draft");
    second.update(cx, |v, _| {
        v.composer.compare = Some(Compare {
            models: vec!["opus".into(), "sonnet".into(), "haiku".into()],
            worktrees: false,
        })
    });
    type_and_send(&second, cx);
    second.read_with(cx, |v, _| {
        assert_eq!(
            v.composer.notice.as_ref().map(|n| n.to_string()),
            None,
            "a finished comparison doesn't hold running slots"
        );
    });
    wait_until(cx, "the second comparison's rows", |_| {
        grouped(&f).len() == 5
    });
}
