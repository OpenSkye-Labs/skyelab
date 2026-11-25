//! The folder watcher on real temp repos and the platform's real file events: what it reports, what it ignores,
//! how it coalesces, its per-folder cap, and that it ends when dropped.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use skyelab_git::watch::{FolderWatch, Mode};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A committed repo with `a.txt`, a `.gitignore` for `target/`, and a `feature` branch.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon").join("r");
    std::fs::create_dir(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("a.txt"), "a\n").expect("w");
    std::fs::write(repo.join(".gitignore"), "target/\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["branch", "feature"]);
    (tmp, repo)
}

struct Watched {
    calls: Arc<AtomicUsize>,
    _watch: FolderWatch,
}

fn watch(dir: &Path, mode: Mode) -> Watched {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let watch = FolderWatch::start(dir, mode, move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    assert!(
        watch.wait_ready(Duration::from_secs(60)),
        "the watcher never got ready"
    );
    // Events from setting the repo up (FSEvents can hand over a few from just before the stream) settle first.
    settle(&calls);
    Watched {
        calls,
        _watch: watch,
    }
}

/// Waits until a whole second passes with no report, then starts the count from zero. A fixed sleep was too short on a
/// loaded machine, where earlier events arrive late and land in the next step's count.
fn settle(calls: &AtomicUsize) {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        calls.store(0, Ordering::SeqCst);
        std::thread::sleep(Duration::from_secs(1));
        if calls.load(Ordering::SeqCst) == 0 {
            return;
        }
        assert!(Instant::now() < end, "reports never stopped");
    }
}

fn wait_for_call(w: &Watched, what: &str) {
    let end = Instant::now() + Duration::from_secs(60);
    while w.calls.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < end, "no change reported for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Nothing is reported for `secs`: the debounce (2 s at most) has long fired by then if anything counted.
fn assert_quiet(w: &Watched, secs: u64, what: &str) {
    std::thread::sleep(Duration::from_secs(secs));
    assert_eq!(w.calls.load(Ordering::SeqCst), 0, "{what} was reported");
}

fn modes() -> [Mode; 2] {
    [Mode::Recursive, Mode::PerDir { cap: 4096 }]
}

#[test]
fn reports_a_file_write() {
    for mode in modes() {
        let (_tmp, repo) = repo();
        let w = watch(&repo, mode);
        std::fs::write(repo.join("a.txt"), "changed\n").expect("w");
        wait_for_call(&w, &format!("a write in {mode:?}"));
        // And a file in a folder made after the watch began.
        settle(&w.calls);
        std::fs::create_dir_all(repo.join("src/deep")).expect("mkdir");
        settle(&w.calls);
        std::fs::write(repo.join("src/deep/new.rs"), "x\n").expect("w");
        wait_for_call(&w, &format!("a write in a new folder in {mode:?}"));
    }
}

#[test]
fn reports_head_change() {
    for mode in modes() {
        let (_tmp, repo) = repo();
        let w = watch(&repo, mode);
        git(&repo, &["switch", "-q", "feature"]);
        wait_for_call(&w, &format!("a branch switch in {mode:?}"));
    }
}

#[test]
fn reports_worktree_head_change() {
    for mode in modes() {
        let (tmp, repo) = repo();
        let wt = tmp.path().canonicalize().expect("canon").join("wt");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                skyelab_core::files::without_verbatim(&wt)
                    .to_str()
                    .expect("utf8"),
                "feature",
            ],
        );
        let w = watch(&wt, mode);
        // HEAD and the index live under the main repo's .git/worktrees/wt, outside the watched folder.
        git(&wt, &["switch", "-q", "-c", "other"]);
        wait_for_call(&w, &format!("a worktree branch switch in {mode:?}"));
    }
}

#[test]
fn ignores_gitignored_churn_and_git_internals() {
    for mode in modes() {
        let (_tmp, repo) = repo();
        std::fs::create_dir_all(repo.join("target/debug")).expect("mkdir");
        let w = watch(&repo, mode);
        for i in 0..50 {
            std::fs::write(repo.join(format!("target/debug/o{i}.o")), "x").expect("w");
        }
        std::fs::create_dir_all(repo.join("target/debug/new/deeper")).expect("mkdir");
        std::fs::write(repo.join("target/debug/new/deeper/x"), "x").expect("w");
        // git internals that change no status: objects, logs, our checkpoints, a lock file.
        let objects = repo.join(".git/objects/zz");
        std::fs::create_dir_all(&objects).expect("mkdir");
        std::fs::write(objects.join("junk"), "x").expect("w");
        std::fs::create_dir_all(repo.join(".git/refs/skyelab/checkpoints/k")).expect("mkdir");
        std::fs::write(repo.join(".git/refs/skyelab/checkpoints/k/1"), "x").expect("w");
        std::fs::write(repo.join(".git/index.lock"), "x").expect("w");
        std::fs::remove_file(repo.join(".git/index.lock")).expect("rm");
        assert_quiet(&w, 3, &format!("ignored churn in {mode:?}"));
    }
}

#[test]
fn coalesces_a_burst() {
    for mode in modes() {
        let (_tmp, repo) = repo();
        let w = watch(&repo, mode);
        for i in 0..50 {
            std::fs::write(repo.join(format!("f{i}.txt")), "x").expect("w");
            std::thread::sleep(Duration::from_millis(10));
        }
        wait_for_call(&w, "a burst");
        std::thread::sleep(Duration::from_secs(2));
        // One refresh on a quiet machine; a loaded CI runner can stall the burst past the 300 ms quiet window
        // or the 2 s cap and split it, but 50 writes must never be 50 refreshes.
        let calls = w.calls.load(Ordering::SeqCst);
        assert!(
            (1..=3).contains(&calls),
            "a 0.5 s burst of 50 writes is a refresh or so in {mode:?}, got {calls}"
        );
    }
}

#[test]
fn per_dir_falls_back_over_cap() {
    let (_tmp, repo) = repo();
    for i in 0..10 {
        std::fs::create_dir_all(repo.join(format!("d{i}/sub"))).expect("mkdir");
    }
    let w = watch(&repo, Mode::PerDir { cap: 3 });
    std::fs::write(repo.join("d4/sub/x.txt"), "x").expect("w");
    assert_quiet(&w, 3, "a write past the cap");
    std::fs::write(repo.join("top.txt"), "x").expect("w");
    wait_for_call(&w, "a top-level write past the cap");
    settle(&w.calls);
    git(&repo, &["switch", "-q", "feature"]);
    wait_for_call(&w, "a branch switch past the cap");
}

#[test]
fn ends_when_dropped() {
    let (_tmp, repo) = repo();
    let held = Arc::new(());
    let h = held.clone();
    let watch = FolderWatch::start(&repo, Mode::native(), move || {
        let _ = &h;
    });
    assert!(watch.wait_ready(Duration::from_secs(60)));
    assert_eq!(
        Arc::strong_count(&held),
        2,
        "the watcher holds its callback"
    );
    drop(watch);
    let end = Instant::now() + Duration::from_secs(60);
    while Arc::strong_count(&held) > 1 {
        assert!(Instant::now() < end, "the watcher thread never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_folder_that_becomes_a_repo_is_watched_from_then_on() {
    for mode in modes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = std::fs::canonicalize(tmp.path()).expect("canon");
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let watch = FolderWatch::start(&dir, mode, move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!watch.wait_ready(Duration::from_secs(60)), "not a repo yet");
        std::fs::write(dir.join("loose.txt"), "x").expect("w");
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "{mode:?}: nothing to report outside a repo"
        );

        git(&dir, &["init", "-q"]);
        let w = Watched {
            calls,
            _watch: watch,
        };
        wait_for_call(&w, "the folder becoming a repo");
        // From then on it is watched as any repo: a new file is reported.
        settle(&w.calls);
        std::fs::create_dir(dir.join("src")).expect("mkdir");
        settle(&w.calls);
        std::fs::write(dir.join("src/new.txt"), "x").expect("w");
        wait_for_call(&w, "a write in the new repo");
    }
}

#[test]
fn a_folder_outside_git_ends_its_thread_when_dropped() {
    let tmp = tempfile::tempdir().expect("tmp");
    let held = Arc::new(());
    let h = held.clone();
    let watch = FolderWatch::start(tmp.path(), Mode::native(), move || {
        let _ = &h;
    });
    assert!(!watch.wait_ready(Duration::from_secs(60)), "not a repo");
    drop(watch);
    let end = Instant::now() + Duration::from_secs(60);
    while Arc::strong_count(&held) > 1 {
        assert!(Instant::now() < end, "the watcher thread never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn reports_a_tracked_file_under_an_ignored_path() {
    for mode in modes() {
        let (_tmp, repo) = repo();
        std::fs::create_dir_all(repo.join("target")).expect("mkdir");
        std::fs::write(repo.join("target/keep.txt"), "a\n").expect("w");
        git(&repo, &["add", "-f", "target/keep.txt"]);
        git(&repo, &["commit", "-q", "-m", "keep"]);
        let w = watch(&repo, mode);
        std::fs::write(repo.join("target/keep.txt"), "changed\n").expect("w");
        wait_for_call(&w, &format!("a tracked file under target/ in {mode:?}"));
        // Its untracked neighbours stay ignored.
        settle(&w.calls);
        std::fs::write(repo.join("target/other.o"), "x").expect("w");
        assert_quiet(&w, 3, &format!("an ignored neighbour in {mode:?}"));
    }
}

#[test]
fn a_subfolder_project_hears_edits_elsewhere_in_its_repo() {
    for mode in modes() {
        let (_tmp, repo) = repo();
        std::fs::create_dir_all(repo.join("sub")).expect("mkdir");
        std::fs::write(repo.join("sub/b.txt"), "b\n").expect("w");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "sub"]);
        // `git status` in sub/ lists the whole repo, so the footer and Changes count a.txt too.
        let w = watch(&repo.join("sub"), mode);
        std::fs::write(repo.join("a.txt"), "changed\n").expect("w");
        wait_for_call(&w, &format!("an edit outside the subfolder in {mode:?}"));
    }
}
