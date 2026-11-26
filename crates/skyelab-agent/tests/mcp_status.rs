//! Acceptance: what each agent says about its MCP servers (Claude's init `mcp_servers`, Codex's startup status
//! notifications) arrives as `Event::Mcp` reports. Nothing runs the real `claude` or `codex`.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, McpServer, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::Duration;

fn server(name: &str, status: &str, error: Option<&str>) -> McpServer {
    McpServer {
        name: name.into(),
        status: status.into(),
        error: error.map(Into::into),
    }
}

/// The reports after the launch one a session of `bin` sends before its stream ends or `stop` shows up.
fn reports(provider: Provider, bin: &Path, setup: impl FnOnce(&Path)) -> Vec<Vec<McpServer>> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let account = Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: root.join("home"),
        provider,
        gh_config_dir: None,
        ssh_agent: false,
    };
    std::fs::create_dir(&account.config_dir).expect("home");
    setup(&account.config_dir);
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "acme".into())],
    };
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
        untrusted: false,
    };
    let session = start_with_bin(bin, &config, &launch).expect("start");
    // Without a grant the launch report is an empty list: it still clears an earlier process's.
    match session.events().recv_timeout(Duration::from_secs(60)) {
        Ok(Event::Mcp {
            servers,
            launch: true,
            warning: None,
        }) => assert!(servers.is_empty(), "{servers:?}"),
        other => panic!("no launch report first: {other:?}"),
    }
    let mut out = Vec::new();
    loop {
        match session.events().recv_timeout(Duration::from_secs(60)) {
            Ok(Event::Mcp { servers, .. }) => out.push(servers),
            Ok(Event::Init { .. }) if provider == Provider::Codex => return out,
            Ok(_) => {}
            Err(_) => return out, // the stream ended
        }
    }
}

#[test]
fn claude_init_and_codex_status_become_reports() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let claude = tmp.path().join("claude");
    std::fs::write(
        &claude,
        "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s1\",\"model\":\"m\",\"mcp_servers\":[{\"name\":\"skyelab\",\"status\":\"connected\"},{\"name\":\"docs\",\"status\":\"failed\"},{\"name\":\"linear\",\"status\":\"needs-auth\"}]}'\n",
    )
    .expect("write fake");
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert_eq!(
        reports(Provider::Claude, &claude, |_| {}),
        [vec![
            server("skyelab", "connected", None),
            server("docs", "failed", None),
            server("linear", "needs-auth", None),
        ]]
    );

    let codex = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh");
    let got = reports(Provider::Codex, &codex, |home| {
        std::fs::write(
            home.join("fake-mcp-status"),
            "docs starting\ndocs failed no such command\n",
        )
        .expect("status file");
    });
    assert_eq!(
        got,
        [
            vec![server("docs", "starting", None)],
            vec![server("docs", "failed", Some("no such command"))],
        ]
    );
    assert_eq!(got[1][0].state(), "Failed");
}
