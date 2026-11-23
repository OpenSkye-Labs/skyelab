//! Review fixes: a thread codex refuses to open never swallows later prompts, and an interrupt sent
//! before the thread opens still stops the first turn.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, Session, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Refuses every thread/resume; starts threads and completes turns like the real one.
const REFUSING: &str = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"id":%s,"result":{}}\n' "$id" ;;
    *'"method":"thread/resume"'*) printf '{"id":%s,"error":{"code":-32600,"message":"no rollout found"}}\n' "$id" ;;
    *'"method":"turn/start"'*)
      printf '{"id":%s,"result":{"turn":{"id":"t1"}}}\n' "$id"
      printf '{"method":"turn/completed","params":{"turn":{"id":"t1","status":"completed"}}}\n' ;;
  esac
done
"#;

struct Fixture {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    account: Account,
    config: AccountsConfig,
}

fn bound() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let account = Account {
        id: "oai".into(),
        label: "OpenAI".into(),
        provider: Provider::Codex,
        config_dir: root.join("codex-home"),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "oai".into())],
    };
    Fixture {
        _tmp: tmp,
        dir,
        account,
        config,
    }
}

fn start(f: &Fixture, bin: &Path, resume: Option<&str>, prompt: &str) -> Session {
    let launch = Launch {
        account: &f.account,
        dir: &f.dir,
        resume,
        prompt,
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    start_with_bin(bin, &f.config, &launch).expect("start")
}

fn next_result(rx: &flume::Receiver<Event>) -> Option<Event> {
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(e @ Event::Result { .. }) => return Some(e),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

#[test]
fn a_prompt_after_a_failed_resume_is_refused_not_swallowed() {
    let f = bound();
    let bin = f.dir.join("refusing-codex.sh");
    std::fs::write(&bin, REFUSING).expect("write fake");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let mut session = start(&f, &bin, Some("gone-thread"), "hi");
    let first = next_result(session.events()).expect("the failed resume reports a Result");
    assert!(
        matches!(&first, Event::Result { is_error: true, result: Some(r), .. } if r.contains("no rollout found")),
        "{first:?}"
    );
    let err = session
        .send_user_message("again")
        .expect_err("a follow-up on a thread that never opened must fail, not queue forever");
    assert!(err.to_string().contains("no rollout found"), "{err:#}");
}

#[test]
fn an_interrupt_before_the_thread_opens_stops_the_first_turn() {
    let f = bound();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh");
    let mut session = start(&f, &fake, None, "hang here");
    session.interrupt().expect("interrupt"); // queued before the fake can answer thread/start
    let result = next_result(session.events()).expect("the hanging turn ends");
    assert!(
        matches!(result, Event::Result { is_error: true, .. }),
        "{result:?}"
    );
}
