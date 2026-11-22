//! `Launch.mcp_config` reaches claude's command line, and only when set.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::Path;
use std::time::Duration;

fn args_for(mcp_config: Option<&Path>) -> String {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let account = Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: dir.join("account"),
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "acme".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config,
    };
    let session = start_with_bin(&bin, &config, &launch).expect("start");
    loop {
        match session.events().recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Unknown(v)) if v["type"] == "fake_env" => {
                return v["args"].as_str().expect("args").to_owned();
            }
            Ok(_) => {}
            Err(e) => panic!("no fake_env line: {e}"),
        }
    }
}

#[test]
fn mcp_config_flag_only_when_given() {
    let path = Path::new("/private/run/skyelab/thread-7.json");
    let args = args_for(Some(path));
    assert!(
        args.contains("--mcp-config /private/run/skyelab/thread-7.json"),
        "{args}"
    );
    assert!(args.contains("--allowedTools mcp__skyelab"), "{args}");
    assert!(args.contains("--strict-mcp-config"), "{args}");

    let args = args_for(None);
    assert!(!args.contains("--mcp-config"), "{args}");
    assert!(!args.contains("mcp__skyelab"), "{args}");
    assert!(args.contains("--strict-mcp-config"), "{args}");
}
