use super::*;
use crate::{Checks, Review};
use std::path::Path;

fn pr(n: u64, title: &str) -> serde_json::Value {
    serde_json::json!({
        "number": n,
        "title": title,
        "url": format!("https://github.com/acme/demo/pull/{n}"),
        "author": {"login": "ada", "is_bot": false},
        "isDraft": n.is_multiple_of(2),
        "reviewDecision": "CHANGES_REQUESTED",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS", "conclusion": ""}],
        "updatedAt": "2026-11-20T10:00:00Z",
        "headRefName": format!("feature-{n}"),
        "body": "Fixes \u{202E}it\nfor real"
    })
}

#[test]
fn parses_a_pr_list_with_every_column() {
    let json = serde_json::to_vec(&vec![pr(7, "Fix login"), pr(8, "Draft thing")]).unwrap();
    let rows = parse_pr_list(&json).unwrap();
    assert_eq!(rows.len(), 2);
    let r = &rows[0];
    assert_eq!(r.number, 7);
    assert_eq!(r.title, "Fix login");
    assert_eq!(r.url, "https://github.com/acme/demo/pull/7");
    assert_eq!(r.author, "ada");
    assert!(!r.draft);
    assert!(rows[1].draft);
    assert_eq!(r.checks, Checks::Pending);
    assert_eq!(r.review, Review::ChangesRequested);
    assert_eq!(r.updated_at, "2026-11-20T10:00:00Z");
    assert_eq!(r.head_branch.as_deref(), Some("feature-7"));
    assert_eq!(
        r.body, "Fixes it\nfor real",
        "bidi controls dropped, newlines kept"
    );
}

#[test]
fn caps_rows_and_fields() {
    let long = "x".repeat(5000);
    let list: Vec<_> = (1..=60).map(|n| pr(n, &long)).collect();
    let rows = parse_pr_list(&serde_json::to_vec(&list).unwrap()).unwrap();
    assert_eq!(rows.len(), MAX_ROWS);
    assert_eq!(rows[0].title.chars().count(), MAX_TITLE);
}

#[test]
fn drops_rows_with_a_link_that_is_not_github_https() {
    let mut bad = pr(3, "t");
    bad["url"] = "javascript:alert(1)".into();
    let rows = parse_pr_list(&serde_json::to_vec(&vec![bad, pr(4, "ok")]).unwrap()).unwrap();
    assert_eq!(rows.iter().map(|r| r.number).collect::<Vec<_>>(), [4]);
}

#[test]
fn junk_json_is_an_error() {
    assert!(parse_pr_list(b"not json").is_err());
    assert!(parse_pr_list(b"{\"a\":1}").is_err());
    assert!(parse_issue_list(b"[1,2").is_err());
}

#[test]
fn parses_issues_with_labels_assignees_and_a_capped_body() {
    let mut labels: Vec<_> = (0..20)
        .map(|i| serde_json::json!({"name": format!("label-{i}"), "color": "ff0000"}))
        .collect();
    labels[0] = serde_json::json!({"name": "bug", "color": "ff0000"});
    let json = serde_json::json!([{
        "number": 12,
        "title": "Crash on start",
        "url": "https://github.com/acme/demo/issues/12",
        "author": {"login": "bob"},
        "labels": labels,
        "assignees": [{"login": "ada"}],
        "updatedAt": "2026-11-21T08:00:00Z",
        "body": "b".repeat(10_000)
    }]);
    let rows = parse_issue_list(&serde_json::to_vec(&json).unwrap()).unwrap();
    let r = &rows[0];
    assert_eq!(
        (r.number, r.title.as_str(), r.author.as_str()),
        (12, "Crash on start", "bob")
    );
    assert_eq!(r.labels.len(), MAX_LABELS);
    assert_eq!(r.labels[0], "bug");
    assert_eq!(r.assignees, ["ada"]);
    assert_eq!(r.body.chars().count(), MAX_BODY);
}

#[test]
fn repo_from_github_remote_urls_only() {
    let ok = [
        ("https://github.com/acme/demo.git", "acme/demo"),
        ("https://github.com/acme/demo", "acme/demo"),
        ("git@github.com:acme/my.repo.git", "acme/my.repo"),
        ("ssh://git@github.com/acme/demo.git", "acme/demo"),
        ("https://github.com/acme/demo/", "acme/demo"),
        ("https://github.com/acme/.github.git", "acme/.github"),
        ("git@github.com:acme/.dotfiles.git", "acme/.dotfiles"),
    ];
    for (url, slug) in ok {
        assert_eq!(parse_repo_url(url).as_deref(), Some(slug), "{url}");
    }
    for url in [
        "https://gitlab.com/acme/demo.git",
        "https://user:tok@github.com/acme/demo.git",
        "https://github.com/-x/demo",
        "https://github.com/acme/demo/extra",
        "https://github.com/acme",
        "git@github.com:acme/de mo.git",
        "/local/path",
        "https://github.com/acme/..",
        "https://github.com/acme/.",
        "https://github.com/../demo",
    ] {
        assert_eq!(parse_repo_url(url), None, "{url}");
    }
}

#[test]
fn list_args_name_the_repo_and_filter() {
    let a = pr_list_args("acme/demo", PrFilter::Mine);
    assert!(
        a.windows(2)
            .any(|w| w == ["--repo", "github.com/acme/demo"]),
        "{a:?}"
    );
    assert!(a.windows(2).any(|w| w == ["--author", "@me"]), "{a:?}");
    assert!(a.windows(2).any(|w| w == ["--limit", "50"]), "{a:?}");
    let a = pr_list_args("acme/demo", PrFilter::ReviewRequested);
    assert!(
        a.windows(2)
            .any(|w| w == ["--search", "review-requested:@me"]),
        "{a:?}"
    );
    let a = pr_list_args("acme/demo", PrFilter::All);
    assert!(!a.iter().any(|x| x == "@me" || x.contains("@me")), "{a:?}");
    let a = issue_list_args("acme/demo", IssueFilter::Mine);
    assert!(a.windows(2).any(|w| w == ["--author", "@me"]), "{a:?}");
    let a = pr_list_args("acme/demo", PrFilter::All);
    assert!(
        a.iter()
            .any(|x| x.contains("headRefName") && x.contains("body")),
        "{a:?}"
    );
    let a = issue_list_args("acme/demo", IssueFilter::Assigned);
    assert!(a.windows(2).any(|w| w == ["--assignee", "@me"]), "{a:?}");
    assert!(
        !issue_list_args("acme/demo", IssueFilter::All)
            .iter()
            .any(|x| x == "@me")
    );
}

/// Through the fake gh: runs in the account's dir with the repo by flag, and refuses a dir with no login.
#[cfg(unix)] // the fake gh is a shell script
#[test]
fn lists_through_gh_under_the_given_config_dir() {
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let dir = tempfile::tempdir().unwrap();
    let github = crate::GitHub {
        gh: &gh,
        config_dir: dir.path(),
    };
    assert!(list_prs(&github, "acme/demo", PrFilter::All).is_err());
    std::fs::write(dir.path().join("fake-login"), "ada").unwrap();
    let prs = list_prs(&github, "acme/demo", PrFilter::All).unwrap();
    assert!(!prs.is_empty());
    let issues = list_issues(&github, "acme/demo", IssueFilter::All).unwrap();
    assert!(!issues.is_empty());
    let log = std::fs::read_to_string(dir.path().join("fake-gh.list")).unwrap();
    assert!(
        log.contains("pr list") && log.contains("github.com/acme/demo"),
        "{log}"
    );
    assert!(log.contains("issue list"), "{log}");
    let real = dir.path().canonicalize().unwrap();
    assert!(log.contains(&format!("cwd={} ", real.display())), "{log}");
    assert!(!log.contains("token=set"), "{log}");
}

#[test]
fn rejects_a_bad_repo_before_running_gh() {
    let dir = tempfile::tempdir().unwrap();
    let gh = Path::new("/definitely/not/gh");
    let github = crate::GitHub {
        gh,
        config_dir: dir.path(),
    };
    let e = list_prs(&github, "-x/demo", PrFilter::All).unwrap_err();
    assert!(matches!(e, crate::Error::Invalid(_)), "{e:?}");
}

/// A gh that never answers is killed at the limit, so a page's fetch can't hang for good.
#[cfg(unix)] // the fake gh is a shell script
#[test]
fn times_out_a_hung_gh() {
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fake-login"), "ada").unwrap();
    std::fs::write(dir.path().join("fake-hold"), "").unwrap();
    let github = crate::GitHub {
        gh: &gh,
        config_dir: dir.path(),
    };
    let started = std::time::Instant::now();
    let r = list_within(
        &github,
        &pr_list_args("acme/demo", PrFilter::All),
        "gh pr list",
        std::time::Duration::from_secs(1),
    );
    assert!(r.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn strips_control_and_bidi_characters() {
    let json = serde_json::json!([{
        "number": 1,
        "title": "ok\u{202E}gpj.exe\u{0007}\u{200B}\u{2028}\u{FEFF}",
        "url": "https://github.com/acme/demo/issues/1",
        "author": {"login": "a\u{2066}b"},
        "labels": [], "assignees": [],
        "body": "line one\nline\u{0000} two"
    }]);
    let r = &parse_issue_list(&serde_json::to_vec(&json).unwrap()).unwrap()[0];
    assert_eq!(r.title, "okgpj.exe");
    assert_eq!(r.author, "ab");
    assert_eq!(r.body, "line one\nline two");
}
