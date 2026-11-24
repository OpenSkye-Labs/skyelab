//! Acceptance (#13): with hooks off, a hook planted in `.git/hooks` never runs, on every platform (Windows too:
//! Git for Windows runs `#!/bin/sh` hooks through its own sh). The control commit with hooks on proves it would.

use std::fs;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_planted_hook_runs_only_with_hooks_on() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    let hook = repo.join(".git/hooks/pre-commit");
    // Relative to the work tree, where git runs hooks: no path quoting between sh and Windows.
    fs::write(&hook, "#!/bin/sh\necho ran >> hook-ran\nexit 0\n").expect("plant hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let marker = repo.join("hook-ran");

    fs::write(repo.join("a.txt"), "one\n").expect("write");
    skyelab_git::commit(&repo, "with hooks", &["a.txt".into()], &[], true, None)
        .expect("commit with hooks");
    assert!(marker.exists(), "the control hook didn't run");
    fs::remove_file(&marker).expect("clear marker");

    fs::write(repo.join("b.txt"), "two\n").expect("write");
    skyelab_git::commit(&repo, "hooks off", &["b.txt".into()], &[], false, None)
        .expect("commit with hooks off");
    assert!(!marker.exists(), "the planted hook ran with hooks off");
}
