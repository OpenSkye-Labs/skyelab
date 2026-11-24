//! Acceptance: an agent gets the bound account's own (user-scope) MCP servers next to SkyeLab's, never the repo's
//! or another account's, and never under SkyeLab's name. Nothing runs the real `claude` or `codex`.
#![cfg(unix)]

use serde_json::{Value, json};
use skyelab_agent::{Event, Launch, McpServer, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

const GRANT: &str = r#"{"mcpServers":{"skyelab":{"type":"stdio","command":"/bin/skyelab","args":["--mcp-proxy"],"env":{"SKYELAB_MCP_TOKEN":"t"}}}}"#;

struct Fixture {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    grant: PathBuf,
    config: AccountsConfig,
}

fn account(root: &Path, id: &str, provider: Provider) -> Account {
    let config_dir = root.join(format!("{id}-config"));
    std::fs::create_dir_all(&config_dir).expect("config dir");
    Account {
        id: id.into(),
        label: id.into(),
        config_dir,
        provider,
        gh_config_dir: None,
        ssh_agent: false,
    }
}

/// `project` bound to account `work`; account `home` exists too.
fn fixture(provider: Provider) -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let grant = root.join("grant.json");
    std::fs::write(&grant, GRANT).expect("grant");
    let config = AccountsConfig {
        accounts: vec![
            account(&root, "work", provider),
            account(&root, "home", provider),
        ],
        bindings: vec![Binding::new(dir.clone(), "work".into())],
    };
    Fixture {
        _tmp: tmp,
        dir,
        grant,
        config,
    }
}

fn claude_json(f: &Fixture, id: &str, v: &str) {
    let a = f
        .config
        .accounts
        .iter()
        .find(|a| a.id == id)
        .expect("account");
    std::fs::write(a.config_dir.join(".claude.json"), v).expect("write .claude.json");
}

/// Every event up to the fake's `fake_env` line, and the `--mcp-config` file it got (`fake_mcp`).
fn run(f: &Fixture, bin: &str) -> (Vec<Event>, Value, String) {
    let work = &f.config.accounts[0];
    let launch = Launch {
        account: work,
        dir: &f.dir,
        resume: None,
        prompt: "hi",
        mcp_config: Some(&f.grant),
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(bin);
    let session = start_with_bin(&bin, &f.config, &launch).expect("start");
    let (mut events, mut mcp, mut args) = (Vec::new(), Value::Null, String::new());
    loop {
        match session.events().recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Unknown(v)) if v["type"] == "fake_mcp" => {
                mcp = v["config"].clone();
                return (events, mcp, args);
            }
            Ok(Event::Unknown(v)) if v["type"] == "fake_env" => {
                args = v["args"].as_str().expect("args").to_owned();
            }
            Ok(Event::Unknown(v)) if v["method"] == "fake/request" => {
                if v["params"]["method"] == "thread/start" {
                    mcp = v["params"]["params"]["config"].clone();
                    return (events, mcp, args);
                }
            }
            Ok(e) => events.push(e),
            Err(e) => panic!("no fake_mcp line (args: {args}, mcp: {mcp}): {e}"),
        }
    }
}

fn names(config: &Value) -> Vec<&str> {
    let mut names: Vec<&str> = config["mcpServers"]
        .as_object()
        .expect("mcpServers")
        .keys()
        .map(String::as_str)
        .collect();
    names.sort();
    names
}

fn launch_report(events: &[Event]) -> (&[McpServer], Option<&str>) {
    match events.first() {
        Some(Event::Mcp {
            servers,
            launch: true,
            warning,
        }) => (servers, warning.as_deref()),
        other => panic!("the first event isn't the launch report: {other:?}"),
    }
}

#[test]
fn user_servers_merge_into_the_grant_project_ones_never() {
    let f = fixture(Provider::Claude);
    claude_json(
        &f,
        "work",
        &json!({
            "mcpServers": { "linear": { "type": "http", "url": "https://mcp.linear.app/mcp" },
                            "files": { "command": "/bin/files-mcp", "env": { "ROOT": "/data" } } },
            "projects": { f.dir.to_str().expect("utf8"): { "mcpServers": { "local-only": { "command": "/bin/x" } } } }
        })
        .to_string(),
    );
    claude_json(
        &f,
        "home",
        r#"{"mcpServers":{"personal":{"command":"/bin/p"}}}"#,
    );
    std::fs::write(
        f.dir.join(".mcp.json"),
        r#"{"mcpServers":{"repo":{"command":"/bin/evil"}}}"#,
    )
    .expect(".mcp.json");

    let (events, config, args) = run(&f, "fake-claude.sh");
    assert_eq!(names(&config), ["files", "linear", "skyelab"], "{config}");
    assert_eq!(config["mcpServers"]["files"]["env"]["ROOT"], "/data");
    assert_eq!(config["mcpServers"]["skyelab"]["command"], "/bin/skyelab");
    assert!(args.contains("--strict-mcp-config"), "{args}");
    let (servers, warning) = launch_report(&events);
    let mut reported: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
    reported.sort();
    assert_eq!(reported, ["files", "linear", "skyelab"]);
    assert!(servers.iter().all(|s| s.status == "starting"));
    assert_eq!(warning, None);
}

#[test]
fn skyelab_name_is_never_shadowed() {
    let f = fixture(Provider::Claude);
    claude_json(
        &f,
        "work",
        r#"{"mcpServers":{"skyelab":{"command":"/bin/fake-skyelab"},"SkyeLab":{"command":"/bin/fake2"},"docs":{"command":"/bin/docs"}}}"#,
    );
    let (_, config, args) = run(&f, "fake-claude.sh");
    assert_eq!(names(&config), ["docs", "skyelab"], "{config}");
    assert_eq!(config["mcpServers"]["skyelab"]["command"], "/bin/skyelab");
    // Only SkyeLab's own tools go ahead without asking; the account's servers ask like any tool.
    let words: Vec<&str> = args.split_whitespace().collect();
    let allowed: Vec<&str> = words
        .windows(2)
        .filter(|w| w[0] == "--allowedTools")
        .map(|w| w[1])
        .collect();
    assert_eq!(allowed, ["mcp__skyelab"], "{args}");
}

#[test]
fn malformed_claude_json_starts_with_a_warning() {
    for bad in [
        "{not json",
        r#"{"mcpServers":["x"]}"#,
        r#"{"mcpServers":{"x":"not an object"}}"#,
        "[]",
    ] {
        let f = fixture(Provider::Claude);
        claude_json(&f, "work", bad);
        let (events, config, _) = run(&f, "fake-claude.sh");
        assert_eq!(names(&config), ["skyelab"], "{bad}: {config}");
        let (servers, warning) = launch_report(&events);
        assert_eq!(servers.len(), 1, "{bad}");
        let warning = warning.unwrap_or_else(|| panic!("{bad}: no warning"));
        assert!(warning.contains(".claude.json"), "{bad}: {warning}");
    }
}

#[test]
fn codex_gets_skyelab_as_one_dotted_entry() {
    let f = fixture(Provider::Codex);
    let (events, config, _) = run(&f, "fake-codex.sh");
    // A whole `mcp_servers` table would replace the servers in the account's own config.toml.
    assert_eq!(
        config
            .as_object()
            .expect("config")
            .keys()
            .collect::<Vec<_>>(),
        ["mcp_servers.skyelab"],
        "{config}"
    );
    assert_eq!(config["mcp_servers.skyelab"]["command"], "/bin/skyelab");
    let (servers, _) = launch_report(&events);
    assert_eq!(servers[0].name, "skyelab");
}
