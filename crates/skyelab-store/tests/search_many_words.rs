//! A long pasted query (a log, a paragraph, one huge word) still searches instead of failing in SQLite.

use skyelab_store::Store;

#[test]
fn a_long_pasted_query_still_searches() {
    let store = Store::open_in_memory().expect("store");
    store
        .search_threads(&"ab ".repeat(1100), None, 10)
        .expect("a thousand short words");
    store
        .search_threads(&"word ".repeat(1100), None, 10)
        .expect("a thousand long words");
    store
        .search_threads(&"x".repeat(60_000), None, 10)
        .expect("one huge word");
}

#[test]
fn short_words_alone_still_search_messages_as_a_phrase() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("a", std::path::Path::new("/a"))
        .expect("p")
        .id;
    let t = store
        .create_thread(&skyelab_store::NewThread {
            project_id: p,
            title: "one",
            cwd: std::path::Path::new("/work"),
            account_id: "acme",
            status: "idle",
        })
        .expect("t")
        .id;
    store
        .append_event(t, "prompt", "\"please go to PR #4 now\"")
        .expect("append");
    let found = |q: &str| store.search_threads(q, None, 10).expect("search").len();
    assert_eq!(found("go to"), 1);
    assert_eq!(found("PR #4"), 1);
    assert_eq!(found("to go"), 0, "short words only: written together");
}
