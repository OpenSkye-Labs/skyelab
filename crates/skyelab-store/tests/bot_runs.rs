//! Schema 28: bots. A task's GitHub triggers and limits, its run history, and the items its poller has seen.

use rusqlite::Connection;
use skyelab_store::scheduled::{BotRun, NewBotRun, ScheduledTask};
use skyelab_store::{NewThread, Store};
use std::path::Path;

fn bot(project_id: i64) -> ScheduledTask {
    ScheduledTask {
        project_id,
        title: "PR review".into(),
        prompt: "Review it".into(),
        permission_mode: "plan".into(),
        account_id: "acme".into(),
        enabled: true,
        triggers: "pr_opened".into(),
        author_filter: "me".into(),
        max_runs_per_hour: 4,
        ..Default::default()
    }
}

#[test]
fn triggers_save_and_a_save_or_re_enable_unprimes_the_poller() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/w/demo"))
        .expect("project");
    let mut t = bot(p.id);
    t.id = store.save_scheduled_task(&t).expect("insert");
    let read = store.scheduled_task(t.id).expect("read").expect("there");
    assert_eq!(
        (
            read.triggers.as_str(),
            read.author_filter.as_str(),
            read.max_runs_per_hour
        ),
        ("pr_opened", "me", 4)
    );
    assert!(read.schedule.is_empty() && !read.gh_primed);
    store.set_gh_primed(t.id).expect("prime");
    assert!(
        store
            .scheduled_task(t.id)
            .expect("read")
            .expect("there")
            .gh_primed
    );
    store.set_scheduled_task_enabled(t.id, false).expect("off");
    assert!(
        store
            .scheduled_task(t.id)
            .expect("read")
            .expect("there")
            .gh_primed,
        "pausing keeps it"
    );
    store.set_scheduled_task_enabled(t.id, true).expect("on");
    assert!(
        !store
            .scheduled_task(t.id)
            .expect("read")
            .expect("there")
            .gh_primed,
        "re-enabling unprimes"
    );
    store.set_gh_primed(t.id).expect("prime");
    store.save_scheduled_task(&t).expect("edit");
    assert!(
        !store
            .scheduled_task(t.id)
            .expect("read")
            .expect("there")
            .gh_primed,
        "a save unprimes"
    );
}

#[test]
fn runs_queue_link_a_thread_prune_and_go_with_their_task() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/w/demo"))
        .expect("project");
    let id = store.save_scheduled_task(&bot(p.id)).expect("insert");
    let url = "https://github.com/acme/demo/pull/7";
    let run = store
        .add_bot_run(&NewBotRun {
            task: id,
            at: 1_000,
            trigger: "pr_opened",
            item_url: Some(url),
            item_prompt: Some("framed"),
            status: "queued",
            error: None,
        })
        .expect("queue");
    let queued = store.queued_bot_run(id).expect("read").expect("one queued");
    assert_eq!(
        (queued.id, queued.item_prompt.as_deref()),
        (run, Some("framed"))
    );
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
    assert!(store.queued_bot_run(id).expect("read").is_none());
    assert!(
        store.thread_untrusted(t.id).expect("read"),
        "a GitHub-started thread stays untrusted on resume"
    );
    let listed: Vec<BotRun> = store.bot_runs(id, 50).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            listed[0].status.as_str(),
            listed[0].thread_id,
            listed[0].item_prompt.as_deref()
        ),
        ("started", Some(t.id), None)
    );
    for i in 0..205 {
        let run = NewBotRun {
            task: id,
            at: 2_000 + i,
            trigger: "schedule",
            status: "started",
            ..Default::default()
        };
        store.add_bot_run(&run).expect("add");
    }
    let all = store.bot_runs(id, 1_000).expect("list");
    assert_eq!(all.len(), 200, "the newest 200 are kept");
    assert_eq!(all[0].at, 2_204, "newest first");

    store
        .mark_bot_seen(id, &["pr:7".into(), "pr:8".into()], 1_000)
        .expect("seen");
    assert_eq!(store.bot_seen(id).expect("read").len(), 2);
    store.delete_scheduled_task(id).expect("delete");
    assert!(store.bot_runs(id, 10).expect("list").is_empty());
    assert!(store.bot_seen(id).expect("read").is_empty());
}

#[test]
fn bot_branches_are_the_worktree_branches_of_task_threads() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/w/demo"))
        .expect("project");
    let id = store.save_scheduled_task(&bot(p.id)).expect("insert");
    let new = |title| NewThread {
        project_id: p.id,
        title,
        cwd: Path::new("/w/demo"),
        account_id: "acme",
        status: "idle",
    };
    let by_bot = store.create_thread(&new("bot")).expect("thread");
    store
        .set_thread_scheduled_task(by_bot.id, id)
        .expect("mark");
    store
        .set_thread_worktree(by_bot.id, Some(Path::new("/w/wt1")), Some("skyelab/bot-1"))
        .expect("wt");
    let by_hand = store.create_thread(&new("hand")).expect("thread");
    store
        .set_thread_worktree(by_hand.id, Some(Path::new("/w/wt2")), Some("mine"))
        .expect("wt");
    let branches = store.bot_branches(p.id).expect("read");
    assert!(
        branches.contains("skyelab/bot-1") && !branches.contains("mine"),
        "{branches:?}"
    );
}

#[test]
fn a_schema_27_store_keeps_its_tasks_as_plain_scheduled_ones() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("s.db");
    let store = Store::open(&path).expect("open");
    let p = store
        .create_project("demo", Path::new("/w/demo"))
        .expect("project");
    let mut plain = bot(p.id);
    plain.triggers.clear();
    plain.schedule = "daily 09:00".into();
    for _ in 0..2 {
        store.save_scheduled_task(&plain).expect("save");
    }
    drop(store);
    Connection::open(&path)
        .expect("raw")
        .execute_batch(
            "DROP TABLE bot_runs; DROP TABLE bot_seen;
             ALTER TABLE scheduled_tasks DROP COLUMN triggers; ALTER TABLE scheduled_tasks DROP COLUMN author_filter;
             ALTER TABLE scheduled_tasks DROP COLUMN max_runs_per_hour; ALTER TABLE scheduled_tasks DROP COLUMN gh_primed;
             PRAGMA user_version = 27;",
        )
        .expect("back to 27");
    let store = Store::open(&path).expect("reopen");
    let tasks = store.scheduled_tasks().expect("list");
    assert_eq!(tasks.len(), 2);
    for t in tasks {
        assert_eq!(
            (
                t.schedule.as_str(),
                t.triggers.as_str(),
                t.author_filter.as_str(),
                t.max_runs_per_hour
            ),
            ("daily 09:00", "", "anyone", 6)
        );
        assert!(store.bot_runs(t.id, 10).expect("list").is_empty());
    }
}
