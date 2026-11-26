//! A thread a GitHub trigger started stays untrusted (no SkyeLab tools on resume), whatever happens to its bot's
//! history: pruned, the bot deleted, or the thread forked.

use skyelab_store::scheduled::{NewBotRun, ScheduledTask};
use skyelab_store::{NewThread, Store};
use std::path::Path;

fn started(store: &Store) -> (i64, i64, i64) {
    let p = store
        .create_project("demo", Path::new("/w/demo"))
        .expect("project");
    let id = store
        .save_scheduled_task(&ScheduledTask {
            project_id: p.id,
            title: "Triage".into(),
            prompt: "Triage it".into(),
            permission_mode: "plan".into(),
            account_id: "acme".into(),
            enabled: true,
            triggers: "issue_opened".into(),
            ..Default::default()
        })
        .expect("insert");
    let run = store
        .add_bot_run(&NewBotRun {
            task: id,
            at: 1_000,
            trigger: "issue_opened",
            item_url: Some("https://github.com/acme/demo/issues/1"),
            item_prompt: Some("framed attacker text"),
            status: "queued",
            error: None,
        })
        .expect("queue");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "run",
            cwd: Path::new("/w/demo"),
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    store.update_bot_run(run, "started", None).expect("start");
    store.set_bot_run_thread(run, t.id).expect("link");
    assert!(store.thread_untrusted(t.id).expect("read"));
    (p.id, id, t.id)
}

#[test]
fn a_github_thread_stays_untrusted_after_its_run_row_is_pruned_or_its_bot_deleted() {
    let store = Store::open_in_memory().expect("open");
    let (_, id, thread) = started(&store);
    // An outsider opens 200 issues: each is recorded as rate limited, pushing the started row out of history.
    for i in 0..200 {
        let run = NewBotRun {
            task: id,
            at: 2_000 + i,
            trigger: "issue_opened",
            status: "rate_limited",
            ..Default::default()
        };
        store.add_bot_run(&run).expect("limited");
    }
    assert!(
        store.thread_untrusted(thread).expect("read"),
        "UNTRUSTED LOST AFTER PRUNE"
    );
    store.delete_scheduled_task(id).expect("delete");
    assert!(
        store.thread_untrusted(thread).expect("read"),
        "UNTRUSTED LOST AFTER DELETE"
    );
}

#[test]
fn a_fork_of_a_github_thread_is_untrusted() {
    let mut store = Store::open_in_memory().expect("open");
    let (project, _, thread) = started(&store);
    let fork = store
        .create_fork(
            &NewThread {
                project_id: project,
                title: "fork",
                cwd: Path::new("/w/demo"),
                account_id: "acme",
                status: "stopped",
            },
            thread,
            "GitHub text",
            &[],
        )
        .expect("fork");
    assert!(
        store.thread_untrusted(fork.id).expect("read"),
        "FORK NOT UNTRUSTED"
    );
}
