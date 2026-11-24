use super::*;
use std::path::PathBuf;

const DAY: i64 = 24 * 60 * 60 * 1000;
const NOW: i64 = 1_000 * DAY;

fn c(path: &str, threads: usize, ago_days: i64, git: bool, repo: Option<&str>) -> Candidate {
    Candidate {
        path: PathBuf::from(path),
        account: "a".into(),
        threads,
        last_active_ms: NOW - ago_days * DAY,
        git,
        repo: repo.map(str::to_owned),
    }
}

#[test]
fn recent_is_a_repo_with_three_threads_this_month() {
    assert!(c("/r", 3, 2, true, None).recent(NOW));
    assert!(!c("/r", 2, 2, true, None).recent(NOW), "too few threads");
    assert!(!c("/r", 5, 31, true, None).recent(NOW), "too old");
    assert!(!c("/r", 5, 2, false, None).recent(NOW), "not a repo");
    assert!(!c("/r", 5, -2, true, None).recent(NOW), "in the future");
}

#[test]
fn groups_by_repository_newest_first_and_plain_folders_last() {
    let all = [
        c("/a", 1, 9, true, Some("acme/api")),
        c("/b", 1, 1, true, Some("acme/web")),
        c("/a-wt", 1, 3, true, Some("acme/api")),
        c("/notes", 1, 0, false, None),
        c("/solo", 1, 5, true, None),
    ];
    let (groups, other) = group(&all);
    let labels: Vec<_> = groups.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(labels, ["acme/web", "acme/api", "solo"]);
    assert_eq!(groups[1].rows, [0, 2]);
    assert_eq!(other, [3]);
}

#[test]
fn merge_defaults_to_the_account_with_the_most_threads() {
    let merged = merge(vec![
        (
            "work".into(),
            vec![(PathBuf::from("/p"), 2, NOW), (PathBuf::from("/q"), 1, NOW)],
        ),
        ("home".into(), vec![(PathBuf::from("/p"), 5, NOW - DAY)]),
    ]);
    let p = merged
        .iter()
        .find(|m| m.path == std::path::Path::new("/p"))
        .expect("p");
    assert_eq!((p.account.as_str(), p.threads), ("home", 7));
    assert_eq!(p.last_active_ms, NOW, "the newest of both");
    let q = merged
        .iter()
        .find(|m| m.path == std::path::Path::new("/q"))
        .expect("q");
    assert_eq!(q.account, "work");
}

#[test]
fn repo_names_come_from_origin_urls() {
    for url in [
        "https://github.com/Acme/Api.git",
        "git@github.com:acme/api.git",
        "ssh://git@github.com/acme/api",
    ] {
        assert_eq!(repo_name(url).as_deref(), Some("acme/api"), "{url}");
    }
    assert_eq!(repo_name("/local/bare.git"), None);
}

#[test]
fn origin_is_read_from_the_repo_config() {
    let root = tempfile::tempdir().expect("tempdir");
    let repo = root.path().join("r");
    std::fs::create_dir_all(repo.join(".git")).expect("mkdir");
    std::fs::write(
        repo.join(".git/config"),
        "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://x/y/z\n[remote \"origin\"]\n\turl = git@github.com:acme/api.git\n",
    )
    .expect("config");
    assert_eq!(
        origin_url(&repo).as_deref(),
        Some("git@github.com:acme/api.git")
    );
    assert_eq!(origin_url(root.path()), None);
}
