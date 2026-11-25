//! The usage poll keeps tokens out of the debug log: a CLI whose answer carries one leaves only its program and
//! args there, and the error SkyeLab shows is its own words. Its own test binary, since debug mode is global.
#![cfg(unix)]

use skyelab_core::{Account, Provider, debug};
use std::os::unix::fs::PermissionsExt as _;

const TOKEN: &str = "sk-ant-oat01-NOT-A-REAL-TOKEN";

#[test]
fn a_token_in_the_cli_answer_reaches_neither_the_log_nor_the_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cli = dir.path().join("claude");
    std::fs::write(
        &cli,
        format!(
            "#!/bin/sh\nread line\nprintf '{{\"type\":\"control_response\",\"response\":{{\"subtype\":\"error\",\"request_id\":\"skyelab-usage\",\"error\":\"bad token {TOKEN}\"}}}}\\n'\n"
        ),
    )
    .expect("script");
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let account = Account {
        id: "work".into(),
        label: "Work".into(),
        provider: Provider::Claude,
        config_dir: dir.path().join("cfg"),
        gh_config_dir: None,
        ssh_agent: false,
    };

    debug::set_enabled(true);
    let err = skyelab_agent::usage_limits(&cli, &account).expect_err("refused");
    debug::set_enabled(false);

    assert!(!format!("{err:#}").contains(TOKEN), "{err:#}");
    let log = serde_json::to_string(&debug::recent(100)).expect("log json");
    assert!(log.contains("claude -p"), "the spawn is logged: {log}");
    assert!(!log.contains(TOKEN), "{log}");
    assert!(
        !log.contains("cfg"),
        "nor the account's config dir (env): {log}"
    );
}
