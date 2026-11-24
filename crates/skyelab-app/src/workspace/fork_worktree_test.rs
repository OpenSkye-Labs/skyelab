//! Fork and Continue-in start where their source runs, as T3's do (a fork spreads its source thread, worktree
//! included; a provider switch keeps the thread's workspace): in the source's worktree, or in the project folder once
//! that worktree is gone.

use super::copy_single_flight_test::{bind, use_fake_codex};
use crate::workspace::manage_harness::rig;
use crate::workspace::thread_extras::fork::ForkThread;
use crate::workspace::thread_extras::handoff::HandOffThread;
use crate::workspace::threads_test::{Fixture, wait_until};
use gpui::TestAppContext;
use skyelab_core::Provider;
use skyelab_store::{NewThread, Thread};

fn newest_child(f: &Fixture, of: i64, n: usize) -> Option<Thread> {
    let kids: Vec<Thread> = f
        .store
        .lock()
        .expect("lock")
        .list_all_threads()
        .expect("threads")
        .into_iter()
        .filter(|t| t.forked_from == Some(of))
        .collect();
    (kids.len() == n).then(|| kids.into_iter().max_by_key(|t| t.id))?
}

#[gpui::test]
fn forks_and_handoffs_start_in_the_sources_worktree(cx: &mut TestAppContext) {
    use_fake_codex(cx);
    let (r, cx) = rig(cx);
    let (f, ws) = (&r.f, r.ws.clone());
    // The source ran in its own worktree (worktrees sit under the project's folder).
    let wt = f.project_dir.join(".skyelab-wt").join("fix");
    std::fs::create_dir_all(&wt).expect("worktree dir");
    let id = {
        let s = f.store.lock().expect("lock");
        let id = s
            .create_thread(&NewThread {
                project_id: r.project,
                title: "Fix",
                cwd: &wt,
                account_id: "acme",
                status: "idle",
            })
            .expect("thread")
            .id;
        s.set_thread_worktree(id, Some(&wt), Some("skyelab/fix"))
            .expect("worktree");
        s.set_thread_worktree_base(id, Some("abc123"))
            .expect("base");
        id
    };
    ws.update(cx, |ws, cx| ws.load_threads(cx));
    cx.run_until_parked();

    let check = |t: &Thread| {
        assert_eq!(t.cwd, wt, "runs in the source's worktree");
        assert_eq!(t.worktree_path.as_deref(), Some(wt.as_path()));
        assert_eq!(t.worktree_branch.as_deref(), Some("skyelab/fix"));
        assert_eq!(t.worktree_base.as_deref(), Some("abc123"));
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.fork_thread(&ForkThread { id, turns: 1 }, window, cx)
    });
    wait_until(cx, "the fork", |_| newest_child(f, id, 1).is_some());
    check(&newest_child(f, id, 1).expect("fork"));

    bind(f, &ws, "oai", cx);
    ws.update_in(cx, |ws, window, cx| {
        let a = HandOffThread {
            id,
            to: Provider::Codex,
        };
        ws.hand_off_thread(&a, window, cx)
    });
    wait_until(cx, "the handoff", |_| newest_child(f, id, 2).is_some());
    check(&newest_child(f, id, 2).expect("handoff"));

    // Discarded: the folder is gone, so the next fork starts in the project folder, with no worktree.
    bind(f, &ws, "acme", cx);
    std::fs::remove_dir_all(&wt).expect("discard");
    f.store
        .lock()
        .expect("lock")
        .set_thread_worktree(id, None, Some("skyelab/fix"))
        .expect("discarded");
    ws.update(cx, |ws, cx| ws.load_threads(cx));
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.fork_thread(&ForkThread { id, turns: 1 }, window, cx)
    });
    wait_until(cx, "the third copy", |_| newest_child(f, id, 3).is_some());
    let t = newest_child(f, id, 3).expect("fork");
    assert_eq!(t.cwd, f.project_dir);
    assert_eq!(t.worktree_path, None);
}
