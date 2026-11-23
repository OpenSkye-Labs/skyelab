//! A message steered into a running turn is part of that turn: stored as a `steer` row, shown like a prompt, and the
//! turn's fold covers the work before and after it (T3's), while the steer itself stays visible.

use super::thread_model::{Item, Record, ToolItem, apply};
use super::timeline_fold::{fold_head, folded_by};

fn tool(id: &str) -> Item {
    Item::Tool(ToolItem {
        id: id.into(),
        name: "Bash".into(),
        input: "{}".into(),
        raw: serde_json::json!({}),
        result: Some("ok".into()),
        is_error: false,
    })
}

#[test]
fn a_steer_is_stored_as_its_own_row_kind() {
    let record = Record::Steer("also add tests".into());
    let (kind, payload) = record.to_row();
    assert_eq!(kind, "steer");
    assert_eq!(
        payload, "\"also add tests\"",
        "the text as a JSON string, like a prompt"
    );
    assert_eq!(Record::from_row(kind, &payload).expect("parses"), record);
    let mut items = Vec::new();
    apply(&mut items, &record);
    assert_eq!(items, vec![Item::Steer("also add tests".into())]);
}

#[test]
fn steer_stays_in_the_turns_fold() {
    let items = vec![
        Item::Prompt("go".into()),
        tool("a"),
        Item::Steer("also add tests".into()),
        tool("b"),
        Item::Text("done".into()),
        Item::Done {
            is_error: false,
            duration_ms: 4200,
            cost_usd: 0.0,
            usage: None,
            at_ms: Some(1),
        },
    ];
    assert_eq!(folded_by(&items, 1), Some(5), "work before the steer folds");
    assert_eq!(folded_by(&items, 3), Some(5), "work after the steer folds");
    assert_eq!(folded_by(&items, 2), None, "the steer itself stays visible");
    assert_eq!(folded_by(&items, 4), None, "the reply stays");
    assert_eq!(
        fold_head(&items, 5),
        Some(1),
        "one header, above the turn's first work row"
    );
}

#[test]
fn search_finds_a_steer() {
    let mut store = skyelab_store::Store::open_in_memory().expect("store");
    let p = store
        .create_project("demo", std::path::Path::new("/tmp/demo"))
        .expect("project");
    let t = store
        .create_thread(&skyelab_store::NewThread {
            project_id: p.id,
            title: "t",
            cwd: std::path::Path::new("/tmp/demo"),
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    let (kind, payload) = Record::Steer("mind the zebra".into()).to_row();
    store.append_event(t.id, kind, &payload).expect("append");
    let found = store.search_threads("zebra", None, 10).expect("search");
    assert_eq!(found.iter().map(|t| t.id).collect::<Vec<_>>(), vec![t.id]);
}
