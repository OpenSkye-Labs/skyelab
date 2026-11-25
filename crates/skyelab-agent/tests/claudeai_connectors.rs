//! Acceptance: a Claude agent turn loads the bound account's own MCP config and claude.ai connectors (no
//! `--strict-mcp-config`), refuses names that shadow SkyeLab's server, and shows an unconnected connector quietly.
//! Background calls (commit text, usage poll, login check) stay strict with claude.ai connectors turned off.
//! Nothing runs the real `claude`.
#![cfg(unix)]

use serde_json::Value;
use skyelab_agent::{
    Event, Launch, McpServer, generate_commit_message, login_status, start_with_bin, usage_limits,
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

const GRANT: &str = r#"{"mcpServers":{"skyelab":{"type":"stdio","command":"/bin/skyelab","args":["--mcp-proxy"]}}}"#;

struct Fixture {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    grant: PathBuf,
    config: AccountsConfig,
}

/// `project/` bound to Claude account `work`; account `home` exists too, with a connector of its own.
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let grant = root.join("grant.json");
    std::fs::write(&grant, GRANT).expect("grant");
    let accounts: Vec<Account> = ["work", "home"]
        .into_iter()
        .map(|id| {
            let config_dir = root.join(format!("{id}-config"));
            std::fs::create_dir_all(&config_dir).expect("config dir");
            Account {
                id: id.into(),
                label: id.into(),
                config_dir,
                provider: Provider::Claude,
                gh_config_dir: None,
                ssh_agent: false,
            }
        })
        .collect();
    std::fs::write(
        accounts[0].config_dir.join("fake-claudeai-status.json"),
        r#"[{"name":"claude.ai Slack","status":"connected","source":"claudeai"},{"name":"claude.ai Notion","status":"needs-auth","source":"claudeai"}]"#,
    )
    .expect("work's connectors");
    std::fs::write(
        accounts[0].config_dir.join("fake-mcp-status.json"),
        r#"[{"name":"linear","status":"needs-auth","source":"user"}]"#,
    )
    .expect("work's own server");
    std::fs::write(
        accounts[1].config_dir.join("fake-claudeai-status.json"),
        r#"[{"name":"claude.ai Home Drive","status":"connected","source":"claudeai"}]"#,
    )
    .expect("home's connectors");
    let config = AccountsConfig {
        accounts,
        bindings: vec![Binding::new(dir.clone(), "work".into())],
    };
    Fixture {
        _tmp: tmp,
        dir,
        grant,
        config,
    }
}

fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-app/tests/fixtures/fake-claude.sh")
}

fn work(f: &Fixture) -> &Account {
    &f.config.accounts[0]
}

fn claude_json(f: &Fixture, text: &str) {
    std::fs::write(work(f).config_dir.join(".claude.json"), text).expect("write .claude.json");
}

/// The servers the agent's init line reported, and the fake's `fake_env` line.
fn launch(f: &Fixture, grant: bool) -> (Vec<McpServer>, Value) {
    let launch = Launch {
        account: work(f),
        dir: &f.dir,
        resume: None,
        prompt: "hi",
        mcp_config: grant.then_some(f.grant.as_path()),
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    let session = start_with_bin(&fake_claude(), &f.config, &launch).expect("start");
    let mut reported = Vec::new();
    loop {
        match session.events().recv_timeout(Duration::from_secs(60)) {
            Ok(Event::Mcp {
                servers,
                launch: true,
                warning: None,
            }) if servers.iter().any(|s| s.status != "starting") => reported = servers,
            Ok(Event::Unknown(v)) if v["type"] == "fake_env" => return (reported, v),
            Ok(_) => {}
            Err(e) => panic!("no fake_env line: {e}"),
        }
    }
}

fn find<'a>(servers: &'a [McpServer], name: &str) -> Option<&'a McpServer> {
    servers.iter().find(|s| s.name == name)
}

/// The `--settings` JSON the agent got, if any.
fn settings_arg(env: &Value) -> Option<Value> {
    let args: Vec<Value> = env["argv"].as_array().expect("argv").clone();
    let i = args.iter().position(|a| a == "--settings")?;
    Some(serde_json::from_str(args[i + 1].as_str().expect("a string")).expect("settings JSON"))
}

#[test]
fn an_agent_turn_loads_its_own_accounts_connectors() {
    let f = fixture();
    claude_json(
        &f,
        r#"{"mcpServers":{"docs":{"command":"/bin/docs"},"skyelab__files":{"command":"/bin/f"},"skyelab":{"command":"/bin/s"}}}"#,
    );
    let (servers, env) = launch(&f, true);
    let args = env["args"].as_str().expect("args");
    assert_eq!(env["strict"], false, "{env}");
    assert_eq!(env["claudeai_env"], "", "connectors aren't turned off: {env}");
    assert!(args.contains("--setting-sources user"), "{args}");
    assert!(!args.contains("--strict-mcp-config"), "{args}");

    // A shadowing name is denied; SkyeLab's own exact name never is (ours replaces it).
    let settings = settings_arg(&env).expect("a --settings arg");
    assert_eq!(
        settings["deniedMcpServers"],
        serde_json::json!([{ "serverName": "skyelab__files" }]),
        "{settings}"
    );

    // Only this account's connectors; the unconnected one reads as not connected, not as a sign-in failure.
    let slack = find(&servers, "claude.ai Slack").expect("work's connector");
    assert_eq!(slack.status, "connected");
    let notion = find(&servers, "claude.ai Notion").expect("work's unconnected connector");
    assert_eq!(notion.status, "not-connected");
    assert_eq!(notion.state(), "Not connected");
    assert!(
        find(&servers, "claude.ai Home Drive").is_none(),
        "another account's connector: {servers:?}"
    );
    // A user server that needs sign-in keeps its Sign in.
    assert_eq!(find(&servers, "linear").expect("linear").status, "needs-auth");
}

#[test]
fn an_unreadable_config_or_no_grant_stays_strict() {
    let f = fixture();
    claude_json(&f, "{not json");
    let (servers, env) = launch(&f, true);
    assert_eq!(env["strict"], true, "{env}");
    assert!(settings_arg(&env).is_none(), "{env}");
    assert!(find(&servers, "claude.ai Slack").is_none(), "{servers:?}");

    let f = fixture();
    let (servers, env) = launch(&f, false);
    assert_eq!(env["strict"], true, "a side chat gets no servers: {env}");
    assert!(find(&servers, "claude.ai Slack").is_none(), "{servers:?}");
}

#[test]
fn background_calls_stay_strict_with_connectors_off() {
    let f = fixture();
    let msg = generate_commit_message(&fake_claude(), &f.config, work(&f), &f.dir, "Write a git commit message")
        .expect("message");
    assert!(msg.starts_with("feat(fake)"), "{msg}");
    let log = std::fs::read_to_string(f.dir.join(".fake-title-call")).expect("call logged");
    assert!(log.contains("arg=[--strict-mcp-config]"), "{log}");
    assert!(log.contains("claudeai_env=false\n"), "{log}");

    // The usage poll and the login check: a stand-in that records the env it got in the account dir.
    let probe = f.dir.join("probe.sh");
    std::fs::write(
        &probe,
        "#!/bin/sh\necho \"$* ENABLE_CLAUDEAI_MCP_SERVERS=$ENABLE_CLAUDEAI_MCP_SERVERS\" >> \"$CLAUDE_CONFIG_DIR/seen\"\necho '{\"loggedIn\":false}'\n",
    )
    .expect("probe");
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let _ = usage_limits(&probe, work(&f));
    let _ = login_status(&probe, work(&f));
    let seen = std::fs::read_to_string(work(&f).config_dir.join("seen")).expect("both ran");
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(lines.len(), 2, "{seen}");
    assert!(lines[0].contains("--strict-mcp-config"), "{seen}");
    for line in lines {
        assert!(line.ends_with("ENABLE_CLAUDEAI_MCP_SERVERS=false"), "{seen}");
    }
}
