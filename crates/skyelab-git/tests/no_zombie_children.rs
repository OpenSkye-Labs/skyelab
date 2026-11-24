//! A git call that fails must still reap every child it started: an unreaped child stays a zombie until the app
//! exits, and enough of them hit the per-user process limit (spawns then fail with EAGAIN, as on GitHub's macOS
//! runners). Its own binary, so no other test's children are counted.
#![cfg(unix)]

use std::process::Command;

fn zombie_children() -> usize {
    let me = std::process::id().to_string();
    let out = Command::new("ps")
        .args(["-A", "-o", "ppid=,stat="])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| {
            let mut f = l.split_whitespace();
            f.next() == Some(me.as_str()) && f.next().is_some_and(|s| s.starts_with('Z'))
        })
        .count()
}

#[test]
fn failing_git_calls_in_a_folder_that_is_no_repo_leave_no_zombies() {
    let dir = tempfile::tempdir().unwrap();
    for _ in 0..5 {
        assert!(skyelab_git::status(dir.path()).is_err());
    }
    assert_eq!(zombie_children(), 0);
}
