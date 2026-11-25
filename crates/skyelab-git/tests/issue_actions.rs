//! Comment on, close and reopen an issue with the account's gh (the fake gh only): the exact gh args, comments on
//! stdin, gh's errors, the row read again, and links that never reach gh.
#![cfg(unix)] // the fake gh is a shell script

use skyelab_git::{CloseReason, GitHub};
use std::path::{Path, PathBuf};

const URL: &str = "https://github.com/acme/demo/issues/31";

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

fn logged_in() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    std::fs::write(dir.join("fake-login"), "octo").expect("write");
    (tmp, dir)
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_default()
}

#[test]
fn comment_close_and_reopen_pass_gh_their_flags_with_comments_on_stdin() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    skyelab_git::comment_issue(&gh, URL, "Seen it.\n--reason x").expect("comments");
    assert_eq!(read(&dir, "fake-gh.review-body"), "Seen it.\n--reason x");
    skyelab_git::close_issue(&gh, URL, CloseReason::NotPlanned, "").expect("closes");
    assert!(skyelab_git::issue_view(&gh, URL).expect("reads").closed);
    skyelab_git::reopen_issue(&gh, URL, "Back.").expect("reopens");
    assert_eq!(read(&dir, "fake-gh.review-body"), "Back.");
    skyelab_git::close_issue(&gh, URL, CloseReason::Completed, "Fixed in #12").expect("closes");
    assert_eq!(read(&dir, "fake-gh.review-body"), "Fixed in #12");
    assert_eq!(
        read(&dir, "fake-gh.actions").lines().collect::<Vec<_>>(),
        [
            format!("issue comment {URL} --body-file -"),
            format!("issue close {URL} --reason not planned"),
            format!("issue comment {URL} --body-file -"),
            format!("issue reopen {URL}"),
            format!("issue comment {URL} --body-file -"),
            format!("issue close {URL} --reason completed"),
        ]
    );
    let row = skyelab_git::issue_view(&gh, URL).expect("reads");
    assert_eq!((row.number, row.url.as_str(), row.closed), (31, URL, true));
    assert_eq!(
        read(&dir, "fake-gh.issueview").lines().last(),
        Some(
            format!(
                "issue view {URL} --json number,title,url,author,labels,assignees,updatedAt,body,state"
            )
            .as_str()
        )
    );
}

#[test]
fn gh_errors_carry_its_stderr_and_say_when_the_comment_was_already_posted() {
    let (_tmp, dir) = logged_in();
    std::fs::write(dir.join("fake-action-fail"), "HTTP 403: no triage access").expect("write");
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    let e = skyelab_git::close_issue(&gh, URL, CloseReason::Completed, "").expect_err("refused");
    assert!(e.to_string().contains("no triage access"), "{e}");
    let e = skyelab_git::comment_issue(&gh, URL, "hi").expect_err("refused");
    assert!(e.to_string().contains("no triage access"), "{e}");
    // The comment went through, then close failed: the error says so, so a retry isn't a blind double post.
    std::fs::remove_file(dir.join("fake-action-fail")).expect("rm");
    let script = dir.join("gh-close-fails.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$2\" = close ] && {{ echo 'closing refused' >&2; exit 1; }}\nexec '{}' \"$@\"\n",
            bin.display()
        ),
    )
    .expect("write");
    std::process::Command::new("chmod")
        .args(["+x", &script.display().to_string()])
        .status()
        .expect("chmod");
    let gh = GitHub {
        gh: &script,
        config_dir: &dir,
    };
    let e = skyelab_git::close_issue(&gh, URL, CloseReason::Completed, "bye").expect_err("refused");
    let e = e.to_string();
    assert!(
        e.contains("comment was posted") && e.contains("closing refused"),
        "{e}"
    );
}

#[test]
fn links_and_comments_gh_would_misread_never_reach_it() {
    let (_tmp, dir) = logged_in();
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    for bad in [
        "https://github.com/acme/demo/pull/31",
        "https://github.com/acme/demo/issues/031",
        "https://github.com/acme/demo/issues/31/",
        "https://github.com/acme/demo/issues/31#c",
        "https://github.com/-acme/demo/issues/31",
        "https://evil.com/acme/demo/issues/31",
        "--repo=x/y",
        "31",
    ] {
        assert!(skyelab_git::issue_link(bad).is_err(), "{bad}");
        assert!(skyelab_git::comment_issue(&gh, bad, "hi").is_err());
        assert!(skyelab_git::close_issue(&gh, bad, CloseReason::Completed, "").is_err());
        assert!(skyelab_git::reopen_issue(&gh, bad, "").is_err());
        assert!(skyelab_git::issue_view(&gh, bad).is_err());
    }
    for body in ["", "  \n", "nul\0", &"x".repeat(64 * 1024 + 1)] {
        assert!(skyelab_git::comment_issue(&gh, URL, body).is_err());
    }
    assert!(skyelab_git::close_issue(&gh, URL, CloseReason::Completed, "nul\0").is_err());
    assert_eq!(read(&dir, "fake-gh.actions"), "");
}

#[test]
fn a_view_about_another_issue_is_refused() {
    let (_tmp, dir) = logged_in();
    std::fs::write(
        dir.join("fake-issue.json"),
        r#"{"number":32,"url":"https://github.com/acme/demo/issues/32","title":"x","state":"OPEN"}"#,
    )
    .expect("write");
    let bin = fake_gh();
    let gh = GitHub {
        gh: &bin,
        config_dir: &dir,
    };
    assert!(skyelab_git::issue_view(&gh, URL).is_err());
}
