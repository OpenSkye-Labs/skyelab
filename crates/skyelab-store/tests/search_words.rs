//! Thread search matches every typed word (in any order), keeps quoted text as one phrase, reads FTS5 syntax as
//! plain text, and returns the matched line.

use std::path::Path;

use skyelab_store::{NewThread, Store};

fn new(project_id: i64, title: &str) -> NewThread<'_> {
    NewThread {
        project_id,
        title,
        cwd: Path::new("/work"),
        account_id: "acme",
        status: "idle",
    }
}

fn prompt(store: &mut Store, thread: i64, text: &str) {
    let json = serde_json::to_string(text).expect("json string");
    store.append_event(thread, "prompt", &json).expect("append");
}

fn ids(store: &Store, q: &str) -> Vec<i64> {
    let mut ids: Vec<i64> = store
        .search_threads(q, None, 50)
        .unwrap_or_else(|e| panic!("search {q:?} failed: {e:#}"))
        .into_iter()
        .map(|h| h.thread.id)
        .collect();
    ids.sort();
    ids
}

#[test]
fn words_any_order() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("a", Path::new("/a")).expect("p").id;
    let both = store.create_thread(&new(p, "one")).expect("t").id;
    let one = store.create_thread(&new(p, "two")).expect("t").id;
    let titled = store
        .create_thread(&new(p, "Parser crash on windows"))
        .expect("t")
        .id;
    prompt(
        &mut store,
        both,
        "the zebra ran far away from the hungry lion",
    );
    prompt(&mut store, one, "only a zebra here");

    assert_eq!(ids(&store, "zebra lion"), [both]);
    assert_eq!(ids(&store, "lion zebra"), [both], "any order");
    assert_eq!(ids(&store, "  ZEBRA   Lion "), [both], "case and spacing");
    assert_eq!(ids(&store, "zebra"), [both, one]);
    assert!(
        ids(&store, "zebra tiger").is_empty(),
        "every word is needed"
    );
    assert_eq!(ids(&store, "windows parser"), [titled], "title words too");
    // A word under three letters must still be in the same message.
    assert_eq!(ids(&store, "zebra ran"), [both]);
    assert_eq!(ids(&store, "a zebra"), [both, one]);
    assert!(ids(&store, "zebra xq").is_empty());
}

#[test]
fn quoted_phrase_exact() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("a", Path::new("/a")).expect("p").id;
    let together = store.create_thread(&new(p, "one")).expect("t").id;
    let apart = store.create_thread(&new(p, "two")).expect("t").id;
    prompt(&mut store, together, "please fix the hungry lion bug");
    prompt(&mut store, apart, "the lion is hungry");

    assert_eq!(ids(&store, "hungry lion"), [together, apart]);
    assert_eq!(ids(&store, "\"hungry lion\""), [together]);
    assert_eq!(ids(&store, "fix \"hungry lion\""), [together]);
    assert_eq!(
        ids(&store, "\"hungry lion"),
        [together],
        "an open quote runs to the end"
    );
    assert!(ids(&store, "\"lion hungry\"").is_empty());
}

#[test]
fn fts_syntax_is_literal() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("a", Path::new("/a")).expect("p").id;
    let plain = store.create_thread(&new(p, "one")).expect("t").id;
    let literal = store.create_thread(&new(p, "two")).expect("t").id;
    prompt(&mut store, plain, "foobar near the gate and some text");
    prompt(
        &mut store,
        literal,
        "run foo* -x col:val NEAR(a b) say \"hi\" (paren ^caret OR AND NOT",
    );

    for q in [
        "foo*",
        "NEAR",
        "NEAR(a b)",
        "-x",
        "\"",
        "\"\"",
        "col:val",
        "(",
        ")",
        "^caret",
        "AND",
        "OR",
        "NOT",
        "a OR b",
        "foo* -x",
        "{col}",
        "\"hi\"",
        "'",
        "\\",
        "%",
        "_",
        "*",
        "-",
        ":",
        "+",
        "NEAR/2",
    ] {
        store
            .search_threads(q, None, 50)
            .unwrap_or_else(|e| panic!("{q:?} errors: {e:#}"));
    }
    assert_eq!(ids(&store, "foo*"), [literal], "* is not a prefix search");
    assert_eq!(
        ids(&store, "foo* -x"),
        [literal],
        "a short word is literal text too"
    );
    assert_eq!(ids(&store, "col:val"), [literal], "no column filter");
    assert_eq!(ids(&store, "NEAR(a b)"), [literal]);
    assert_eq!(ids(&store, "^caret"), [literal]);
    assert_eq!(ids(&store, "say \"hi\""), [literal]);
    assert!(ids(&store, "%").is_empty());
}

#[test]
fn hidden_rows_multiword() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("a", Path::new("/a")).expect("p").id;
    let gone = store.create_project("b", Path::new("/b")).expect("p").id;
    let parent = store.create_thread(&new(p, "parent")).expect("t").id;
    let side = store
        .create_side_chat(&new(p, "Side: zebra lion"), parent, "ctx")
        .expect("side")
        .id;
    let deleted = store.create_thread(&new(p, "deleted")).expect("t").id;
    let removed = store.create_thread(&new(gone, "removed")).expect("t").id;
    for t in [side, deleted, removed] {
        prompt(&mut store, t, "the zebra met a lion");
    }
    store.set_thread_deleted(deleted, true).expect("delete");
    store.set_project_removed(gone, true).expect("remove");

    assert!(ids(&store, "zebra lion").is_empty());
    assert!(ids(&store, "\"zebra met\"").is_empty());
}

#[test]
fn snippet_shows_match() {
    let mut store = Store::open_in_memory().expect("store");
    let p = store.create_project("a", Path::new("/a")).expect("p").id;
    let t = store.create_thread(&new(p, "one")).expect("t").id;
    let titled = store.create_thread(&new(p, "zebra stripes")).expect("t").id;
    let long = format!(
        "{} the   zebra\nmet a lion {}",
        "lorem ipsum dolor ".repeat(10),
        "tail words ".repeat(30)
    );
    prompt(&mut store, t, "an older zebra and lion message");
    prompt(&mut store, t, &long);

    let hits = store
        .search_threads("lion zebra", None, 10)
        .expect("search");
    assert_eq!(hits.len(), 1);
    let snippet = hits[0]
        .snippet
        .as_deref()
        .expect("a message match has a snippet");
    assert!(snippet.starts_with('…'), "{snippet}");
    assert!(snippet.ends_with('…'), "{snippet}");
    assert!(
        snippet.contains("the zebra met a lion"),
        "newest message, spaces collapsed: {snippet}"
    );
    assert!(snippet.chars().count() <= 120, "{snippet}");

    let hits = store.search_threads("stripes", None, 10).expect("search");
    assert_eq!(hits[0].thread.id, titled);
    assert_eq!(hits[0].snippet, None, "a title match has no snippet");
}
