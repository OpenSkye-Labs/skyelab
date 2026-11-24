use super::*;
use crate::Block;

/// Queues a prompt on a thread that opened.
fn p(c: &mut Codex, text: &str) -> Vec<String> {
    c.prompt(text, &[], false).expect("thread is open")
}

fn parse(line: &str) -> Value {
    serde_json::from_str(line).expect("test line is JSON")
}

fn method(line: &str) -> String {
    parse(line)["method"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// A started thread with no turn yet.
fn open() -> Codex {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    let (events, writes) = c.on_line(
        json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }),
    );
    assert_eq!(
        events,
        vec![Event::Init {
            session_id: "thr".into(),
            model: "m".into()
        }]
    );
    assert!(writes.is_empty());
    c
}

#[test]
fn start_sends_initialize_then_thread_start_with_hardening() {
    let (_, lines) = Codex::new(
        Path::new("/w"),
        None,
        Some(json!({ "mcp_servers.skyelab": {} })),
        Mode::Default,
        Pick::default(),
    );
    let names: Vec<_> = lines.iter().map(|l| method(l)).collect();
    assert_eq!(names, ["initialize", "initialized", "thread/start"]);
    let p = &parse(&lines[2])["params"];
    assert_eq!(p["approvalPolicy"], "untrusted");
    assert_eq!(p["sandbox"], "workspace-write");
    assert_eq!(p["cwd"], "/w");
    assert!(p["config"]["mcp_servers.skyelab"].is_object());
}

#[test]
fn prompts_wait_for_the_thread_and_for_the_running_turn() {
    let (mut c, _) = Codex::new(
        Path::new("/w"),
        Some("thr"),
        None,
        Mode::Default,
        Pick::default(),
    );
    assert!(p(&mut c, "one").is_empty(), "no thread id yet");
    let (_, writes) =
        c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" } } }));
    assert_eq!(method(&writes[0]), "turn/start");
    assert_eq!(parse(&writes[0])["params"]["input"][0]["text"], "one");
    assert!(p(&mut c, "two").is_empty(), "one turn at a time");
    let (events, writes) = c.on_line(json!({ "method": "turn/completed", "params": { "turn": { "id": "t1", "status": "completed" } } }));
    assert!(matches!(
        events[..],
        [Event::Result {
            is_error: false,
            ..
        }]
    ));
    assert_eq!(parse(&writes[0])["params"]["input"][0]["text"], "two");
}

#[test]
fn interrupt_before_the_turn_id_waits_for_it() {
    let mut c = open();
    let turn = p(&mut c, "go");
    assert!(c.interrupt().is_empty());
    let id = parse(&turn[0])["id"].clone();
    let (_, writes) = c.on_line(json!({ "id": id, "result": { "turn": { "id": "t9" } } }));
    let p = &parse(&writes[0]);
    assert_eq!(p["method"], "turn/interrupt");
    assert_eq!(p["params"], json!({ "threadId": "thr", "turnId": "t9" }));
}

#[test]
fn idle_interrupt_does_nothing() {
    let mut c = open();
    assert!(c.interrupt().is_empty());
    assert!(!c.interrupt_wanted);
}

#[test]
fn approval_requests_wait_for_an_answer_and_others_are_refused() {
    let mut c = open();
    let (events, writes) = c.on_line(json!({
        "id": 7, "method": "item/commandExecution/requestApproval",
        "params": { "command": "make deploy", "cwd": "/w" },
    }));
    assert!(
        writes.is_empty(),
        "nothing goes back until the user answers"
    );
    assert_eq!(
        events,
        vec![Event::Permission {
            id: "codex-7".into(),
            tool: "shell".into(),
            input: json!({ "command": "make deploy", "cwd": "/w" }),
        }]
    );
    assert_eq!(c.answer("codex-8", Decision::AllowOnce), None);
    let reply = c.answer("codex-7", Decision::AllowThread).expect("open");
    c.forget("codex-7");
    assert_eq!(
        parse(&reply),
        json!({ "id": 7, "result": { "decision": "acceptForSession" } })
    );
    assert_eq!(c.answer("codex-7", Decision::Deny), None, "answered once");
    let (_, writes) =
        c.on_line(json!({ "id": "x", "method": "mcpServer/elicitation/request", "params": {} }));
    assert_eq!(parse(&writes[0])["id"], "x");
    assert!(parse(&writes[0])["error"]["message"].is_string());
}

#[test]
fn resolved_elsewhere_or_turn_end_withdraws_requests() {
    let mut c = open();
    c.on_line(json!({ "id": 3, "method": "item/fileChange/requestApproval", "params": {} }));
    c.on_line(json!({ "id": "s", "method": "item/fileChange/requestApproval", "params": {} }));
    let (events, _) = c.on_line(
        json!({ "method": "serverRequest/resolved", "params": { "threadId": "thr", "requestId": 3 } }),
    );
    let withdrawn = |id: &str| Event::PermissionResolved {
        id: id.into(),
        decision: Decision::Withdrawn,
    };
    assert_eq!(events, vec![withdrawn("codex-3")]);
    let (events, _) = c.on_line(
        json!({ "method": "turn/completed", "params": { "turn": { "id": "t", "status": "interrupted" } } }),
    );
    assert_eq!(events[0], withdrawn("codex-\"s\""));
    assert!(matches!(events[1], Event::Result { .. }));
}

#[test]
fn mode_sets_the_policy_and_a_change_rides_the_next_turn() {
    for (mode, approval, sandbox) in [
        (Mode::Default, "untrusted", "workspace-write"),
        (Mode::AcceptEdits, "untrusted", "workspace-write"),
        (Mode::Plan, "on-request", "read-only"),
        (Mode::Bypass, "never", "danger-full-access"),
    ] {
        let (_, lines) = Codex::new(Path::new("/w"), None, None, mode, Pick::default());
        let p = &parse(&lines[2])["params"];
        assert_eq!(
            (p["approvalPolicy"].as_str(), p["sandbox"].as_str()),
            (Some(approval), Some(sandbox))
        );
    }
    let mut c = open();
    c.set_mode(Mode::Plan);
    let turn = p(&mut c, "go");
    let params = &parse(&turn[0])["params"];
    assert_eq!(params["approvalPolicy"], "on-request");
    assert_eq!(params["sandboxPolicy"], json!({ "type": "readOnly" }));

    // A refused turn/start never applied it, so the next turn carries it again.
    let mut c = open();
    c.set_mode(Mode::Bypass);
    let turn = p(&mut c, "a");
    p(&mut c, "b");
    let id = parse(&turn[0])["id"].clone();
    let (_, writes) = c.on_line(json!({ "id": id, "error": { "code": 1, "message": "nope" } }));
    assert_eq!(parse(&writes[0])["params"]["approvalPolicy"], "never");
}

#[test]
fn failed_turn_start_reports_and_frees_the_queue() {
    let mut c = open();
    let turn = p(&mut c, "a");
    p(&mut c, "b");
    let id = parse(&turn[0])["id"].clone();
    let (events, writes) =
        c.on_line(json!({ "id": id, "error": { "code": 1, "message": "nope" } }));
    assert_eq!(events, vec![failed("nope".into())]);
    assert_eq!(parse(&writes[0])["params"]["input"][0]["text"], "b");
}

#[test]
fn thread_start_error_is_a_failed_result() {
    let (mut c, _) = Codex::new(
        Path::new("/w"),
        Some("gone"),
        None,
        Mode::Default,
        Pick::default(),
    );
    let (events, _) =
        c.on_line(json!({ "id": c.start_id, "error": { "code": 1, "message": "no such thread" } }));
    assert_eq!(events, vec![failed("no such thread".into())]);
}

#[test]
fn a_resumed_thread_without_a_rollout_is_gone() {
    let gone = "no rollout found for thread id thr-old";
    let (mut c, _) = Codex::new(
        Path::new("/w"),
        Some("thr-old"),
        None,
        Mode::Default,
        Pick::default(),
    );
    let (events, _) =
        c.on_line(json!({ "id": c.start_id, "error": { "code": -32600, "message": gone } }));
    assert_eq!(events, vec![Event::SessionGone, failed(gone.into())]);
    // A new thread refused with the same words isn't a gone session: nothing was resumed.
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    let (events, _) =
        c.on_line(json!({ "id": c.start_id, "error": { "code": -32600, "message": gone } }));
    assert_eq!(events, vec![failed(gone.into())]);
}

#[test]
fn items_map_to_tool_use_results_and_text() {
    let mut c = open();
    let cmd = json!({ "type": "commandExecution", "id": "i1", "command": "ls", "cwd": "/w", "status": "inProgress", "commandActions": [] });
    let (events, _) = c.on_line(json!({ "method": "item/started", "params": { "item": cmd } }));
    assert_eq!(
        events,
        vec![Event::Assistant {
            content: vec![Block::ToolUse {
                id: "i1".into(),
                name: "shell".into(),
                input: json!({ "command": "ls" })
            }]
        }]
    );
    let done = json!({ "type": "commandExecution", "id": "i1", "command": "ls", "status": "failed", "aggregatedOutput": "boom", "exitCode": 2 });
    let (events, _) = c.on_line(json!({ "method": "item/completed", "params": { "item": done } }));
    assert_eq!(
        events,
        vec![Event::User {
            content: vec![Block::ToolResult {
                tool_use_id: "i1".into(),
                content: json!("boom"),
                is_error: true
            }]
        }]
    );
    let msg = json!({ "type": "agentMessage", "id": "i2", "text": "done" });
    let (events, _) = c.on_line(json!({ "method": "item/completed", "params": { "item": msg } }));
    assert_eq!(
        events,
        vec![Event::Assistant {
            content: vec![Block::Text {
                text: "done".into()
            }]
        }]
    );
    let (events, _) = c.on_line(json!({ "method": "turn/completed", "params": { "turn": { "id": "t", "status": "completed", "durationMs": 5 } } }));
    assert_eq!(
        events,
        vec![Event::Result {
            is_error: false,
            duration_ms: 5,
            total_cost_usd: 0.0,
            result: Some("done".into()),
            prompts: 1,
            usage: None,
            turn_cost_usd: None,
        }]
    );
}

#[test]
fn mcp_calls_are_named_like_claudes() {
    let item = json!({ "type": "mcpToolCall", "id": "m", "server": "skyelab", "tool": "list_projects", "arguments": {}, "status": "inProgress" });
    let Some(Event::Assistant { content }) = started(&item) else {
        panic!("tool use");
    };
    assert!(
        matches!(&content[0], Block::ToolUse { name, .. } if name == "mcp__skyelab__list_projects")
    );
}

#[test]
fn deltas_and_status_are_dropped_unknown_methods_kept() {
    let mut c = open();
    for m in [
        "item/agentMessage/delta",
        "thread/tokenUsage/updated",
        "account/rateLimits/updated",
        "mcpServer/startupStatus/updated",
    ] {
        assert_eq!(
            c.on_line(json!({ "method": m, "params": {} })),
            (vec![], vec![]),
            "{m}"
        );
    }
    let (events, _) =
        c.on_line(json!({ "method": "error", "params": { "error": { "message": "x" } } }));
    assert!(matches!(events[..], [Event::Unknown(_)]));
}

#[test]
fn every_ancestor_is_untrusted() {
    let args = args(Path::new("/a/b")).expect("args");
    assert_eq!(args[0], "app-server");
    assert_eq!(args[1], "-c");
    let projects = args[2].to_str().expect("utf-8");
    for p in ["\"/\"=", "\"/a\"=", "\"/a/b\"="] {
        assert!(
            projects.contains(&format!("{p}{{trust_level=\"untrusted\"}}")),
            "{projects}"
        );
    }
}

#[test]
fn a_worktrees_main_repo_is_untrusted_too() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let main = root.join("main");
    let gitdir = main.join(".git/worktrees/wt");
    std::fs::create_dir_all(&gitdir).expect("mkdir");
    std::fs::write(gitdir.join("commondir"), "../..\n").expect("commondir");
    let wt = root.join("elsewhere/wt");
    std::fs::create_dir_all(wt.join("sub")).expect("mkdir");
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).expect(".git");
    assert_eq!(main_repo(&wt.join("sub")), Some(main.clone()));
    let args = args(&wt.join("sub")).expect("args");
    let main = json!(main.to_str().expect("utf-8")).to_string();
    assert!(args[2].to_str().expect("utf-8").contains(&main));
}

#[test]
fn a_thread_that_never_opened_refuses_prompts() {
    let (mut c, _) = Codex::new(
        Path::new("/w"),
        Some("gone"),
        None,
        Mode::Default,
        Pick::default(),
    );
    assert!(p(&mut c, "first").is_empty(), "waits for the thread");
    let (events, writes) =
        c.on_line(json!({ "id": c.start_id, "error": { "code": 1, "message": "gone" } }));
    assert_eq!((events, writes), (vec![failed("gone".into())], vec![]));
    assert!(c.prompt("again", &[], false).is_err());
}

#[test]
fn interrupt_while_the_thread_opens_stops_the_first_turn() {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    p(&mut c, "go");
    assert!(c.interrupt().is_empty());
    let (_, writes) =
        c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" } } }));
    let turn_req = parse(&writes[0])["id"].clone();
    let (_, writes) = c.on_line(json!({ "id": turn_req, "result": { "turn": { "id": "t1" } } }));
    assert_eq!(parse(&writes[0])["method"], "turn/interrupt");
}

/// A thread with turn `t` running for prompt "one".
fn running() -> Codex {
    let mut c = open();
    let writes = p(&mut c, "one");
    let req = parse(&writes[0])["id"].as_u64().expect("id");
    c.on_line(json!({ "id": req, "result": { "turn": { "id": "t" } } }));
    c
}

fn completed(c: &mut Codex) -> u32 {
    let (events, _) = c.on_line(json!({ "method": "turn/completed", "params": { "turn": { "id": "t", "status": "completed" } } }));
    match events.as_slice() {
        [Event::Result { prompts, .. }] => *prompts,
        other => panic!("expected one result, got {other:?}"),
    }
}

#[test]
fn a_steer_goes_into_the_running_turn_and_its_result_answers_both() {
    let mut c = running();
    let writes = c.prompt("two", &[], true).expect("steer");
    assert_eq!(method(&writes[0]), "turn/steer");
    let params = &parse(&writes[0])["params"];
    assert_eq!(params["expectedTurnId"], "t");
    assert_eq!(params["input"][0]["text"], "two");
    let req = parse(&writes[0])["id"].as_u64().expect("id");
    c.on_line(json!({ "id": req, "result": { "turnId": "t" } }));
    assert_eq!(completed(&mut c), 2);
}

#[test]
fn a_refused_steer_runs_as_its_own_turn_and_is_counted_once() {
    // Refused while its turn still runs: that turn answers one, the new turn the steer.
    let mut c = running();
    let writes = c.prompt("two", &[], true).expect("steer");
    let req = parse(&writes[0])["id"].as_u64().expect("id");
    c.on_line(json!({ "id": req, "error": { "message": "no active turn" } }));
    assert_eq!(completed(&mut c), 1);
    // Refused after its turn finished (and counted it): the turn it runs as answers none.
    let mut c = running();
    let writes = c.prompt("two", &[], true).expect("steer");
    let req = parse(&writes[0])["id"].as_u64().expect("id");
    assert_eq!(completed(&mut c), 2);
    let (_, writes) = c.on_line(json!({ "id": req, "error": { "message": "turn ended" } }));
    assert_eq!(method(&writes[0]), "turn/start");
    let start = parse(&writes[0])["id"].as_u64().expect("id");
    c.on_line(json!({ "id": start, "result": { "turn": { "id": "t" } } }));
    assert_eq!(completed(&mut c), 0);
}

#[test]
fn a_steer_with_no_running_turn_queues_like_a_prompt() {
    let mut c = open();
    let writes = c.prompt("one", &[], true).expect("steer");
    assert_eq!(method(&writes[0]), "turn/start");
}
