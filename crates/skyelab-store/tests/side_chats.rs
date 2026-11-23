use std::path::Path;

use skyelab_store::{NewThread, Store};

fn new<'a>(project_id: i64, title: &'a str) -> NewThread<'a> {
    NewThread {
        project_id,
        title,
        cwd: Path::new("/work/demo"),
        account_id: "work",
        status: "stopped",
    }
}

#[test]
fn side_chats_are_hidden_from_thread_lists_and_listed_under_their_parent() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let parent = s.create_thread(&new(p.id, "main")).expect("parent");
    let side = s
        .create_side_chat(&new(p.id, "Side: why?"), parent.id, "context\n")
        .expect("side chat");

    assert_eq!(side.side_of, Some(parent.id));
    assert_eq!(
        side.forked_from, None,
        "no history is copied, so it isn't a fork"
    );
    assert_eq!(side.account_id, "work");
    assert_eq!(side.cwd, Path::new("/work/demo"));
    assert_eq!(
        s.fork_context(side.id).expect("ctx").as_deref(),
        Some("context\n")
    );
    assert!(s.events_since(side.id, 0).expect("events").is_empty());

    let ids = |v: Vec<skyelab_store::Thread>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(ids(s.list_threads(p.id).expect("list")), vec![parent.id]);
    assert_eq!(ids(s.list_all_threads().expect("all")), vec![parent.id]);
    assert_eq!(ids(s.side_chats(parent.id).expect("side")), vec![side.id]);
    assert_eq!(
        s.thread(side.id).expect("row").expect("found").side_of,
        Some(parent.id)
    );
    // Its first session takes the context; once it has one, the context goes.
    s.set_agent_session_id(side.id, Some("sess"))
        .expect("session");
    assert_eq!(s.fork_context(side.id).expect("ctx"), None);
}

#[test]
fn a_parent_without_side_chats_lists_none_and_its_side_chats_survive_a_reopen() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("db.sqlite");
    let (parent, side) = {
        let mut s = Store::open(&path).expect("store");
        let p = s
            .create_project("demo", Path::new("/work/demo"))
            .expect("project");
        let parent = s.create_thread(&new(p.id, "main")).expect("parent");
        let other = s.create_thread(&new(p.id, "other")).expect("other");
        assert!(s.side_chats(other.id).expect("none").is_empty());
        let side = s
            .create_side_chat(&new(p.id, "Side"), parent.id, "ctx")
            .expect("side");
        (parent.id, side.id)
    };
    let s = Store::open(&path).expect("reopen");
    let listed: Vec<i64> = s
        .side_chats(parent)
        .expect("side")
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(listed, vec![side]);
}
