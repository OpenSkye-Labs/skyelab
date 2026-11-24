//! Acceptance: git run by an agent asks only the bound account's `gh` for https credentials, never a helper or
//! header from the user's git config, and never uses ssh-agent unless the account opts in.
//! The parent env is hostile (a token, an ssh-agent socket, a HOME whose .gitconfig names a personal helper and a
//! URL-scoped auth header), so the agent is started from a child test process that has it.
#![cfg(unix)]
mod common;

use common::start_in;
use skyelab_agent::Event;
use skyelab_core::{Account, AccountsConfig, Binding};
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const CHILD_MARKER: &str = "SKYELAB_AGENT_GIT_CHILD";
const BASE: &str = "SKYELAB_AGENT_GIT_BASE";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Starts the env-dumping fake agent in `<base>/proj-<name>` and returns the env it got.
fn agent_env(base: &Path, name: &str, ssh_agent: bool) -> HashMap<String, String> {
    let proj = base.join(format!("proj-{name}"));
    std::fs::create_dir_all(&proj).expect("mkdir");
    let cfg = AccountsConfig {
        accounts: vec![Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: base.join("claude-acme"),
            provider: Default::default(),
            gh_config_dir: Some(base.join("gh-acme")),
            ssh_agent,
        }],
        bindings: vec![Binding::new(proj.clone(), "acme".into())],
    };
    let session = start_in(&fixture("fake-claude-env.sh"), &cfg, &proj, "hi").expect("start");
    loop {
        let event = session
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("event within 60 s");
        if let Event::Unknown(v) = event
            && v["type"] == "fake_env_written"
        {
            break;
        }
    }
    std::fs::read_to_string(proj.join("agent.env"))
        .expect("agent.env")
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// git with exactly the agent's env, as the agent's Bash tool would run it.
fn git_as_agent(env: &HashMap<String, String>, args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new("git")
        .args(args)
        .env_clear()
        .envs(env)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git runs");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("git exits")
}

#[test]
fn git_credentials_in_child() {
    let Some(base) = std::env::var_os(BASE) else {
        return;
    };
    let base = PathBuf::from(base);

    let env = agent_env(&base, "plain", false);
    assert!(
        !env.contains_key("SSH_AUTH_SOCK"),
        "ssh-agent leaked: {env:?}"
    );
    assert!(!env.contains_key("GH_TOKEN"), "{env:?}");

    let fill = git_as_agent(
        &env,
        &["credential", "fill"],
        "protocol=https\nhost=github.com\npath=org/repo\n\n",
    );
    let out = String::from_utf8_lossy(&fill.stdout);
    assert!(
        out.contains("password=fake-token"),
        "the bound gh must answer: {out} {}",
        String::from_utf8_lossy(&fill.stderr)
    );
    assert!(
        !out.contains("personal"),
        "the user's helper answered: {out}"
    );
    assert!(
        base.join("gh-acme/fake-gh.credential").is_file(),
        "gh ran without the bound GH_CONFIG_DIR"
    );

    let header = git_as_agent(
        &env,
        &[
            "config",
            "--get-urlmatch",
            "http.extraHeader",
            "https://github.com/org/repo",
        ],
        "",
    );
    assert_eq!(
        String::from_utf8_lossy(&header.stdout).trim(),
        "",
        "a URL-scoped auth header from ~/.gitconfig still applies"
    );

    let ssh = git_as_agent(&env, &["ls-remote", "ssh://git@127.0.0.1/x.git"], "");
    assert!(!ssh.status.success());
    let err = String::from_utf8_lossy(&ssh.stderr);
    assert!(
        err.contains("SkyeLab"),
        "ssh must be refused by SkyeLab: {err}"
    );

    let env = agent_env(&base, "ssh", true);
    assert_eq!(
        env.get("SSH_AUTH_SOCK").map(String::as_str),
        Some("/tmp/fake-agent.sock")
    );
    let keys: Vec<&String> = env
        .iter()
        .filter(|(k, _)| k.starts_with("GIT_CONFIG_KEY_"))
        .map(|(_, v)| v)
        .collect();
    assert!(
        !keys
            .iter()
            .any(|k| k.eq_ignore_ascii_case("core.sshCommand")),
        "ssh opt-in still overrides ssh: {keys:?}"
    );
}

#[test]
fn agent_git_uses_only_the_bound_gh() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = std::fs::canonicalize(tmp.path()).expect("canonical");
    let home = base.join("home");
    let bin = base.join("bin");
    std::fs::create_dir_all(&home).expect("mkdir");
    std::fs::create_dir_all(&bin).expect("mkdir");
    std::fs::create_dir_all(base.join("gh-acme")).expect("mkdir");
    std::fs::write(
        home.join(".gitconfig"),
        "[credential]\n\thelper = \"!f() { echo username=me; echo password=personal; }; f\"\n\
         [http \"https://github.com/org/\"]\n\textraHeader = AUTHORIZATION: basic personal\n",
    )
    .expect("gitconfig");
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    std::os::unix::fs::symlink(&gh, bin.join("gh")).expect("link gh");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(std::env::current_exe().expect("test binary path"))
        .args(["git_credentials_in_child", "--exact", "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env(BASE, &base)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("PATH", path)
        .env("GH_TOKEN", "parent-gh-token")
        .env("SSH_AUTH_SOCK", "/tmp/fake-agent.sock")
        .output()
        .expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}
