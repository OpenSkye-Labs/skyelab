//! A thread whose worktree was discarded keeps its PR link; the PR is read by URL, so it must still be read.
use super::*;

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(ok.success(), "git {args:?}");
}

#[test]
fn discarded_worktree_thread_pr_is_still_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).expect("mkdir");
    git(&project, &["init", "-q"]);
    git(&project, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let gh = root.join("gh-acme");
    std::fs::create_dir_all(&gh).expect("mkdir");
    std::fs::write(gh.join("fake-login"), "octo").expect("login");
    std::fs::write(gh.join("hosts.yml"), "github.com:\n    user: octo\n").expect("hosts");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "acme".into(),
            config_dir: root.join(".claude-acme"),
            provider: Default::default(),
            gh_config_dir: Some(gh.clone()),
            ssh_agent: false,
        })
        .expect("account");
    config
        .add_binding(Binding::new(project.clone(), "acme".into()))
        .expect("bind");
    // The thread's cwd is its worktree; Discard removes it (the branch and the PR link stay).
    let wt = project.join("wt-fix");
    git(
        &project,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "fix",
            wt.to_str().expect("utf-8"),
        ],
    );
    git(
        &project,
        &["worktree", "remove", wt.to_str().expect("utf-8")],
    );
    assert!(!wt.exists());
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    let view = read_pr(&config, &wt, "acme", URL, &bin);
    assert!(
        matches!(&view, PrView::Status(s) if s.state == PrState::Open),
        "PR never read after discard: {view:?}"
    );
}
