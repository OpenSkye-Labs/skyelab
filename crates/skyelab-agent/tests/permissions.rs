#![cfg(unix)]
//! Permission asks from `claude` (stream-json `can_use_tool` control requests) reach the session's events, wait for
//! an answer, and the answer reaches the agent. The fake reports the reply it got as a `fake_answer` line.

use serde_json::{Value, json};
use skyelab_agent::{Decision, Event, Launch, Mode, Session, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

fn start(name: &str, prompt: &str, mode: Mode) -> (Session, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical tempdir");
    let _ = name;
    let account = Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: dir.join("account"),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let cfg = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "acme".into())],
    };
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
    };
    (
        start_with_bin(&fake_bin(), &cfg, &launch).expect("start fake"),
        tmp,
    )
}

/// Events until `pick` matches one, which it returns.
fn until<T>(s: &Session, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
    loop {
        let e = s
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("event within 60 s");
        if let Some(t) = pick(&e) {
            return t;
        }
    }
}

fn unknown(e: &Event, ty: &str) -> Option<Value> {
    match e {
        Event::Unknown(v) if v["type"] == ty => Some(v.clone()),
        _ => None,
    }
}

fn args(s: &Session) -> String {
    until(s, |e| unknown(e, "fake_env"))["args"]
        .as_str()
        .expect("args")
        .to_owned()
}

#[test]
fn claude_mode_flags() {
    let cases = [
        (Mode::Default, "--permission-mode default"),
        (Mode::Plan, "--permission-mode plan"),
        (Mode::AcceptEdits, "--permission-mode acceptEdits"),
        (
            Mode::Bypass,
            "--permission-mode bypassPermissions --allow-dangerously-skip-permissions",
        ),
    ];
    for (mode, flag) in cases {
        let (s, _tmp) = start("mode", "hi", mode);
        let args = args(&s);
        assert!(
            args.contains("--permission-prompt-tool stdio"),
            "{mode:?}: {args}"
        );
        assert!(args.contains(flag), "{mode:?}: {args}");
    }
}

/// Starts with a prompt that asks, and returns the ask's id after checking it.
fn asked(prompt: &str) -> (Session, tempfile::TempDir, String) {
    let (s, tmp) = start("ask", prompt, Mode::Default);
    let (id, tool, input) = until(&s, |e| match e {
        Event::Permission { id, tool, input } => Some((id.clone(), tool.clone(), input.clone())),
        _ => None,
    });
    // The CLI's request id, after this session's nonce.
    assert!(
        id.ends_with("-perm-1") && id.len() > "-perm-1".len(),
        "{id}"
    );
    assert_eq!(tool, "Bash");
    assert_eq!(input, json!({ "command": "make deploy" }));
    (s, tmp, id)
}

fn reply(s: &Session) -> Value {
    until(s, |e| unknown(e, "fake_answer"))["reply"].clone()
}

#[test]
fn claude_allow_reaches_agent_and_waits() {
    let (mut s, _tmp, id) = asked("permit please");
    // Unanswered, the agent sends nothing more.
    let early = s.events().recv_timeout(Duration::from_millis(500));
    assert!(early.is_err(), "the agent went on unanswered: {early:?}");
    s.answer(&id, Decision::AllowOnce).expect("answer");
    // The agent's reply can come before or after the resolved event (sent once the answer is queued).
    let (mut resolved, mut r) = (None, None);
    while resolved.is_none() || r.is_none() {
        match s
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("event within 60 s")
        {
            Event::PermissionResolved { id, decision, .. } => resolved = Some((id, decision)),
            Event::Unknown(v) if v["type"] == "fake_answer" => r = Some(v["reply"].clone()),
            _ => {}
        }
    }
    assert_eq!(resolved, Some((id.clone(), Decision::AllowOnce)));
    let r = r.expect("reply");
    assert_eq!(r["type"], "control_response");
    assert_eq!(r["response"]["request_id"], "perm-1");
    assert_eq!(
        r["response"]["response"],
        json!({ "behavior": "allow", "updatedInput": { "command": "make deploy" } })
    );
}

/// Each answer the turn ends on (the agent reads it and ends at once) reaches the thread as given, before the turn's
/// end: never withdrawn by that end first, which left the row without its decision or rule.
#[test]
fn an_answer_the_turn_ends_on_is_never_withdrawn() {
    for i in 0..20 {
        let (mut s, _tmp, id) = asked("permit-end please");
        let rule = format!("rule {i}");
        s.answer_by_rule(&id, rule.clone()).expect("answer");
        let mut resolved = Vec::new();
        loop {
            match s
                .events()
                .recv_timeout(Duration::from_secs(60))
                .expect("event within 60 s")
            {
                Event::PermissionResolved { decision, rule, .. } => resolved.push((decision, rule)),
                Event::Result { .. } => break,
                _ => {}
            }
        }
        assert_eq!(resolved, [(Decision::AllowOnce, Some(rule))], "run {i}");
    }
}

#[test]
fn claude_deny_reaches_agent() {
    let (mut s, _tmp, id) = asked("permit please");
    s.answer(&id, Decision::Deny).expect("answer");
    let r = reply(&s);
    assert_eq!(r["response"]["response"]["behavior"], "deny");
    assert!(r["response"]["response"]["message"].is_string());
}

#[test]
fn claude_allow_thread_sends_session_rules() {
    let (mut s, _tmp, id) = asked("permit please");
    s.answer(&id, Decision::AllowThread).expect("answer");
    let r = reply(&s);
    let rules = &r["response"]["response"]["updatedPermissions"];
    assert_eq!(
        rules,
        &json!([{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "make deploy" }],
                  "behavior": "allow", "destination": "session" }]),
        "the suggestion, kept to this session, never written to settings"
    );
}

#[test]
fn answer_unknown_id_fails() {
    let (mut s, _tmp, id) = asked("permit please");
    assert!(s.answer("perm-9", Decision::AllowOnce).is_err());
    assert!(
        s.answer("perm-1", Decision::AllowOnce).is_err(),
        "the bare CLI id isn't ours"
    );
    assert!(s.answer(&id, Decision::Withdrawn).is_err());
    s.answer(&id, Decision::Deny).expect("first answer");
    assert!(s.answer(&id, Decision::AllowOnce).is_err(), "answered once");
}

#[test]
fn claude_cancel_withdraws() {
    let (mut s, _tmp) = start("withdraw", "withdraw it", Mode::Default);
    let resolved = until(&s, |e| match e {
        Event::PermissionResolved { id, decision, .. } => Some((id.clone(), *decision)),
        _ => None,
    });
    assert!(resolved.0.ends_with("-perm-2"), "{resolved:?}");
    assert_eq!(resolved.1, Decision::Withdrawn);
    assert!(s.answer(&resolved.0, Decision::AllowOnce).is_err());
}
