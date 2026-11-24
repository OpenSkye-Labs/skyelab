use super::*;
use std::path::{Path, PathBuf};

fn tree() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("tempdir");
    let home = std::fs::canonicalize(root.path()).expect("canonicalize");
    for d in ["code/app/.git", "code/api", "code/.hidden", "Codex", "docs"] {
        std::fs::create_dir_all(home.join(d)).expect("mkdir");
    }
    std::fs::write(home.join("code/app/.git/HEAD"), "ref: refs/heads/main\n").expect("head");
    std::fs::write(home.join("code/notes.txt"), "").expect("file");
    (root, home)
}

#[test]
fn only_paths_browse() {
    let home = Path::new("/h");
    assert_eq!(parse("demo", Some(home)), None);
    assert_eq!(parse("", Some(home)), None);
    let b = parse("~/co", Some(home)).expect("a path");
    assert_eq!(b.dir, Path::new("/h"));
    assert_eq!(b.filter, "co");
    assert_eq!(b.typed_dir, "~/");
    let b = parse("~", Some(home)).expect("home");
    assert_eq!((b.dir.as_path(), b.filter.as_str()), (Path::new("/h"), ""));
    assert_eq!(b.typed_dir, "~/");
    let b = parse("/usr/lo", Some(home)).expect("absolute");
    assert_eq!(
        (b.dir.as_path(), b.filter.as_str()),
        (Path::new("/usr"), "lo")
    );
    assert_eq!(parse("~/x", None), None, "no home: ~ can't expand");
}

#[test]
fn lists_matching_folders_with_git_and_branch() {
    let (_root, home) = tree();
    let b = parse("~/code/", Some(&home)).expect("a path");
    let names: Vec<_> = list(&b, 200).into_iter().map(|f| f.name).collect();
    assert_eq!(
        names,
        ["api", "app"],
        "folders only, hidden left out, sorted"
    );
    let b = parse("~/code/a", Some(&home)).expect("a path");
    let app = list(&b, 200)
        .into_iter()
        .find(|f| f.name == "app")
        .expect("app");
    assert_eq!(app.path, home.join("code/app"));
    assert_eq!(app.git, Some(Some("main".into())));
    let b = parse("~/code/.h", Some(&home)).expect("a path");
    assert_eq!(list(&b, 200).len(), 1, "a dot filter lists hidden folders");
    // Case-insensitive: `co` finds both `code` and `Codex`.
    let b = parse("~/co", Some(&home)).expect("a path");
    assert_eq!(list(&b, 200).len(), 2);
    assert_eq!(list(&b, 1).len(), 1, "capped");
}

#[test]
fn completes_a_folder_into_the_typed_path() {
    let (_root, home) = tree();
    let b = parse("~/co", Some(&home)).expect("a path");
    assert_eq!(
        complete(&b, "code"),
        format!("~/code{}", std::path::MAIN_SEPARATOR)
    );
    assert_eq!(typed_path("~/code", Some(&home)), Some(home.join("code")));
    assert_eq!(typed_path("~/code/", Some(&home)), Some(home.join("code")));
    assert_eq!(typed_path("code", Some(&home)), None);
}

#[test]
fn git_info_reads_head_without_running_git() {
    let (_root, home) = tree();
    assert_eq!(git_info(&home.join("code/app")), Some(Some("main".into())));
    assert_eq!(git_info(&home.join("code/api")), None);
    std::fs::write(home.join("code/app/.git/HEAD"), "0123456789abcdef\n").expect("detach");
    assert_eq!(
        git_info(&home.join("code/app")),
        Some(None),
        "detached: a repo, no branch"
    );
}

#[test]
fn new_project_folder_is_a_name_or_a_path_and_never_a_full_folder() {
    let (_root, home) = tree();
    let base = home.join("code");
    assert_eq!(
        new_project_dir("demo", &base, Some(&home)).expect("ok"),
        base.join("demo")
    );
    assert_eq!(
        new_project_dir("~/x/y", &base, Some(&home)).expect("ok"),
        home.join("x/y")
    );
    assert!(new_project_dir("", &base, Some(&home)).is_err());
    assert!(
        new_project_dir("app", &base, Some(&home)).is_err(),
        "not empty"
    );
    assert!(
        new_project_dir("notes.txt", &base, Some(&home)).is_err(),
        "a file"
    );
    assert!(
        new_project_dir("../up", &base, Some(&home)).is_err(),
        "no climbing out by name"
    );
    std::fs::create_dir_all(base.join("empty")).expect("mkdir");
    assert!(
        new_project_dir("empty", &base, Some(&home)).is_ok(),
        "an empty folder is fine"
    );
}

#[test]
fn clone_urls_are_plain_https() {
    assert_eq!(
        clone_name("https://github.com/acme/widgets.git").expect("ok"),
        "widgets"
    );
    assert_eq!(
        clone_name("https://github.com/acme/widgets/").expect("ok"),
        "widgets"
    );
    for bad in [
        "git@github.com:acme/widgets.git",
        "ssh://github.com/acme/widgets",
        "http://github.com/acme/widgets",
        "https://token@github.com/acme/widgets",
        "https://github.com/",
        "https://github.com/acme/..",
        "https://github.com/acme/w=x",
        "-https://x/y",
    ] {
        assert!(clone_name(bad).is_err(), "{bad} must be refused");
    }
    let long = format!("https://github.com/{}", "a".repeat(3000));
    assert!(clone_name(&long).is_err(), "over 2,048 bytes");
}
