//! A user's git config must not change what the parsers see. Repo-local config overrides global the
//! same way, so setting it here stands in for a hostile `~/.gitconfig`.

use std::fs;
use std::path::Path;
use std::process::Command;

use skyelab_git::ChangeKind;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn hostile_config_is_ignored() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("old ü.txt"), "a\nb\nc\nd\n").expect("write");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    for (k, v) in [
        ("status.renames", "false"),
        ("status.showUntrackedFiles", "no"),
        ("status.relativePaths", "true"),
        ("diff.renames", "false"),
        ("diff.noprefix", "true"),
        ("diff.relative", "true"),
        ("core.quotePath", "true"),
        ("color.ui", "always"),
    ] {
        git(root, &["config", k, v]);
    }
    git(root, &["mv", "old ü.txt", "new ü.txt"]);
    fs::write(root.join("untracked.txt"), "u\n").expect("write");
    fs::create_dir(root.join("sub")).expect("mkdir");

    let st = skyelab_git::status(&root.join("sub")).expect("status");
    assert_eq!(st.staged.len(), 1, "{st:?}");
    assert_eq!(st.staged[0].kind, ChangeKind::Renamed);
    assert_eq!(st.staged[0].path, Path::new("new ü.txt"));
    assert_eq!(st.untracked, vec![Path::new("untracked.txt").to_path_buf()]);

    let diffs = skyelab_git::diff(&root.join("sub"), "HEAD").expect("diff");
    assert_eq!(diffs.len(), 1, "{diffs:#?}");
    assert_eq!(diffs[0].kind, ChangeKind::Renamed);
    assert_eq!(diffs[0].old_path.as_deref(), Some(Path::new("old ü.txt")));
    assert_eq!(diffs[0].new_path.as_deref(), Some(Path::new("new ü.txt")));
}
