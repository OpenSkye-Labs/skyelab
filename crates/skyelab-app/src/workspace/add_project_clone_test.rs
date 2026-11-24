//! Acceptance: Clone from URL takes plain https only, asks for the account first, clones with that account's own gh
//! (the fake gh records its dir), binds after the clone succeeds, and a failed clone binds nothing.
#![cfg(all(unix, debug_assertions))] // the fake gh is a shell script; the CA hook is debug-only

#[path = "../../../skyelab-git/tests/support/https_remote.rs"]
mod https_remote;

use super::add_project_rig::{choose, dialog_open, rig, shown, type_in};
use super::threads_test::wait_until;
use gpui::TestAppContext;
use std::process::Command;
use std::sync::OnceLock;

/// One server per test binary: the CA it makes can be trusted only once (`trust_test_ca`'s first call wins).
fn server() -> &'static (https_remote::HttpsRemote, std::path::PathBuf) {
    static SERVER: OnceLock<(https_remote::HttpsRemote, std::path::PathBuf)> = OnceLock::new();
    SERVER.get_or_init(|| {
        let root = Box::leak(Box::new(tempfile::tempdir().expect("root")));
        let root = std::fs::canonicalize(root.path()).expect("canon");
        let remote = https_remote::HttpsRemote::start(&root);
        skyelab_git::trust_test_ca(&remote.ca_file);
        (remote, root)
    })
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A served bare repo `name.git` with one commit; its URL.
fn served(remote: &https_remote::HttpsRemote, root: &std::path::Path, name: &str) -> String {
    let bare = root.join(format!("{name}.git"));
    git(root, &["init", "-q", "--bare", "-b", "main", bare.to_str().expect("utf8")]);
    let work = root.join(format!("{name}-work"));
    std::fs::create_dir_all(&work).expect("mkdir");
    git(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("a.txt"), "a\n").expect("write");
    git(&work, &["add", "."]);
    git(&work, &["-c", "user.name=t", "-c", "user.email=t@e", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"]);
    git(&work, &["push", "-q", bare.to_str().expect("utf8"), "main"]);
    format!("{}/{name}.git", remote.base)
}

#[gpui::test]
fn clone_asks_the_account_first_and_uses_its_gh(cx: &mut TestAppContext) {
    let (remote, served_root) = server();
    let url = served(remote, served_root, "widgets");
    let (rig, cx) = rig(cx, |_, _| {});
    rig.open_add(cx);
    choose("Clone from URL", cx);
    for bad in ["http://x.test/a/b", "https://me:pw@x.test/a/b"] {
        type_in(bad, cx);
        let s = shown(cx);
        assert_eq!(s.rows.len(), 1, "{s:?}");
        assert_eq!(s.rows[0].0, "Can't add", "{bad} refused with a reason");
        cx.simulate_keystrokes("cmd-a backspace");
    }
    type_in(&url, cx);
    choose("Clone into ~/code/widgets", cx);
    assert_eq!(shown(cx).page.as_deref(), Some("Which account does widgets run on?"));
    let dest = rig.home.join("code/widgets");
    assert!(!dest.exists(), "nothing cloned before the pick");
    choose("WORK", cx);
    rig.wait_project(&dest, cx);
    assert!(!dialog_open(cx));
    assert_eq!(std::fs::read_to_string(dest.join("a.txt")).expect("cloned"), "a\n");
    assert!(
        rig.home.join(".config/gh-work/fake-gh.credential").exists(),
        "the WORK account's own gh answered"
    );
    assert_eq!(rig.bound_to(&dest).as_deref(), Some("work"));
    assert!(remote.seen.lock().expect("log").iter().any(|s| s.method == "POST" && s.target.contains("widgets") && s.authorized));
}

#[gpui::test]
fn a_failed_clone_binds_nothing(cx: &mut TestAppContext) {
    let (remote, served_root) = server();
    let url = served(remote, served_root, "private");
    let (rig, cx) = rig(cx, |_, _| {});
    rig.open_add(cx);
    choose("Clone from URL", cx);
    type_in(&url, cx);
    choose("Clone into ~/code/private", cx);
    // HOME has no gh dir: no login, and the remote wants one.
    choose("HOME", cx);
    let ws = rig.ws.clone();
    wait_until(cx, "the failure notice", |cx| {
        ws.read_with(cx, |ws, _| ws.notices.iter().any(|n| n.contains("git clone")))
    });
    let dest = rig.home.join("code/private");
    assert!(!dest.exists());
    assert_eq!(rig.bound_to(&dest), None);
    assert!(rig.projects(cx).is_empty());
}
