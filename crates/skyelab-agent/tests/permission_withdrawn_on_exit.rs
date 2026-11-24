#![cfg(unix)]
//! An ask still open when the agent goes away (Stop, crash, quit) is withdrawn before the event stream ends, or the
//! stored request stays "Waiting for approval" and is offered to the next session.

use skyelab_agent::{Decision, Event, Launch, Mode, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::Path;
use std::time::Duration;

#[test]
fn killed_agent_withdraws_its_open_ask() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical tempdir");
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
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "permit please",
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Mode::Default,
    };
    let mut s = start_with_bin(&bin, &cfg, &launch).expect("start fake");
    let asked = loop {
        let e = s
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("the ask within 60 s");
        if let Event::Permission { id, .. } = e {
            break id;
        }
    };
    s.kill().expect("kill");
    let mut rest = Vec::new();
    while let Ok(e) = s.events().recv_timeout(Duration::from_secs(60)) {
        rest.push(e);
    }
    assert!(
        rest.iter().any(|e| matches!(e, Event::PermissionResolved { id, decision: Decision::Withdrawn, .. } if *id == asked)),
        "open ask never withdrawn after the agent died; events after kill: {rest:?}"
    );
}
