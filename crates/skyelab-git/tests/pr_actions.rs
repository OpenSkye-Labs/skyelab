//! Merge and review a pull request with the account's gh (the fake gh only): the exact gh args, the account's config
//! dir, the repo's allowed merge methods, and refusals that never reach gh.
#![cfg(unix)] // the fake gh is a shell script

use skyelab_git::{GitHub, MergeMethod, PrState, Verdict};
use std::path::{Path, PathBuf};

const URL: &str = "https://github.com/acme/demo/pull/7";

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

fn logged_in() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    std::fs::write(dir.join("fake-login"), "octo").expect("write");
    (tmp, dir)
}

fn actions(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("fake-gh.actions"))
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn merge_passes_the_method_and_the_delete_flag_in_the_account_dir() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    skyelab_git::merge_pr(&gh, URL, MergeMethod::Squash, true).expect("merges");
    skyelab_git::merge_pr(&gh, URL, MergeMethod::Merge, false).expect("merges");
    skyelab_git::merge_pr(&gh, URL, MergeMethod::Rebase, false).expect("merges");
    assert_eq!(
        actions(&dir),
        [
            format!("pr merge {URL} --squash --delete-branch"),
            format!("pr merge {URL} --merge"),
            format!("pr merge {URL} --rebase"),
        ]
    );
    let env = std::fs::read_to_string(dir.join("fake-gh.fullenv")).expect("env");
    assert!(env.contains(&format!("GH_CONFIG_DIR={}", dir.display())));
    assert!(env.contains("GH_PROMPT_DISABLED=1"));
    // The fake now reads the PR as merged, so a refresh after the action shows it.
    let s = skyelab_git::pr_status(&gh, URL).expect("reads");
    assert_eq!(s.state, PrState::Merged);
}

#[test]
fn a_failed_merge_carries_gh_stderr() {
    let (_tmp, dir) = logged_in();
    std::fs::write(
        dir.join("fake-action-fail"),
        "Pull request acme/demo#7 is not mergeable: the base branch policy prohibits the merge.",
    )
    .expect("write");
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let e = skyelab_git::merge_pr(&gh, URL, MergeMethod::Squash, false).expect_err("refused");
    assert!(e.to_string().contains("base branch policy"), "{e}");
    let e = skyelab_git::review_pr(&gh, URL, Verdict::Approve, "").expect_err("refused");
    assert!(e.to_string().contains("base branch policy"), "{e}");
    // A failed merge leaves the PR open.
    assert_eq!(
        skyelab_git::pr_status(&gh, URL).expect("reads").state,
        PrState::Open
    );
}

#[test]
fn links_that_are_not_github_prs_never_reach_gh() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    for bad in [
        "--repo=x",
        "https://evil.example/acme/demo/pull/7",
        "https://github.com/acme/demo/pull/7?x=1",
    ] {
        assert!(skyelab_git::merge_pr(&gh, bad, MergeMethod::Merge, false).is_err());
        assert!(skyelab_git::review_pr(&gh, bad, Verdict::Approve, "").is_err());
        assert!(skyelab_git::merge_methods(&gh, bad).is_err());
    }
    assert!(!dir.join("fake-gh.actions").exists());
    assert!(!dir.join("fake-gh.repoview").exists());
}

#[test]
fn merge_methods_are_what_the_repo_allows_in_t3_order() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    use MergeMethod::*;
    assert_eq!(
        skyelab_git::merge_methods(&gh, URL).expect("reads"),
        [Squash, Merge, Rebase]
    );
    let calls = std::fs::read_to_string(dir.join("fake-gh.repoview")).expect("logged");
    assert_eq!(
        calls.trim(),
        "repo view github.com/acme/demo --json mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed"
    );
    let repo = |json: &str| {
        std::fs::write(dir.join("fake-repo.json"), json).expect("write");
        skyelab_git::merge_methods(&gh, URL).expect("reads")
    };
    assert_eq!(
        repo(r#"{"mergeCommitAllowed":true,"squashMergeAllowed":false,"rebaseMergeAllowed":true}"#),
        [Merge, Rebase]
    );
    assert_eq!(
        repo(
            r#"{"mergeCommitAllowed":false,"squashMergeAllowed":true,"rebaseMergeAllowed":false}"#
        ),
        [Squash]
    );
}

#[test]
fn review_passes_the_verdict_and_the_body_on_stdin() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let body = |d: &Path| std::fs::read_to_string(d.join("fake-gh.review-body")).expect("body");
    skyelab_git::review_pr(&gh, URL, Verdict::Approve, "").expect("approves");
    assert_eq!(body(&dir), "");
    skyelab_git::review_pr(
        &gh,
        URL,
        Verdict::RequestChanges,
        "Please add a test.\n--merge",
    )
    .expect("requests");
    assert_eq!(body(&dir), "Please add a test.\n--merge");
    skyelab_git::review_pr(&gh, URL, Verdict::Comment, "Looks fine.").expect("comments");
    assert_eq!(body(&dir), "Looks fine.");
    assert_eq!(
        actions(&dir),
        [
            format!("pr review {URL} --approve --body-file -"),
            format!("pr review {URL} --request-changes --body-file -"),
            format!("pr review {URL} --comment --body-file -"),
        ]
    );
}

#[test]
fn review_bodies_gh_would_refuse_never_reach_it() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let long = "x".repeat(64 * 1024 + 1);
    for (verdict, body) in [
        (Verdict::Comment, ""),
        (Verdict::Comment, "  \n "),
        (Verdict::RequestChanges, ""),
        (Verdict::Approve, long.as_str()),
        (Verdict::Comment, "a\0b"),
    ] {
        assert!(
            skyelab_git::review_pr(&gh, URL, verdict, body).is_err(),
            "{verdict:?} {:?}",
            &body[..body.len().min(8)]
        );
    }
    assert!(!dir.join("fake-gh.actions").exists());
}
