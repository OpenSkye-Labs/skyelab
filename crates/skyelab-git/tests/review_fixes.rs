//! Review fixes for PR status and fetch: a placeholder `mergedAt` and UNC remotes.

#[test]
fn zero_merged_at_on_an_open_pr_is_still_open() {
    let s = skyelab_git::parse_pr_view(
        br#"{"state":"OPEN","isDraft":false,"mergedAt":"0001-01-01T00:00:00Z"}"#,
    )
    .expect("parses");
    assert_eq!(s.state, skyelab_git::PrState::Open, "got {:?}", s.state);
}

#[cfg(unix)] // the fake gh is a shell script
#[test]
fn fetch_refuses_unc_remotes() {
    use std::path::Path;
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("mkdir");
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .expect("git runs");
        assert!(ok.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "branch.main.remote", "origin"]);
    git(&["config", "branch.main.merge", "refs/heads/main"]);
    git(&["remote", "add", "origin", "//host/share/repo.git"]);
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let gh = skyelab_git::GitHub {
        gh: &bin,
        config_dir: tmp.path(),
    };
    for url in ["//host/share/repo.git", "file://host/share/repo.git"] {
        git(&["remote", "set-url", "origin", url]);
        let err = skyelab_git::fetch(&repo, &gh).expect_err(url);
        assert!(err.to_string().contains("https"), "{url}: {err}");
    }
}
