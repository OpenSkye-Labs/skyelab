use std::path::Path;

use skyelab_store::{Draft, NewThread, Store};

fn thread(store: &Store) -> (i64, i64) {
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("test setup");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "fix the login bug",
            cwd: Path::new("/work/demo"),
            account_id: "work",
            status: "starting",
        })
        .expect("test setup");
    (p.id, t.id)
}

#[test]
fn generated_title_replaces_only_a_first_line_title() {
    let store = Store::open_in_memory().expect("store");
    let (_, id) = thread(&store);
    assert_eq!(store.title_source(id).unwrap().as_deref(), Some("prompt"));
    assert!(store.set_generated_title(id, "Fix login bug", false).unwrap());
    assert_eq!(store.thread(id).unwrap().unwrap().title, "Fix login bug");

    // The user renames it: a late generated title doesn't replace theirs.
    store.set_thread_title(id, "mine").unwrap();
    assert!(!store.set_generated_title(id, "Other", false).unwrap());
    assert_eq!(store.thread(id).unwrap().unwrap().title, "mine");
    // Regenerate (forced) does.
    assert!(store.set_generated_title(id, "Again", true).unwrap());
    assert_eq!(store.thread(id).unwrap().unwrap().title, "Again");
}

#[test]
fn drafts_keep_text_and_files_per_owner_and_go_with_their_thread() {
    let mut store = Store::open_in_memory().expect("store");
    let (_, id) = thread(&store);
    let owner = format!("thread:{id}");
    store.set_draft_text(&owner, "half a thought").unwrap();
    let files = vec![("a.txt".to_owned(), b"hello".to_vec())];
    store.set_draft_files(&owner, &files).unwrap();
    assert_eq!(
        store.draft(&owner).unwrap(),
        Draft {
            text: "half a thought".into(),
            files: files.clone()
        }
    );
    assert!(store.draft("thread:999").unwrap().is_empty());

    store.set_draft_text(&owner, "").unwrap();
    assert_eq!(store.draft(&owner).unwrap().files, files, "files stay");
    store.set_draft_files(&owner, &[]).unwrap();
    assert!(store.draft(&owner).unwrap().is_empty());

    store.set_draft_text(&owner, "again").unwrap();
    store.delete_thread(id).unwrap();
    assert!(store.draft(&owner).unwrap().is_empty(), "deleted with its thread");
}

#[test]
fn fork_copies_the_given_events_and_records_its_source() {
    let mut store = Store::open_in_memory().expect("store");
    let (project, id) = thread(&store);
    store.append_event(id, "prompt", "\"hi\"").unwrap();
    store
        .append_event(id, "agent", r#"{"RateLimit":{"window":"5h","used":50.0}}"#)
        .unwrap();
    store.append_event(id, "agent", r#"{"Result":{}}"#).unwrap();
    let events = store.events_since(id, 0).unwrap();
    let fork = store
        .create_fork(
            &NewThread {
                project_id: project,
                title: "fix the login bug",
                cwd: Path::new("/work/demo"),
                account_id: "work",
                status: "stopped",
            },
            id,
            "context",
            &events,
        )
        .unwrap();
    assert_eq!(fork.forked_from, Some(id));
    assert_eq!(fork.account_id, "work");
    assert_eq!(store.fork_context(fork.id).unwrap().as_deref(), Some("context"));
    assert_eq!(store.title_source(fork.id).unwrap().as_deref(), Some("fork"));
    let copied = store.events_since(fork.id, 0).unwrap();
    let kinds: Vec<(i64, &str)> = copied.iter().map(|e| (e.seq, e.kind.as_str())).collect();
    assert_eq!(kinds, [(1, "prompt"), (2, "agent")], "usage reading left out");
    assert_eq!(copied[1].payload_json, r#"{"Result":{}}"#);
    assert_eq!(store.thread(id).unwrap().unwrap().forked_from, None);
}
