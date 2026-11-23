//! Acceptance: the import scan never follows a symlink out of the account dir. A linked `projects/` (Claude) or
//! `sessions/` (Codex) could be another account's, so its sessions are not offered; the same files in a real dir are.
#![cfg(unix)] // symlinks

use skyelab_agent::import;
use skyelab_core::Provider;
use std::os::unix::fs::symlink;
use std::path::Path;

fn fixture(name: &str, cwd: &Path) -> String {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("fixture is in the repo");
    text.replace("__CWD__", cwd.to_str().expect("temp paths are UTF-8"))
}

/// `home/<top>` linked to another account's dir holding one session, then the same dir moved in for real.
fn linked_then_real(provider: Provider, top: &str, rel: &str, name: &str, fixture_name: &str) {
    let home = tempfile::tempdir().expect("temp dir");
    let theirs = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let folder = folder.path().canonicalize().expect("exists");
    let rel = rel.replace("{dir}", &import::claude_dir_name(&folder));
    let dir = theirs.path().join(top).join(&rel);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join(name), fixture(fixture_name, &folder)).expect("write");
    symlink(theirs.path().join(top), home.path().join(top)).expect("link");
    assert!(
        import::list(provider, home.path(), &folder).is_empty(),
        "{provider:?}: a linked {top}/ is not read"
    );
    std::fs::remove_file(home.path().join(top)).expect("unlink");
    std::fs::rename(theirs.path().join(top), home.path().join(top)).expect("move in");
    assert_eq!(
        import::list(provider, home.path(), &folder).len(),
        1,
        "{provider:?}: the same dir, really in the account dir, is read"
    );
}

#[test]
fn a_linked_projects_dir_is_not_followed() {
    linked_then_real(
        Provider::Claude,
        "projects",
        "{dir}",
        "5f1c2a3b-0000-4000-8000-00000000c1a0.jsonl",
        "claude-transcript.jsonl",
    );
}

#[test]
fn a_linked_sessions_dir_is_not_followed() {
    linked_then_real(
        Provider::Codex,
        "sessions",
        "2026/11/21",
        "rollout-2026-11-21T08-00-00-019a0c0d-0000-7000-8000-00000000c0de.jsonl",
        "codex-rollout.jsonl",
    );
}
