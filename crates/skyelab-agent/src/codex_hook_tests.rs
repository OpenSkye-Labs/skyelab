//! Codex's `hook/started` / `hook/completed`: the account's own hooks, shown while they run.

use super::*;

fn open() -> Codex {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }));
    c
}

/// Codex 0.172's real line (real-run-3), with the given method, thread and status.
fn hook(method: &str, thread: &str, status: &str, message: Value) -> Value {
    json!({ "method": method, "params": { "threadId": thread, "turnId": "t1", "run": {
        "id": "session-start:6:/home/.codex/hooks.json", "eventName": "sessionStart",
        "handlerType": "command", "executionMode": "sync", "scope": "thread", "source": "user",
        "status": status, "statusMessage": message, "completedAt": null } } })
}

#[test]
fn hooks_start_and_finish() {
    let mut c = open();
    let (events, writes) = c.on_line(hook(
        "hook/started",
        "thr",
        "running",
        json!("Reporting status to Orca"),
    ));
    assert!(writes.is_empty());
    assert_eq!(
        events,
        vec![Event::Hook {
            id: "session-start:6:/home/.codex/hooks.json".into(),
            name: "sessionStart (Reporting status to Orca)".into(),
            running: true,
        }]
    );
    let (events, _) = c.on_line(hook("hook/completed", "thr", "completed", Value::Null));
    assert_eq!(
        events,
        vec![Event::Hook {
            id: "session-start:6:/home/.codex/hooks.json".into(),
            name: "sessionStart".into(),
            running: false,
        }]
    );
}

#[test]
fn hooks_of_another_thread_are_dropped_but_one_before_the_thread_opens_is_kept() {
    let mut c = open();
    let (events, _) = c.on_line(hook("hook/started", "sub", "running", Value::Null));
    assert!(events.is_empty(), "a subagent's hook: {events:?}");
    let (mut fresh, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    let (events, _) = fresh.on_line(hook("hook/started", "thr", "running", Value::Null));
    assert_eq!(events.len(), 1, "no thread id yet: {events:?}");
}
