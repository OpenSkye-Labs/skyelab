//! Helpers for the composer acceptance tests: a bound project, a recording `claude`, the codex fake.

use serde_json::Value;
use skyelab_agent::{Event, Launch, Session};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Fixture {
    pub _tmp: tempfile::TempDir,
    pub dir: PathBuf,
    pub account: Account,
    pub config: AccountsConfig,
}

/// A project bound to one `provider` account.
pub fn bound(provider: Provider) -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let account = Account {
        id: "acct".into(),
        label: "Acct".into(),
        provider,
        config_dir: root.join("home"),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "acct".into())],
    };
    Fixture {
        _tmp: tmp,
        dir,
        account,
        config,
    }
}

/// A stand-in `claude` that writes its args to `args.txt` and every stdin line to `stdin.txt` in its cwd.
pub fn recorder(f: &Fixture) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let bin = f.dir.parent().expect("root").join("recorder.sh");
    std::fs::write(
        &bin,
        // args.txt appears whole (renamed into place), so a reader never sees half the args.
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > args.tmp && mv args.tmp args.txt\nwhile IFS= read -r l; do printf '%s\\n' \"$l\" >> stdin.txt; done\n",
    )
    .expect("write recorder");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    bin
}

/// Waits for `file` in the project to hold at least `lines` lines.
pub fn lines_of(f: &Fixture, file: &str, lines: usize) -> Vec<String> {
    for _ in 0..500 {
        if let Ok(t) = std::fs::read_to_string(f.dir.join(file)) {
            let got: Vec<String> = t.lines().map(str::to_owned).collect();
            if got.len() >= lines {
                return got;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{file} never got {lines} lines");
}

pub fn launch<'a>(
    f: &'a Fixture,
    model: Option<&'a str>,
    effort: Option<&'a str>,
    attachments: &'a [skyelab_agent::Attachment],
) -> Launch<'a> {
    Launch {
        account: &f.account,
        dir: &f.dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
        model,
        effort,
        attachments,
        mode: Default::default(),
    }
}

pub fn codex_fake() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh")
}

/// The codex fake's echo of every request with `method`, until `count` of them arrived.
pub fn codex_requests(s: &Session, method: &str, count: usize) -> Vec<Value> {
    let mut got = Vec::new();
    while got.len() < count {
        match s.events().recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Unknown(v))
                if v["method"] == "fake/request" && v["params"]["method"] == method =>
            {
                got.push(v["params"]["params"].clone())
            }
            Ok(_) => {}
            Err(e) => panic!("waiting for {method}: {e}"),
        }
    }
    got
}
