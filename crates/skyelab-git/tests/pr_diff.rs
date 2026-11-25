//! Reading a pull request's diff and posting a review with line comments, with the account's gh (the fake gh only):
//! the exact gh args, the account's config dir, the head pinned around the read, and the review JSON.
#![cfg(unix)] // the fake gh is a shell script

use skyelab_git::{GitHub, ReviewComment, Side, Verdict};
use std::path::{Path, PathBuf};

const URL: &str = "https://github.com/acme/demo/pull/7";
const HEAD: &str = "1111111111111111111111111111111111111111";

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

fn logged_in() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    std::fs::write(dir.join("fake-login"), "octo").expect("write");
    (tmp, dir)
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

#[test]
fn reads_the_diff_with_gh_pr_diff_in_the_account_dir_and_pins_the_head() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let d = skyelab_git::pr_diff(&gh, URL).expect("reads");
    assert_eq!(
        read(&dir, "fake-gh.prdiff").trim(),
        format!("pr diff {URL} --color never")
    );
    assert_eq!(d.head.as_deref(), Some(HEAD));
    assert!(!d.files.is_empty(), "the default fake diff lists a file");
    // Two pr view reads: one before the diff, one after.
    assert_eq!(read(&dir, "fake-gh.prview").lines().count(), 2);
}

#[test]
fn a_head_that_moves_during_the_read_is_an_error() {
    let (_tmp, dir) = logged_in();
    std::fs::write(dir.join("fake-head-after-diff"), "2".repeat(40)).expect("write");
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let e = skyelab_git::pr_diff(&gh, URL).expect_err("refused");
    assert!(format!("{e}").contains("changed while"), "{e}");
}

#[test]
fn the_diff_is_parsed_as_text_and_capped() {
    let (_tmp, dir) = logged_in();
    let mut diff = String::from(
        "diff --git a/../../etc/passwd b/../../etc/passwd\n--- a/../../etc/passwd\n+++ b/../../etc/passwd\n@@ -1 +1 @@\n-root\n+\x1b]52;c;evil\x07\n",
    );
    diff.push_str("diff --git a/big.txt b/big.txt\nnew file mode 100644\n--- /dev/null\n+++ b/big.txt\n@@ -0,0 +1,6000 @@\n");
    for i in 0..6000 {
        diff.push_str(&format!("+line {i}\n"));
    }
    std::fs::write(dir.join("fake-pr.diff"), diff).expect("write");
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let d = skyelab_git::pr_diff(&gh, URL).expect("reads");
    assert_eq!(d.files.len(), 2);
    assert_eq!(
        d.files[0].new_path.as_deref(),
        Some(Path::new("../../etc/passwd"))
    );
    assert!(
        matches!(&d.files[0].hunks[0].lines[1], skyelab_git::Line::Added(t) if t.contains("evil"))
    );
    assert_eq!(d.files[1].too_large, Some(6000));
}

#[test]
fn a_review_with_line_comments_posts_one_review_through_gh_api() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let comments = [
        ReviewComment {
            path: "src/a.rs".into(),
            line: 12,
            side: Side::Right,
            start: Some((10, Side::Right)),
            body: "Why three lines?".into(),
        },
        ReviewComment {
            path: "src/b.rs".into(),
            line: 4,
            side: Side::Left,
            start: None,
            body: "Keep this.".into(),
        },
    ];
    skyelab_git::submit_review(
        &gh,
        URL,
        HEAD,
        Verdict::RequestChanges,
        "Two things.",
        &comments,
    )
    .expect("posts");
    assert_eq!(
        read(&dir, "fake-gh.actions").trim(),
        "api --method POST repos/acme/demo/pulls/7/reviews --input -"
    );
    let sent: serde_json::Value =
        serde_json::from_str(&read(&dir, "fake-gh.review-body")).expect("json");
    assert_eq!(
        sent,
        serde_json::json!({
            "commit_id": HEAD,
            "event": "REQUEST_CHANGES",
            "body": "Two things.",
            "comments": [
                {"path": "src/a.rs", "line": 12, "side": "RIGHT", "start_line": 10, "start_side": "RIGHT", "body": "Why three lines?"},
                {"path": "src/b.rs", "line": 4, "side": "LEFT", "body": "Keep this."}
            ]
        })
    );
}

#[test]
fn a_comment_only_review_needs_no_summary_but_needs_a_pinned_head() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let c = [ReviewComment {
        path: "a".into(),
        line: 1,
        side: Side::Right,
        start: Some((1, Side::Right)),
        body: "x".into(),
    }];
    skyelab_git::submit_review(&gh, URL, HEAD, Verdict::Comment, "", &c).expect("posts");
    let sent: serde_json::Value =
        serde_json::from_str(&read(&dir, "fake-gh.review-body")).expect("json");
    // A one-line range is sent as one line (GitHub refuses start_line == line).
    assert!(sent["comments"][0].get("start_line").is_none(), "{sent}");
    assert!(skyelab_git::submit_review(&gh, URL, "nothex", Verdict::Comment, "", &c).is_err());
    assert!(skyelab_git::submit_review(&gh, URL, HEAD, Verdict::Comment, "", &[]).is_err());
    let bad = "https://github.com/acme/demo/pull/7/../../x";
    assert!(skyelab_git::submit_review(&gh, bad, HEAD, Verdict::Comment, "", &c).is_err());
    // Exactly the one good call reached gh.
    assert_eq!(read(&dir, "fake-gh.actions").lines().count(), 1);
}

#[test]
fn a_failed_review_reports_ghs_error() {
    let (_tmp, dir) = logged_in();
    std::fs::write(
        dir.join("fake-action-fail"),
        "Unprocessable Entity: line must be part of the diff",
    )
    .expect("write");
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let c = [ReviewComment {
        path: "a".into(),
        line: 99,
        side: Side::Right,
        start: None,
        body: "x".into(),
    }];
    let e =
        skyelab_git::submit_review(&gh, URL, HEAD, Verdict::Comment, "", &c).expect_err("fails");
    assert!(format!("{e}").contains("part of the diff"), "{e}");
}

#[test]
fn the_fake_refuses_a_review_pinned_to_another_commit() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let c = [ReviewComment {
        path: "a".into(),
        line: 1,
        side: Side::Right,
        start: None,
        body: "x".into(),
    }];
    let other = "2".repeat(40);
    let e =
        skyelab_git::submit_review(&gh, URL, &other, Verdict::Comment, "", &c).expect_err("422");
    assert!(format!("{e}").contains("422"), "{e}");
}
