use std::path::Path;

use super::*;

#[test]
fn missing_git_binary_is_typed() {
    let err = git_with(Path::new("/definitely/not/a/git"), Path::new("."), &["--version"]).expect_err("no binary");
    assert!(matches!(err, Error::GitNotInstalled), "{err:?}");
}

#[test]
fn unquotes_c_style_paths() {
    assert_eq!(diff::unquote(r#""tab\there \"q\" \\ \303\274""#), "tab\there \"q\" \\ ü");
    assert_eq!(diff::unquote("plain name"), "plain name");
}

#[test]
fn parses_porcelain_worktree_list() {
    let out = b"worktree /r\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
worktree /r/wt with space\0HEAD 2222222222222222222222222222222222222222\0detached\0\0";
    let list = worktree::parse_list(out);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].branch.as_deref(), Some("main"));
    assert_eq!(list[1].path, Path::new("/r/wt with space"));
    assert!(list[1].detached);
    assert_eq!(list[1].branch, None);
}
