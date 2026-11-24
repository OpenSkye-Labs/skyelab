#![cfg(unix)]
mod common;

use common::start_in;
use skyelab_agent::Event;
use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

#[test]
fn capped_line_stays_within_cap() {
    let dir = std::env::temp_dir().join(format!("skyelab-utf8cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let dir = std::fs::canonicalize(&dir).expect("canonicalize");
    let bin = dir.join("fake.sh");
    std::fs::write(
        &bin,
        "#!/bin/sh\nhead -c 16777215 /dev/zero | tr '\\0' a\nprintf '\\303\\251bbbb\\n'\ncat >/dev/null\n",
    )
    .expect("write fake");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let cfg = AccountsConfig {
        accounts: vec![Account {
            id: "a".into(),
            label: "A".into(),
            config_dir: dir.join("acct"),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        }],
        bindings: vec![Binding::new(dir.clone(), "a".into())],
    };
    let session = start_in(&bin, &cfg, &dir, "hi").expect("start");
    let next = || {
        session
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("event within 10s")
    };
    assert!(matches!(next(), Event::Mcp { launch: true, .. }));
    match next() {
        Event::Unknown(serde_json::Value::String(s)) => {
            assert!(s.len() <= 16 << 20, "capped len {}", s.len())
        }
        other => panic!("expected truncated line, got {other:?}"),
    }
}
