//! The sidebar's preview line: each thread's latest assistant text, from one query.

use std::path::Path;

use skyelab_store::{NewThread, Store};

fn thread(store: &Store, project_id: i64, title: &str) -> i64 {
    store
        .create_thread(&NewThread {
            project_id,
            title,
            cwd: Path::new("/work"),
            account_id: "acme",
            status: "idle",
        })
        .expect("thread")
        .id
}

fn reply(text: &str) -> String {
    format!(
        r#"{{"Assistant":{{"content":[{{"type":"tool_use","id":"t","name":"Bash","input":{{}}}},{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

#[test]
fn latest_assistant_text_per_thread() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("p", Path::new("/p")).expect("p").id;
    let answered = thread(&store, p, "answered");
    let silent = thread(&store, p, "silent");
    let deleted = thread(&store, p, "deleted");
    store
        .append_event(answered, "prompt", "\"go\"")
        .expect("append");
    store
        .append_event(answered, "agent", &reply("first answer"))
        .expect("append");
    store
        .append_event(answered, "prompt", "\"again\"")
        .expect("append");
    store
        .append_event(answered, "agent", &reply("second answer"))
        .expect("append");
    // Later events that carry no text (a tool result, the turn's end, a broken row) don't hide the reply.
    store
        .append_event(answered, "agent", r#"{"Result":{"is_error":false}}"#)
        .expect("append");
    store
        .append_event(answered, "agent", "not json")
        .expect("append");
    // A blank text block is not a reply either: the one before it is kept.
    store
        .append_event(answered, "agent", &reply("   "))
        .expect("append");
    store
        .append_event(silent, "prompt", "\"hello\"")
        .expect("append");
    store
        .append_event(deleted, "agent", &reply("gone"))
        .expect("append");
    store.set_thread_deleted(deleted, true).expect("delete");

    let replies = store.last_replies(None).expect("replies");
    assert_eq!(
        replies.get(&answered).map(String::as_str),
        Some("second answer")
    );
    assert!(!replies.contains_key(&silent), "a prompt is not a reply");
    assert!(!replies.contains_key(&deleted));
}

#[test]
fn long_replies_are_cut_in_the_query() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("p", Path::new("/p")).expect("p").id;
    let t = thread(&store, p, "long");
    store
        .append_event(t, "agent", &reply(&"é".repeat(5000)))
        .expect("append");
    let replies = store.last_replies(None).expect("replies");
    assert_eq!(replies[&t].chars().count(), 400);
}

#[test]
fn replies_can_be_scoped_to_one_project() {
    let mut store = Store::open_in_memory().expect("store");
    let a = store.create_project("a", Path::new("/a")).expect("a").id;
    let b = store.create_project("b", Path::new("/b")).expect("b").id;
    let in_a = thread(&store, a, "in a");
    let in_b = thread(&store, b, "in b");
    for (t, text) in [(in_a, "from a"), (in_b, "from b")] {
        store
            .append_event(t, "agent", &reply(text))
            .expect("append");
    }
    let replies = store.last_replies(Some(a)).expect("replies");
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies.get(&in_a).map(String::as_str), Some("from a"));
    assert_eq!(store.last_replies(None).expect("all").len(), 2);
}
