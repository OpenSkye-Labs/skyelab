//! This machine's sidebar reads its shell: mapped back to store rows, the shell is exactly what the store lists,
//! every field and the order included. And a store write reaches a local subscriber as a change.

mod common;

use std::time::Duration;

use skyelab_env::{LocalSource, Source, store_project, store_thread};
use skyelab_protocol::ShellDelta;

#[test]
fn the_shell_maps_back_to_the_store_rows() {
    let env = common::env();
    {
        // Fill the 1.7 fields, so a dropped one would show.
        let store = env.store.lock().expect("store lock");
        store
            .set_thread_archived(env.thread_id, true)
            .expect("archive");
        store
            .set_thread_unread(env.thread_id, true)
            .expect("unread");
        store
            .set_thread_permission_mode(env.thread_id, "plan")
            .expect("mode");
        store
            .set_thread_pr_url(env.thread_id, Some("https://github.com/o/r/pull/1"))
            .expect("pr");
        store
            .set_agent_session_id(env.thread_id, Some("s-1"))
            .expect("session");
        store
            .set_limit_resume(env.thread_id, Some(42))
            .expect("resume");
    }
    let shell = LocalSource::new(env.backend())
        .subscribe_shell()
        .expect("shell")
        .snapshot;
    let store = env.store.lock().expect("store lock");
    let projects: Vec<_> = shell.projects.iter().map(store_project).collect();
    assert_eq!(projects, store.list_projects().expect("projects"));
    let threads: Vec<_> = shell.threads.iter().map(store_thread).collect();
    assert_eq!(threads, store.list_all_threads().expect("threads"));
}

#[test]
fn a_store_write_reaches_a_local_subscriber() {
    let env = common::env();
    let local = LocalSource::new(env.backend()); // the stream lives as long as its source
    let feed = local.subscribe_shell().expect("shell");
    env.store
        .lock()
        .expect("store lock")
        .set_thread_title(env.thread_id, "renamed here")
        .expect("rename");
    let change = feed
        .changes
        .recv_timeout(Duration::from_secs(30))
        .expect("a change");
    assert_eq!(change.seq, feed.snapshot.seq + 1);
    match change.delta {
        ShellDelta::ThreadUpdated(t) => assert_eq!(t.title, "renamed here"),
        other => panic!("{other:?}"),
    }
}
