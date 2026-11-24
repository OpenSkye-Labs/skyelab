//! Acceptance: a Claude agent gets the MCP servers of the bound account's enabled plugins, named as Claude Code names
//! them (`plugin:<plugin>:<server>`), next to SkyeLab's. Never a disabled plugin's, a project-only install's or another
//! account's. The fixture is a redacted copy of a real config dir (Claude Code 2.1.331). Nothing runs the real `claude`.
#![cfg(unix)]

use serde_json::Value;
use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

const GRANT: &str = r#"{"mcpServers":{"skyelab":{"type":"stdio","command":"/bin/skyelab","args":["--mcp-proxy"],"env":{"SKYELAB_MCP_TOKEN":"t"}}}}"#;

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    dir: PathBuf,
    grant: PathBuf,
    config: AccountsConfig,
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}

/// `project` bound to account `work`, whose config dir is the fixture; its plugin cache is another config dir's (`other-config/plugins/cache`), as
/// a real `installPath` into another config dir does. Account `home` exists too.
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let grant = root.join("grant.json");
    std::fs::write(&grant, GRANT).expect("grant");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-plugins");
    let work = root.join("work-config");
    copy_tree(&src, &work);
    let shared = root.join("other-config/plugins/cache");
    std::fs::create_dir_all(root.join("other-config/plugins")).expect("mkdir");
    std::fs::rename(work.join("plugins/cache"), &shared).expect("move cache");
    let installed = work.join("plugins/installed_plugins.json");
    let text = std::fs::read_to_string(&installed).expect("installed");
    std::fs::write(
        &installed,
        text.replace("@CACHE@", shared.to_str().expect("utf8")),
    )
    .expect("write");
    let home = root.join("home-config");
    std::fs::create_dir_all(home.join("plugins/cache/p/personal/1/.claude-plugin")).expect("mkdir");
    std::fs::write(
        home.join("settings.json"),
        r#"{"enabledPlugins":{"personal@p":true}}"#,
    )
    .expect("settings");
    std::fs::write(
        home.join("plugins/installed_plugins.json"),
        format!(
            r#"{{"version":2,"plugins":{{"personal@p":[{{"scope":"user","installPath":"{}"}}]}}}}"#,
            home.join("plugins/cache/p/personal/1").display()
        ),
    )
    .expect("installed");
    std::fs::write(
        home.join("plugins/cache/p/personal/1/.mcp.json"),
        r#"{"mcpServers":{"personal":{"command":"/bin/personal"}}}"#,
    )
    .expect("mcp");
    let account = |id: &str, config_dir: PathBuf| Account {
        id: id.into(),
        label: id.into(),
        config_dir,
        provider: Provider::Claude,
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account("work", work), account("home", home)],
        bindings: vec![Binding::new(dir.clone(), "work".into())],
    };
    Fixture {
        _tmp: tmp,
        root,
        dir,
        grant,
        config,
    }
}

/// The launch report's names and warning, and the `--mcp-config` file the fake got.
fn run(f: &Fixture) -> (Vec<String>, Option<String>, Value) {
    let launch = Launch {
        account: &f.config.accounts[0],
        dir: &f.dir,
        resume: None,
        prompt: "hi",
        mcp_config: Some(&f.grant),
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let session = start_with_bin(&bin, &f.config, &launch).expect("start");
    let mut report = None;
    loop {
        match session.events().recv_timeout(Duration::from_secs(60)) {
            Ok(Event::Unknown(v)) if v["type"] == "fake_mcp" => {
                let (names, warning) = report.expect("the launch report comes first");
                return (names, warning, v["config"].clone());
            }
            Ok(Event::Mcp {
                servers,
                launch: true,
                warning,
            }) => {
                let mut names: Vec<String> = servers.into_iter().map(|s| s.name).collect();
                names.sort();
                report = Some((names, warning));
            }
            Ok(_) => {}
            Err(e) => panic!("no fake_mcp line: {e}"),
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

const LOADED: [&str; 4] = [
    "plugin:aikido:aikido-mcp",
    "plugin:posthog:posthog",
    "plugin:rooty:tools",
    "skyelab",
];

#[test]
fn enabled_plugin_servers_merge_into_the_grant() {
    let f = fixture();
    let (reported, _, config) = run(&f);
    assert_eq!(names(&config), LOADED, "{config}");
    assert_eq!(reported, LOADED);
    let servers = &config["mcpServers"];
    assert_eq!(servers["plugin:aikido:aikido-mcp"]["command"], "npx");
    assert_eq!(servers["plugin:posthog:posthog"]["type"], "http");
    assert_eq!(
        servers["plugin:posthog:posthog"]["url"],
        "https://mcp.posthog.com/mcp"
    );
    // ${CLAUDE_PLUGIN_ROOT} is the install dir, ${CLAUDE_PLUGIN_DATA} the account's own data dir for that plugin.
    let rooty = f.root.join("other-config/plugins/cache/local/rooty/1.0.0");
    let data = f.config.accounts[0]
        .config_dir
        .join("plugins/data/rooty-local");
    let tools = &servers["plugin:rooty:tools"];
    assert_eq!(tools["command"], format!("{}/bin/tools", rooty.display()));
    assert_eq!(tools["args"][1], format!("{}/db", data.display()));
    assert_eq!(tools["env"]["HOME_HINT"], rooty.display().to_string());
    assert_eq!(servers["skyelab"]["command"], "/bin/skyelab");
}

#[test]
fn only_enabled_user_scope_plugins_of_the_bound_account_load() {
    let f = fixture();
    let (_, _, config) = run(&f);
    let text = config.to_string();
    for absent in ["off-server", "proj-server", "personal"] {
        assert!(!text.contains(absent), "{absent} loaded: {config}");
    }
}

#[test]
fn a_broken_plugin_is_left_out_with_a_warning() {
    let f = fixture();
    let (reported, warning, _) = run(&f);
    assert_eq!(reported, LOADED);
    let warning = warning.expect("broken and gone plugins are said");
    assert!(warning.contains("broken@local"), "{warning}");
    assert!(warning.contains("gone@local"), "{warning}");
}

#[test]
fn a_user_server_keeps_a_name_a_plugin_also_uses() {
    let f = fixture();
    std::fs::write(
        f.config.accounts[0].config_dir.join(".claude.json"),
        r#"{"mcpServers":{"plugin:posthog:posthog":{"command":"/bin/mine"}}}"#,
    )
    .expect(".claude.json");
    let (reported, warning, config) = run(&f);
    assert_eq!(reported, LOADED);
    assert_eq!(
        config["mcpServers"]["plugin:posthog:posthog"]["command"],
        "/bin/mine"
    );
    let warning = warning.expect("the clash is said");
    assert!(warning.contains("plugin:posthog:posthog"), "{warning}");
}
