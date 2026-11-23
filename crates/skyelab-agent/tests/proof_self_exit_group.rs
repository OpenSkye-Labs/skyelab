#![cfg(unix)]
mod common;

use common::start_in;
use skyelab_agent::Event;
use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

#[test]
fn drop_after_self_exit_kills_group() {
    let dir = std::env::temp_dir().join(format!("skyelab-selfexit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let dir = std::fs::canonicalize(&dir).expect("canonicalize");
    let bin = dir.join("fake.sh");
    std::fs::write(
        &bin,
        "#!/bin/sh\nsleep 30 &\necho $! > grandchild.pid\nread -r line\necho '{\"type\":\"bye\"}'\nexit 0\n",
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
    loop {
        if let Event::Unknown(v) = session
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("event within 10s")
            && v["type"] == "bye"
        {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(300)); // the leader exits, unreaped
    drop(session);
    std::thread::sleep(Duration::from_millis(300));
    let pid = std::fs::read_to_string(dir.join("grandchild.pid")).expect("pid file");
    let alive = std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .status()
        .expect("kill -0")
        .success();
    if alive {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", pid.trim()])
            .status();
    }
    assert!(!alive, "grandchild survived drop of a self-exited session");
}
