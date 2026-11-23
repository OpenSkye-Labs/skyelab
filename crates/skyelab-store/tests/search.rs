//! Acceptance (package D): thread search over titles and message text (prompts and assistant text).

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

fn ids(store: &Store, q: &str, project: Option<i64>) -> Vec<i64> {
    let mut ids: Vec<i64> = store
        .search_threads(q, project, 50)
        .expect("search")
        .into_iter()
        .map(|t| t.id)
        .collect();
    ids.sort();
    ids
}

#[test]
fn finds_threads_by_title_prompt_and_assistant_text_only() {
    let mut store = Store::open_in_memory().expect("store");
    let a = store.create_project("a", Path::new("/a")).expect("p").id;
    let b = store.create_project("b", Path::new("/b")).expect("p").id;
    let titled = thread(&store, a, "Fix the Parser");
    let prompted = thread(&store, a, "one");
    let answered = thread(&store, b, "two");
    let tooled = thread(&store, b, "three");

    store
        .append_event(prompted, "prompt", "\"please look at the zebra crash\"")
        .expect("append");
    store
        .append_event(
            answered,
            "agent",
            r#"{"Assistant":{"content":[{"type":"text","text":"The Walrus module is fine"}]}}"#,
        )
        .expect("append");
    store
        .append_event(
            tooled,
            "agent",
            r#"{"Assistant":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"narwhal"}}]}}"#,
        )
        .expect("append");
    // Malformed payloads are stored as before and simply not indexed.
    store
        .append_event(tooled, "agent", "not json")
        .expect("a bad payload still appends");

    assert_eq!(ids(&store, "parser", None), [titled], "title, any case");
    assert_eq!(ids(&store, "zebra", None), [prompted]);
    assert_eq!(ids(&store, "walrus", None), [answered], "assistant text");
    assert!(
        ids(&store, "narwhal", None).is_empty(),
        "tool input is not message text"
    );
    assert_eq!(
        ids(&store, "zebra", Some(b)),
        Vec::<i64>::new(),
        "project scope"
    );
    assert_eq!(ids(&store, "walrus", Some(b)), [answered]);
    // Under three characters only titles match (trigram needs three).
    assert_eq!(ids(&store, "on", None), [prompted]);
    assert!(ids(&store, "ze", None).is_empty());
    // LIKE wildcards in the query are literal.
    assert!(ids(&store, "%", None).is_empty());
    assert!(
        ids(&store, "", None).is_empty(),
        "an empty query finds nothing"
    );

    store.set_thread_deleted(prompted, true).expect("delete");
    assert!(
        ids(&store, "zebra", None).is_empty(),
        "deleted threads hide"
    );
    store.set_thread_deleted(prompted, false).expect("undo");
    store.delete_thread(prompted).expect("hard delete");
    assert!(ids(&store, "zebra", None).is_empty());
    store.set_project_removed(b, true).expect("remove");
    assert!(
        ids(&store, "walrus", None).is_empty(),
        "removed projects hide"
    );
}
