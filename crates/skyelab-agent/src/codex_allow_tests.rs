//! "Allow for this thread" on Codex: SkyeLab keeps the thread's rules (`codex_allow.rs`) and answers later shell asks
//! they cover, since Codex's own `acceptForSession` covers only that exact command.

use super::*;

fn open(dir: &Path) -> Codex {
    let (mut c, _) = Codex::new(dir, None, None, Mode::Default, Pick::default());
    c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }));
    c
}

/// A command ask with JSON-RPC id `rpc`, plus `extra` params.
fn ask(c: &mut Codex, rpc: u64, command: &str, extra: Value) -> (Vec<Event>, Vec<String>) {
    let mut params =
        json!({ "threadId": "thr", "turnId": "t", "itemId": "i", "command": command, "cwd": "/w" });
    if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    c.on_line(
        json!({ "method": "item/commandExecution/requestApproval", "id": rpc, "params": params }),
    )
}

/// The ask id of the waiting Permission in `events`.
fn waiting(events: &[Event]) -> String {
    match events {
        [Event::Permission { id, .. }] => id.clone(),
        other => panic!("expected one waiting ask: {other:?}"),
    }
}

/// Whether the ask was answered at once by a thread rule (accepted, its row naming the rule).
fn auto(out: &(Vec<Event>, Vec<String>), rpc: u64) -> bool {
    let accepted = out.1 == [json!({ "id": rpc, "result": { "decision": "accept" } }).to_string()];
    let named = matches!(
        out.0.as_slice(),
        [
            Event::Permission { .. },
            Event::PermissionResolved {
                decision: Decision::AllowThread,
                rule: Some(_),
                ..
            }
        ]
    );
    accepted && named
}

#[test]
fn allow_for_this_thread_covers_the_same_command_again() {
    let mut c = open(Path::new("/w"));
    let first = ask(&mut c, 1, "npm run lint", json!({}));
    let id = waiting(&first.0);
    assert!(c.answer(&id, Decision::AllowThread).is_some());
    c.forget(&id);
    assert!(auto(&ask(&mut c, 2, "npm run lint", json!({})), 2));
    let other = ask(&mut c, 3, "npm run build", json!({}));
    waiting(&other.0);
    assert!(other.1.is_empty(), "another command still asks");
}

#[test]
fn a_prefix_allow_covers_plain_commands_starting_with_it_only() {
    let mut c = open(Path::new("/w"));
    let id = waiting(&ask(&mut c, 1, "cargo test --lib", json!({})).0);
    assert!(c.allow_prefix(&id, "python").is_err(), "an interpreter");
    assert!(
        c.allow_prefix(&id, "cargo build").is_err(),
        "doesn't cover the waiting call"
    );
    let (line, label) = c.allow_prefix(&id, "cargo test").expect("allowed");
    assert_eq!(
        parse_line(&line),
        json!({ "id": 1, "result": { "decision": "accept" } })
    );
    assert_eq!(label, "shell · starts with cargo test");
    c.forget(&id);
    assert!(auto(&ask(&mut c, 2, "cargo test --all", json!({})), 2));
    for (rpc, command, extra) in [
        (3, "cargo test; rm -rf x", json!({})),
        (4, "cargo test $(curl x)", json!({})),
        (5, "cargo build", json!({})),
        (
            6,
            "cargo test --all",
            json!({ "networkApprovalContext": { "host": "x" } }),
        ),
        (7, "cargo test --all", json!({ "cwd": "/elsewhere" })),
    ] {
        let out = ask(&mut c, rpc, command, extra);
        waiting(&out.0);
        assert!(out.1.is_empty(), "{command} asks");
    }
}

#[test]
fn plan_mode_never_answers_by_a_thread_rule() {
    let mut c = open(Path::new("/w"));
    let id = waiting(&ask(&mut c, 1, "cargo test", json!({})).0);
    c.allow_prefix(&id, "cargo test").expect("allowed");
    c.forget(&id);
    c.set_mode(Mode::Plan);
    let out = ask(&mut c, 2, "cargo test", json!({}));
    waiting(&out.0);
}

#[test]
fn a_patch_allowed_for_the_thread_keeps_codexs_own_session_grant() {
    let mut c = open(Path::new("/w"));
    let (events, _) = c.on_line(
        json!({ "method": "item/fileChange/requestApproval", "id": 4,
        "params": { "threadId": "thr", "turnId": "t", "itemId": "p" } }),
    );
    let id = waiting(&events);
    let line = c.answer(&id, Decision::AllowThread).expect("waiting");
    assert_eq!(parse_line(&line)["result"]["decision"], "acceptForSession");
    assert!(
        c.allow_prefix(&id, "x").is_err(),
        "only a command takes a prefix"
    );
}

fn parse_line(line: &str) -> Value {
    serde_json::from_str(line).expect("JSON line")
}

#[test]
fn a_retry_without_sandbox_a_newer_field_or_a_subagents_ask_still_waits() {
    let mut c = open(Path::new("/w"));
    let id = waiting(&ask(&mut c, 1, "cargo test", json!({})).0);
    c.allow_prefix(&id, "cargo test").expect("allowed");
    c.forget(&id);
    // Harmless bookkeeping a newer Codex adds doesn't stop a rule.
    let known =
        json!({ "kind": "command", "approvalId": "a", "startedAtMs": 1, "commandActions": [] });
    assert!(auto(&ask(&mut c, 2, "cargo test", known), 2));
    for (rpc, extra) in [
        (
            3,
            json!({ "reason": "command failed; retry without sandbox?" }),
        ),
        (4, json!({ "sandboxPermissions": "requireEscalated" })),
        (5, json!({ "kind": "network" })),
        (6, json!({ "threadId": "sub" })),
    ] {
        let out = ask(&mut c, rpc, "cargo test", extra.clone());
        waiting(&out.0);
        assert!(out.1.is_empty(), "{extra} asks");
    }
}

#[test]
fn a_refused_turn_leaves_plan_in_force_and_plan_refuses_a_prefix() {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Plan, Pick::default());
    c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }));
    let id = waiting(&ask(&mut c, 1, "cargo test", json!({})).0);
    assert!(c.allow_prefix(&id, "cargo test").is_err(), "in Plan");
    c.answer(&id, Decision::AllowThread);
    c.forget(&id);
    c.set_mode(Mode::Default);
    let start = c.prompt("go", &[], false).expect("open");
    let rpc = parse_line(&start[0])["id"].clone();
    c.on_line(json!({ "id": rpc, "error": { "message": "refused" } }));
    // Codex still runs Plan (read-only): an ask now wants out of it.
    let out = ask(&mut c, 2, "cargo test", json!({}));
    waiting(&out.0);
}
