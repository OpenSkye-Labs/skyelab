//! Acceptance: a thread's pull request state, read with `gh pr view` under one account's gh config dir.
//! `gh` is a fake; nothing reaches the network.

use skyelab_git::{Checks, PrState, Review, parse_pr_view};

fn parse(json: &str) -> skyelab_git::PrStatus {
    parse_pr_view(json.as_bytes()).expect("parses")
}

#[test]
fn state_draft_wins_over_open_and_merged_over_closed() {
    let s = parse(
        r#"{"state":"OPEN","isDraft":false,"mergedAt":null,"reviewDecision":"","statusCheckRollup":[]}"#,
    );
    assert_eq!(s.state, PrState::Open);
    assert_eq!(s.checks, Checks::None);
    assert_eq!(s.review, Review::None);
    let s = parse(r#"{"state":"OPEN","isDraft":true,"statusCheckRollup":null}"#);
    assert_eq!(s.state, PrState::Draft);
    let s = parse(r#"{"state":"MERGED","isDraft":false,"mergedAt":"2026-11-01T00:00:00Z"}"#);
    assert_eq!(s.state, PrState::Merged);
    let s = parse(r#"{"state":"CLOSED","isDraft":false,"mergedAt":null}"#);
    assert_eq!(s.state, PrState::Closed);
    assert!(PrState::Merged.is_done() && PrState::Closed.is_done());
    assert!(!PrState::Open.is_done() && !PrState::Draft.is_done());
    for s in [
        PrState::Open,
        PrState::Draft,
        PrState::Merged,
        PrState::Closed,
    ] {
        assert_eq!(PrState::parse(s.as_str()), Some(s));
    }
    assert!(parse_pr_view(br#"{"state":"WEIRD"}"#).is_err());
    assert!(parse_pr_view(b"not json").is_err());
}

#[test]
fn review_decisions() {
    let review = |d: &str| parse(&format!(r#"{{"state":"OPEN","reviewDecision":"{d}"}}"#)).review;
    assert_eq!(review("APPROVED"), Review::Approved);
    assert_eq!(review("CHANGES_REQUESTED"), Review::ChangesRequested);
    assert_eq!(review("REVIEW_REQUIRED"), Review::Required);
    assert_eq!(review(""), Review::None);
}

#[test]
fn checks_failing_beats_pending_beats_passing_and_lists_failures() {
    let s = parse(
        r#"{"state":"OPEN","statusCheckRollup":[
        {"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://github.com/a/b/runs/1"},
        {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/a/b/runs/2"},
        {"__typename":"CheckRun","name":"slow","status":"IN_PROGRESS","conclusion":"","detailsUrl":""},
        {"__typename":"StatusContext","context":"ci/legacy","state":"ERROR","targetUrl":"javascript:alert(1)"}
        ]}"#,
    );
    assert_eq!(s.checks, Checks::Failing);
    let names: Vec<_> = s.failed.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["test", "ci/legacy"]);
    assert_eq!(
        s.failed[0].url.as_deref(),
        Some("https://github.com/a/b/runs/2")
    );
    assert_eq!(s.failed[1].url, None, "only https links are kept");

    let s = parse(
        r#"{"state":"OPEN","statusCheckRollup":[
        {"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"SUCCESS"},
        {"__typename":"StatusContext","context":"deploy","state":"PENDING"}]}"#,
    );
    assert_eq!(s.checks, Checks::Pending);
    assert!(s.failed.is_empty());

    let s = parse(
        r#"{"state":"OPEN","statusCheckRollup":[
        {"__typename":"CheckRun","name":"lint","status":"COMPLETED","conclusion":"SKIPPED"},
        {"__typename":"StatusContext","context":"x","state":"SUCCESS"}]}"#,
    );
    assert_eq!(s.checks, Checks::Passing);
}

#[test]
fn failing_list_is_bounded() {
    let many: Vec<String> = (0..50)
        .map(|i| {
            format!(
                r#"{{"__typename":"CheckRun","name":"{}{i}","status":"COMPLETED","conclusion":"FAILURE"}}"#,
                "n".repeat(500)
            )
        })
        .collect();
    let s = parse(&format!(
        r#"{{"state":"OPEN","statusCheckRollup":[{}]}}"#,
        many.join(",")
    ));
    assert_eq!(s.checks, Checks::Failing);
    assert_eq!(s.failed.len(), skyelab_git::MAX_FAILED_CHECKS);
    assert!(s.failed.iter().all(|f| f.name.chars().count() <= 200));
}

#[test]
fn logged_in_reads_hosts_without_running_gh() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dir = tmp.path();
    assert!(!skyelab_git::gh_logged_in(&dir.join("missing")));
    assert!(!skyelab_git::gh_logged_in(dir), "no hosts.yml");
    std::fs::write(dir.join("hosts.yml"), "ghe.example.com:\n    user: x\n").expect("write");
    assert!(!skyelab_git::gh_logged_in(dir), "another host only");
    std::fs::write(
        dir.join("hosts.yml"),
        "github.com:\n    users:\n        octo:\n    user: octo\n",
    )
    .expect("write");
    assert!(skyelab_git::gh_logged_in(dir));
}

#[cfg(unix)] // the fake gh is a shell script
mod fake {
    use skyelab_git::{Checks, GitHub, PrState};
    use std::path::{Path, PathBuf};

    fn fake_gh() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
    }

    const URL: &str = "https://github.com/acme/demo/pull/7";

    #[test]
    fn fake_pr_view_runs_under_the_account_dir_with_the_fields_asked() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
        std::fs::write(dir.join("fake-login"), "octo").expect("write");
        let bin = fake_gh();
        let gh = GitHub {
            gh: &bin,
            config_dir: &dir,
        };
        let s = skyelab_git::pr_status(&gh, URL).expect("reads");
        assert_eq!((s.state, s.checks), (PrState::Open, Checks::Passing));
        let calls = std::fs::read_to_string(dir.join("fake-gh.prview")).expect("recorded");
        assert_eq!(
            calls.trim(),
            format!("pr view {URL} --json state,isDraft,mergedAt,reviewDecision,statusCheckRollup")
        );
        let env = std::fs::read_to_string(dir.join("fake-gh.fullenv")).expect("env");
        assert!(env.contains(&format!("GH_CONFIG_DIR={}", dir.display())));
        assert!(env.contains("GH_PROMPT_DISABLED=1"));

        std::fs::write(
            dir.join("fake-pr.json"),
            r#"{"state":"MERGED","isDraft":false,"mergedAt":"2026-11-02T00:00:00Z"}"#,
        )
        .expect("write");
        assert_eq!(
            skyelab_git::pr_status(&gh, URL).expect("reads").state,
            PrState::Merged
        );
    }

    #[test]
    fn a_link_that_is_not_a_github_pr_never_reaches_gh() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join("fake-login"), "octo").expect("write");
        let bin = fake_gh();
        let gh = GitHub {
            gh: &bin,
            config_dir: tmp.path(),
        };
        for bad in [
            "--repo=x",
            "https://evil.example/acme/demo/pull/7",
            "acme/demo#7",
        ] {
            assert!(skyelab_git::pr_status(&gh, bad).is_err(), "{bad}");
        }
        assert!(!tmp.path().join("fake-gh.prview").exists());
    }

    #[test]
    fn signed_out_fake_fails_the_read() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let bin = fake_gh();
        let gh = GitHub {
            gh: &bin,
            config_dir: tmp.path(),
        };
        assert!(skyelab_git::pr_status(&gh, URL).is_err());
    }
}
