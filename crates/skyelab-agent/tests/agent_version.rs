//! `agent_version` runs `--version` with a scrubbed env and reports the first line.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

fn script(dir: &Path, body: &str) -> std::path::PathBuf {
    let p = dir.join("agent");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("write");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    p
}

#[test]
fn agent_version_reads_the_first_line_without_account_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Prints its config dir var too: it must be empty, never inherited from SkyeLab's own env.
    let bin = script(
        dir.path(),
        r#"[ "$1" = --version ] || exit 9; printf '\n  2.1.0 (Claude Code) %s|\nmore\n' "$CLAUDE_CONFIG_DIR$CODEX_HOME""#,
    );
    let got = skyelab_agent::agent_version(&bin).expect("version");
    assert_eq!(got, "2.1.0 (Claude Code) |");
}

#[test]
fn agent_version_fails_for_a_missing_or_failing_bin() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(skyelab_agent::agent_version(Path::new("skyelab-no-such-agent")).is_err());
    let bin = script(dir.path(), "exit 3");
    assert!(skyelab_agent::agent_version(&bin).is_err());
}
