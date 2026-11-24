use super::*;

fn state(dir: &str, git_dir: &str, common: &str) -> State {
    State {
        dir: dir.into(),
        toplevel: dir.into(),
        git_dir: git_dir.into(),
        common: common.into(),
        mode: Mode::Recursive,
        nested: Vec::new(),
        ignores: Vec::new(),
        tracked_ignored: HashSet::new(),
        index_moved: false,
        dirs: HashSet::new(),
        capped: false,
    }
}

#[test]
fn git_state_that_counts() {
    let s = state("/r", "/r/.git", "/r/.git");
    for (p, want) in [
        ("/r/.git/HEAD", true),
        ("/r/.git/index", true),
        ("/r/.git/index.lock", false),
        ("/r/.git/refs/heads/main", true),
        ("/r/.git/refs/heads/main.lock", false),
        ("/r/.git/packed-refs", true),
        ("/r/.git/refs/skyelab/checkpoints/k/1", false),
        ("/r/.git/objects/ab/cdef", false),
        ("/r/.git/logs/HEAD", false),
        ("/r/.git/worktrees/wt/HEAD", false),
        ("/r/src/a.rs", true),
        ("/r/vendor/x/.git/HEAD", false),
        ("/elsewhere/a.rs", false),
    ] {
        assert_eq!(s.relevant(Path::new(p)), want, "{p}");
    }
}

/// Per-folder mode, forced on any platform: a deleted folder's watches (it and its subfolders) leave the count.
#[test]
fn a_deleted_folder_leaves_the_per_folder_count() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    let out = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git init");
    std::fs::create_dir_all(repo.join("a/b/c")).expect("mkdir");
    std::fs::create_dir_all(repo.join("keep")).expect("mkdir");
    let mut s = State::open(&repo, Mode::PerDir { cap: 100 }).expect("a repo");
    let mut w = RecommendedWatcher::new(|_| {}, Config::default()).expect("watcher");
    assert!(s.watch_all(&mut w));
    assert_eq!(
        s.dirs.len(),
        5,
        "the top, a, a/b, a/b/c and keep: {:?}",
        s.dirs
    );

    std::fs::remove_dir_all(repo.join("a")).expect("rm");
    let gone = notify::Event::new(EventKind::Remove(notify::event::RemoveKind::Folder))
        .add_path(repo.join("a"));
    s.handle(Ok(gone), &mut w);
    assert_eq!(s.dirs, HashSet::from([repo.clone(), repo.join("keep")]));

    // A folder that still exists keeps its watch, whatever the event says.
    let stale = notify::Event::new(EventKind::Remove(notify::event::RemoveKind::Any))
        .add_path(repo.join("keep"));
    s.handle(Ok(stale), &mut w);
    assert_eq!(s.dirs.len(), 2);
}

#[test]
fn a_linked_worktree_reads_its_own_head_and_the_shared_refs() {
    let s = state("/wt", "/r/.git/worktrees/wt", "/r/.git");
    for (p, want) in [
        ("/r/.git/worktrees/wt/HEAD", true),
        ("/r/.git/worktrees/wt/index", true),
        ("/r/.git/worktrees/wt/logs/HEAD", false),
        ("/r/.git/HEAD", false),
        ("/r/.git/index", false),
        ("/r/.git/refs/heads/x", true),
        ("/r/src/a.rs", false),
        ("/wt/a.rs", true),
    ] {
        assert_eq!(s.relevant(Path::new(p)), want, "{p}");
    }
}
