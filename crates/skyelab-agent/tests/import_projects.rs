//! Acceptance: onboarding's Import projects scan lists each folder an account's sessions ran in, with the session
//! count and the newest one's time, from the account dir only (a linked dir is someone else's and is skipped).

use skyelab_agent::import;
use skyelab_core::Provider;
use std::path::Path;

fn claude_session(home: &Path, dir: &str, name: &str, cwd: &Path) {
    let d = home.join("projects").join(dir);
    std::fs::create_dir_all(&d).expect("mkdir");
    let line = serde_json::json!({"type": "user", "cwd": cwd, "sessionId": name,
        "message": {"role": "user", "content": "hi"}});
    std::fs::write(d.join(format!("{name}.jsonl")), format!("{line}\n")).expect("write");
}

fn codex_session(home: &Path, name: &str, cwd: &Path) {
    let d = home.join("sessions/2026/11/20");
    std::fs::create_dir_all(&d).expect("mkdir");
    let line = serde_json::json!({"type": "session_meta", "payload": {"id": name, "cwd": cwd}});
    std::fs::write(d.join(format!("rollout-{name}.jsonl")), format!("{line}\n")).expect("write");
}

#[test]
fn lists_folders_with_counts_from_both_providers() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (claude, codex) = (tmp.path().join("claude"), tmp.path().join("codex"));
    // Absolute on every platform: `/work/app` has no drive on Windows and would be left out as relative.
    let (app, api) = (
        tmp.path().join("work").join("app"),
        tmp.path().join("work").join("api"),
    );
    let (app, api) = (app.as_path(), api.as_path());
    claude_session(&claude, "-work-app", "s1", app);
    claude_session(&claude, "-work-app", "s2", app);
    claude_session(&claude, "-work-api", "s3", api);
    claude_session(&claude, "-bad", "s4", Path::new("relative/path"));
    codex_session(&codex, "c1", app);
    let found = import::project_folders(Provider::Claude, &claude);
    let counts: Vec<_> = found.iter().map(|(p, n, _)| (p.as_path(), *n)).collect();
    assert_eq!(
        counts,
        [(api, 1), (app, 2)],
        "relative folders are left out"
    );
    assert!(found.iter().all(|(_, _, ms)| *ms > 0));
    let found = import::project_folders(Provider::Codex, &codex);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, app);
}

#[cfg(unix)]
#[test]
fn a_linked_projects_dir_is_not_read() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (home, theirs) = (tmp.path().join("home"), tmp.path().join("theirs"));
    claude_session(&theirs, "-x", "s1", Path::new("/x"));
    std::fs::create_dir_all(&home).expect("mkdir");
    std::os::unix::fs::symlink(theirs.join("projects"), home.join("projects")).expect("link");
    assert!(import::project_folders(Provider::Claude, &home).is_empty());
}
