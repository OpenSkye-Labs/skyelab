//! Acceptance (#12): a filter or merge driver written to the repo's config while SkyeLab's git call is starting
//! never runs. A writer thread keeps adding and removing one while the calls that can run them go on.
#![cfg(unix)] // the planted filter is a shell script

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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

#[test]
fn a_filter_written_mid_call_never_runs() {
    let version = Command::new("git").arg("version").output().expect("git");
    let version = String::from_utf8_lossy(&version.stdout);
    let numbers: Vec<u32> = version
        .trim()
        .trim_start_matches("git version ")
        .split('.')
        .take(2)
        .map(|n| n.parse().unwrap_or(0))
        .collect();
    if numbers < vec![2, 54] {
        eprintln!(
            "skipped: {version} is older than 2.54, where SkyeLab falls back to the listed overrides"
        );
        return;
    }
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    git(&base, &["init", "-q", "-b", "main", "repo"]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@example.com")] {
        git(&repo, &["config", k, v]);
    }
    fs::write(
        repo.join(".gitattributes"),
        "*.txt filter=race merge=race\n",
    )
    .expect("attrs");
    fs::write(repo.join("a.txt"), "0\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let marker = base.join("ran");
    let evil = base.join("evil");
    fs::write(
        &evil,
        format!("#!/bin/sh\necho ran >> '{}'\ncat\n", marker.display()),
    )
    .expect("evil");
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o755)).expect("chmod");

    let config = repo.join(".git").join("config");
    let clean = fs::read_to_string(&config).expect("config");
    let planted = format!(
        "{clean}[filter \"race\"]\n\tclean = {e}\n\tsmudge = {e}\n[merge \"race\"]\n\tdriver = {e}\n",
        e = evil.display()
    );
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (stop, config, planted) = (stop.clone(), config.clone(), planted.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                // Rename, as `git config` does, so git never reads a half-written file.
                for text in [&planted, &clean] {
                    let tmp = config.with_extension("racing");
                    fs::write(&tmp, text).expect("write config");
                    fs::rename(&tmp, &config).expect("swap config");
                    std::thread::sleep(Duration::from_micros(300));
                }
            }
        })
    };
    let started = Instant::now();
    let mut calls = 0;
    while started.elapsed() < Duration::from_secs(4) {
        fs::write(repo.join("a.txt"), format!("{calls}\n")).expect("write");
        // Errors are fine (a call may see the repo mid-swap); a filter running is not.
        let _ = skyelab_git::snapshot_tree(&repo);
        let _ = skyelab_git::status(&repo);
        let _ = skyelab_git::diff(&repo, "HEAD");
        calls += 1;
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().expect("writer");
    // The writer really planted it: plain git runs it.
    fs::write(&config, &planted).expect("plant");
    fs::write(repo.join("a.txt"), "plain\n").expect("write");
    let after_skyelab = fs::read_to_string(&marker).unwrap_or_default();
    git(&repo, &["add", "a.txt"]);
    assert!(
        fs::read_to_string(&marker).is_ok(),
        "the planted filter doesn't run at all"
    );
    assert_eq!(after_skyelab, "", "a racing filter ran in {calls} calls");
}
