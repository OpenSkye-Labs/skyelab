//! An agent's git must not fall back to the user's own askpass for https credentials.
#![cfg(unix)]
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::process::{Command, Stdio};

#[test]
fn user_askpass_never_answers_for_the_account() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).expect("mkdir");
    let askpass = tmp.path().join("personal-askpass.sh");
    std::fs::write(&askpass, "#!/bin/sh\necho personal\n").expect("askpass");
    std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    std::fs::write(
        home.join(".gitconfig"),
        format!("[core]\n\taskPass = {}\n", askpass.display()),
    )
    .expect("gitconfig");
    let git = skyelab_core::env::git_credential_env(false, &[], false);
    let mut child = Command::new("git")
        .args(["credential", "fill"])
        .current_dir(tmp.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .envs(git)
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
        .write_all(b"protocol=https\nhost=github.com\npath=org/repo\n\n")
        .expect("stdin");
    let out = child.wait_with_output().expect("git exits");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("password=personal"),
        "ASKPASS_LEAK: the user's askpass supplied credentials: {stdout}"
    );
}
