//! Codex approval requests reach the session as `Event::Permission`, the turn waits, and the answer reaches
//! `codex app-server` (the fake). The mode picks the approval policy and sandbox. Nothing runs the real `codex`.
#![cfg(unix)]

use serde_json::{Value, json};
use skyelab_agent::{Decision, Event, Launch, Mode, Session, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn start(prompt: &str, mode: Mode) -> (Session, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let account = Account {
        id: "oai".into(),
        label: "OpenAI".into(),
        provider: Provider::Codex,
        config_dir: dir.join("codex-home"),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "oai".into())],
    };
    let bin: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh");
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt,
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode,
        untrusted: false,
    };
    (start_with_bin(&bin, &config, &launch).expect("start"), tmp)
}

fn next(s: &Session) -> Event {
    s.events()
        .recv_timeout(Duration::from_secs(60))
        .expect("event within 60 s")
}

/// The params of the first line the fake echoes back that `pick` accepts.
fn report(s: &Session, pick: impl Fn(&Value) -> bool) -> Value {
    loop {
        if let Event::Unknown(v) = next(s)
            && v["method"] == "fake/request"
            && pick(&v["params"])
        {
            return v["params"].clone();
        }
    }
}

#[test]
fn mode_sets_policy_on_thread_start() {
    for (mode, approval, sandbox) in [
        (Mode::Default, "untrusted", "workspace-write"),
        (Mode::Bypass, "never", "danger-full-access"),
    ] {
        let (s, _tmp) = start("hi", mode);
        let p = report(&s, |p| p["method"] == "thread/start");
        assert_eq!(p["params"]["approvalPolicy"], approval);
        assert_eq!(p["params"]["sandbox"], sandbox);
    }
}

#[test]
fn approval_request_waits_for_answer() {
    for (decision, wire) in [
        (Decision::AllowOnce, "accept"),
        (Decision::AllowThread, "acceptForSession"),
        (Decision::Deny, "decline"),
    ] {
        let (mut s, _tmp) = start("approve this", Mode::Default);
        let (id, tool, input) = loop {
            if let Event::Permission { id, tool, input } = next(&s) {
                break (id, tool, input);
            }
        };
        assert_eq!(tool, "shell");
        assert_eq!(input["command"], "make deploy");
        // The fake reads one line after asking: until we answer, the turn goes no further.
        let early = loop {
            match s.events().recv_timeout(Duration::from_millis(500)) {
                Ok(Event::Unknown(v)) if v["method"] == "fake/request" => continue,
                other => break other,
            }
        };
        assert!(early.is_err(), "the turn went on unanswered: {early:?}");
        s.answer(&id, decision).expect("answer");
        let rest: Vec<Event> = std::iter::from_fn(|| Some(next(&s)))
            .take_while(|e| !matches!(e, Event::Result { .. }))
            .collect();
        let reply = rest.iter().find_map(|e| match e {
            Event::Unknown(v) if v["method"] == "fake/request" && v["params"]["id"] == 99 => {
                Some(v["params"].clone())
            }
            _ => None,
        });
        assert_eq!(
            reply,
            Some(json!({ "id": 99, "result": { "decision": wire } }))
        );
        assert!(
            rest.iter().any(
                |e| matches!(e, Event::PermissionResolved { decision: d, .. } if *d == decision)
            ),
            "{rest:?}"
        );
    }
}

/// Each approval request in arrival order, with what SkyeLab did about it, until the turn's result.
fn until_result(s: &Session) -> Vec<Event> {
    std::iter::from_fn(|| Some(next(s)))
        .take_while(|e| !matches!(e, Event::Result { .. }))
        .collect()
}

#[test]
fn a_thread_prefix_allow_answers_the_next_matching_command() {
    let (mut s, _tmp) = start("approve2 please", Mode::Default);
    let (id, input) = loop {
        if let Event::Permission { id, input, .. } = next(&s) {
            break (id, input);
        }
    };
    assert_eq!(input["command"], "cargo test --lib");
    assert!(
        s.allow_thread_prefix(&id, "bash").is_err(),
        "a shell prefix allows anything"
    );
    s.allow_thread_prefix(&id, "cargo test").expect("allowed");
    let rest = until_result(&s);
    let replies: Vec<Value> = rest
        .iter()
        .filter_map(|e| match e {
            Event::Unknown(v)
                if v["method"] == "fake/request" && v["params"]["result"].is_object() =>
            {
                Some(v["params"].clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        replies,
        vec![
            json!({ "id": 99, "result": { "decision": "accept" } }),
            json!({ "id": 100, "result": { "decision": "accept" } }),
        ],
        "the second command was answered without asking: {rest:?}"
    );
    let resolved: Vec<_> = rest
        .iter()
        .filter_map(|e| match e {
            Event::PermissionResolved { decision, rule, .. } => Some((*decision, rule.clone())),
            _ => None,
        })
        .collect();
    let by = Some("shell · starts with cargo test".to_owned());
    assert_eq!(
        resolved,
        vec![
            (Decision::AllowThread, by.clone()),
            (Decision::AllowThread, by)
        ]
    );
}
