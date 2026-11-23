use skyelab_store::{NewThread, Store};
use std::path::Path;

#[test]
fn account_events_are_newest_first_and_only_that_accounts() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s
        .create_project("demo", Path::new("/demo"))
        .expect("project");
    let thread = |account_id| {
        s.create_thread(&NewThread {
            project_id: p.id,
            title: "t",
            cwd: Path::new("/demo"),
            account_id,
            status: "idle",
        })
        .expect("thread")
        .id
    };
    let (work, home) = (thread("work"), thread("home"));
    s.append_event(
        work,
        "agent",
        r#"{"Unknown":{"type":"rate_limit_event","n":1}}"#,
    )
    .expect("append");
    s.append_event(work, "agent", r#"{"Unknown":{"type":"other"}}"#)
        .expect("append");
    s.append_event(
        work,
        "agent",
        r#"{"Unknown":{"type":"rate_limit_event","n":2}}"#,
    )
    .expect("append");
    s.append_event(
        home,
        "agent",
        r#"{"Unknown":{"type":"rate_limit_event","n":3}}"#,
    )
    .expect("append");
    let found = s
        .account_events_matching("work", "agent", "rate_limit_event", 10)
        .expect("query");
    let payloads: Vec<&str> = found.iter().map(|e| e.payload_json.as_str()).collect();
    assert_eq!(payloads.len(), 2, "{payloads:?}");
    assert!(
        payloads[0].contains("\"n\":2"),
        "newest first: {payloads:?}"
    );
    assert!(
        s.account_events_matching("nobody", "agent", "rate_limit_event", 10)
            .expect("query")
            .is_empty()
    );
}
