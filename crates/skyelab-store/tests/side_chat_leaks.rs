//! Side chats stay out of thread search, and go with their parent when it is deleted.

use skyelab_store::{NewThread, Store};
use std::path::Path;

fn new(project_id: i64, title: &str) -> NewThread<'_> {
    NewThread {
        project_id,
        title,
        cwd: Path::new("/work/demo"),
        account_id: "work",
        status: "stopped",
    }
}

#[test]
fn search_leaves_side_chats_out() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let parent = s.create_thread(&new(p.id, "zebra main")).expect("parent");
    let side = s
        .create_side_chat(&new(p.id, "Side: zebra question"), parent.id, "ctx")
        .expect("side");
    let found: Vec<i64> = s
        .search_threads("zebra", None, 10)
        .expect("search")
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(found, vec![parent.id], "side chat {} in search", side.id);
}

#[test]
fn deleting_or_purging_a_parent_removes_its_side_chats() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    for purge in [false, true] {
        let parent = s.create_thread(&new(p.id, "main")).expect("parent");
        let side = s
            .create_side_chat(&new(p.id, "Side"), parent.id, "User: a pasted token\n")
            .expect("side");
        s.append_event(side.id, "prompt", "\"why?\"")
            .expect("event");
        if purge {
            s.set_thread_deleted(parent.id, true).expect("mark");
            s.purge_thread(parent.id).expect("purge");
        } else {
            s.delete_thread(parent.id).expect("delete");
        }
        assert!(s.thread(side.id).expect("read").is_none(), "purge={purge}");
        assert!(s.events_since(side.id, 0).expect("events").is_empty());
    }
}
