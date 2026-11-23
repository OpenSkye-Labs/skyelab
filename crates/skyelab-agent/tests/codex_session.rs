//! Acceptance: a Codex-bound folder runs `codex app-server` (here the fake) under its account, and its
//! notifications arrive as the same `Event`s `claude` produces. Nothing runs the real `codex`.
#![cfg(unix)]

use serde_json::Value;
use skyelab_agent::{Block, Event, Launch, Session, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh")
}

fn codex_account(dir: &Path) -> Account {
    Account {
        id: "oai".into(),
        label: "OpenAI".into(),
        provider: Provider::Codex,
        config_dir: dir.join("codex-home"),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    account: Account,
    config: AccountsConfig,
}

fn bound() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path())
        .expect("canonical")
        .join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let account = codex_account(&dir);
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "oai".into())],
    };
    Fixture {
        _tmp: tmp,
        dir,
        account,
        config,
    }
}

fn start(
    f: &Fixture,
    resume: Option<&str>,
    prompt: &str,
    mcp: Option<&Path>,
) -> anyhow::Result<Session> {
    let launch = Launch {
        account: &f.account,
        dir: &f.dir,
        resume,
        prompt,
        mcp_config: mcp,
    };
    start_with_bin(&fake(), &f.config, &launch)
}

fn next(rx: &flume::Receiver<Event>) -> Event {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("event within 10s")
}

/// Events up to and including the next `Result`, without the fake's own report lines.
fn turn(rx: &flume::Receiver<Event>, reports: &mut Vec<Value>) -> Vec<Event> {
    let mut out = Vec::new();
    loop {
        match next(rx) {
            Event::Unknown(v) if v["method"].as_str().is_some_and(|m| m.starts_with("fake/")) => {
                reports.push(v)
            }
            e @ Event::Result { .. } => {
                out.push(e);
                return out;
            }
            e => out.push(e),
        }
    }
}

fn requests<'a>(reports: &'a [Value], method: &str) -> Vec<&'a Value> {
    reports
        .iter()
        .filter(|r| r["method"] == "fake/request" && r["params"]["method"] == method)
        .map(|r| &r["params"])
        .collect()
}

fn env_report(reports: &[Value]) -> &Value {
    &reports
        .iter()
        .find(|r| r["method"] == "fake/env")
        .expect("fake/env report")["params"]
}

#[test]
fn a_turn_streams_as_claude_events_under_the_bound_codex_home() {
    let f = bound();
    let session = start(&f, None, "hi", None).expect("start");
    let mut reports = Vec::new();
    let events = turn(session.events(), &mut reports);
    assert_eq!(
        events,
        vec![
            Event::Init {
                session_id: "thr_fake".into(),
                model: "gpt-fake".into()
            },
            Event::Assistant {
                content: vec![Block::ToolUse {
                    id: "item_2".into(),
                    name: "shell".into(),
                    input: serde_json::json!({ "command": "/bin/zsh -lc ls" }),
                }]
            },
            Event::User {
                content: vec![Block::ToolResult {
                    tool_use_id: "item_2".into(),
                    content: "README.md\n".into(),
                    is_error: false,
                }]
            },
            Event::Assistant {
                content: vec![Block::Text {
                    text: "Hello from fake codex.".into()
                }]
            },
            Event::Result {
                is_error: false,
                duration_ms: 1530,
                total_cost_usd: 0.0,
                result: Some("Hello from fake codex.".into()),
            },
        ]
    );
    let env = env_report(&reports);
    assert_eq!(
        env["codex_home"],
        f.dir.join("codex-home").to_str().expect("utf-8")
    );
    assert_eq!(env["cwd"], f.dir.to_str().expect("utf-8"));
    let args = env["args"].as_str().expect("args");
    assert!(args.starts_with("app-server -c projects={"), "{args}");
    for p in f.dir.ancestors() {
        let entry = format!("\"{}\"={{trust_level=\"untrusted\"}}", p.display());
        assert!(args.contains(&entry), "{p:?} not untrusted in {args}");
    }
    let start = requests(&reports, "thread/start");
    assert_eq!(start.len(), 1);
    assert_eq!(start[0]["params"]["approvalPolicy"], "never");
    assert_eq!(start[0]["params"]["sandbox"], "workspace-write");
    assert_eq!(start[0]["params"]["cwd"], f.dir.to_str().expect("utf-8"));
    let turns = requests(&reports, "turn/start");
    assert_eq!(turns[0]["params"]["threadId"], "thr_fake");
    assert_eq!(turns[0]["params"]["input"][0]["text"], "hi");
}

#[test]
fn approval_requests_are_declined() {
    let f = bound();
    let session = start(&f, None, "approve this", None).expect("start");
    let mut reports = Vec::new();
    let events = turn(session.events(), &mut reports);
    assert!(matches!(
        events.last(),
        Some(Event::Result {
            is_error: false,
            ..
        })
    ));
    let reply = reports
        .iter()
        .find(|r| r["params"]["id"] == 99)
        .expect("an answer to the approval request");
    assert_eq!(reply["params"]["result"]["decision"], "decline");
}

#[test]
fn unbound_or_rebound_folder_starts_nothing() {
    let f = bound();
    let launch = Launch {
        account: &f.account,
        dir: &f.dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
    };
    let err = start_with_bin(&fake(), &AccountsConfig::default(), &launch)
        .err()
        .expect("unbound folder must not start");
    assert!(err.to_string().contains("not bound"), "{err:#}");
    let mut other = f.config.clone();
    other.accounts[0].provider = Provider::Claude;
    let err = start_with_bin(&fake(), &other, &launch)
        .err()
        .expect("account changed under us");
    assert!(
        format!("{err:#}").contains("nothing was started"),
        "{err:#}"
    );
}

#[test]
fn resume_reopens_the_thread_with_the_same_hardening() {
    let f = bound();
    let grant = f.dir.join("grant.json");
    std::fs::write(
        &grant,
        r#"{"mcpServers":{"skyelab":{"type":"stdio","command":"/bin/skyelab","args":["--mcp-proxy"],"env":{"SKYELAB_MCP_SOCKET":"/run/mcp.sock","SKYELAB_MCP_TOKEN":"secret-token"}}}}"#,
    )
    .expect("grant");
    let session = start(&f, Some("019a-old-thread"), "again", Some(&grant)).expect("start");
    let mut reports = Vec::new();
    let events = turn(session.events(), &mut reports);
    assert_eq!(
        events[0],
        Event::Init {
            session_id: "019a-old-thread".into(),
            model: "gpt-fake".into()
        }
    );
    assert!(
        requests(&reports, "thread/start").is_empty(),
        "a resume never starts a new thread"
    );
    let resume = requests(&reports, "thread/resume");
    let p = &resume[0]["params"];
    assert_eq!(p["threadId"], "019a-old-thread");
    assert_eq!(p["approvalPolicy"], "never");
    assert_eq!(p["sandbox"], "workspace-write");
    assert_eq!(
        p["config"]["mcp_servers"]["skyelab"]["command"],
        "/bin/skyelab"
    );
    assert_eq!(
        requests(&reports, "turn/start")[0]["params"]["threadId"],
        "019a-old-thread"
    );

    let err = start(&f, Some("--config=x"), "again", None)
        .err()
        .expect("flag-like id");
    assert!(err.to_string().contains("refusing"), "{err:#}");
}

#[test]
fn mcp_tools_go_over_stdin_never_argv() {
    let f = bound();
    let grant = f.dir.join("grant.json");
    std::fs::write(
        &grant,
        r#"{"mcpServers":{"skyelab":{"type":"stdio","command":"/bin/skyelab","args":["--mcp-proxy"],"env":{"SKYELAB_MCP_SOCKET":"/run/mcp.sock","SKYELAB_MCP_TOKEN":"secret-token"}}}}"#,
    )
    .expect("grant");
    let session = start(&f, None, "hi", Some(&grant)).expect("start");
    let mut reports = Vec::new();
    turn(session.events(), &mut reports);
    let opened = requests(&reports, "thread/start");
    let server = &opened[0]["params"]["config"]["mcp_servers"]["skyelab"];
    assert_eq!(server["command"], "/bin/skyelab");
    assert_eq!(server["args"], serde_json::json!(["--mcp-proxy"]));
    assert_eq!(server["env"]["SKYELAB_MCP_TOKEN"], "secret-token");
    assert_eq!(server["env"]["SKYELAB_MCP_SOCKET"], "/run/mcp.sock");
    let args = env_report(&reports)["args"].as_str().expect("args");
    assert!(!args.contains("secret-token"), "{args}");

    // Without a grant there is no server at all.
    let session = start(&f, None, "hi", None).expect("start");
    let mut reports = Vec::new();
    turn(session.events(), &mut reports);
    assert!(requests(&reports, "thread/start")[0]["params"]["config"].is_null());
}

#[test]
fn interrupt_ends_the_turn_and_the_thread_goes_on() {
    let f = bound();
    let mut session = start(&f, None, "hang here", None).expect("start");
    let rx = session.events().clone();
    let mut reports = Vec::new();
    // Wait until the turn runs, then interrupt it.
    while requests(&reports, "turn/start").is_empty() {
        if let Event::Unknown(v) = next(&rx) {
            reports.push(v);
        }
    }
    std::thread::sleep(Duration::from_millis(200)); // let turn/started arrive; an earlier interrupt must wait for it anyway
    session.interrupt().expect("interrupt");
    let events = turn(&rx, &mut reports);
    assert!(
        matches!(events.last(), Some(Event::Result { is_error: true, .. })),
        "{events:?}"
    );
    let interrupt = requests(&reports, "turn/interrupt");
    assert_eq!(
        interrupt[0]["params"],
        serde_json::json!({ "threadId": "thr_fake", "turnId": "turn_hang" })
    );
    assert_eq!(
        session.try_wait().expect("try_wait"),
        None,
        "the process stays up"
    );
    session.send_user_message("next").expect("follow-up");
    let events = turn(&rx, &mut reports);
    assert!(
        matches!(
            events.last(),
            Some(Event::Result {
                is_error: false,
                ..
            })
        ),
        "{events:?}"
    );
    session.kill().expect("kill");
}
