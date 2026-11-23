#![cfg(unix)]
//! The agent's stderr is kept per session: only the newest `STDERR_TAIL` bytes, in order.

use skyelab_agent::{Launch, STDERR_TAIL, Session, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::Path;
use std::time::{Duration, Instant};

fn start(prompt: &str, dir: &Path) -> Session {
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
        bindings: vec![Binding::new(dir.into(), "acme".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let launch = Launch {
        account: &account,
        dir,
        resume: None,
        prompt,
        mcp_config: None,
        mode: Default::default(),
    };
    start_with_bin(&bin, &cfg, &launch).expect("start fake")
}

fn tempdir() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    (tmp, dir)
}

#[test]
fn stderr_capture_is_capped() {
    let (_tmp, dir) = tempdir();
    let s = start("stderr-flood", &dir);
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut all, mut largest) = (String::new(), 0);
    while !all.ends_with("END") {
        assert!(
            Instant::now() < deadline,
            "no END on stderr: {} bytes",
            all.len()
        );
        std::thread::sleep(Duration::from_millis(50));
        let more = s.take_stderr();
        largest = largest.max(more.len());
        all += &more;
    }
    assert!(largest <= STDERR_TAIL, "{largest} bytes kept at once");
    assert!(
        all.len() < 204_800,
        "the oldest bytes were dropped: {} kept",
        all.len()
    );
    assert!(s.take_stderr().is_empty(), "taken once");
}

#[test]
fn crash_leaves_its_stderr_and_a_nonzero_exit() {
    let (_tmp, dir) = tempdir();
    let mut s = start("crash", &dir);
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(st) = s.try_wait().expect("try_wait") {
            break st;
        }
        assert!(Instant::now() < deadline, "fake did not exit");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(3));
    // The stderr thread may still be draining the pipe.
    let mut text = String::new();
    while !text.contains("boom") && Instant::now() < deadline {
        text += &s.take_stderr();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(text.contains("fake-claude: boom"), "{text:?}");
}
