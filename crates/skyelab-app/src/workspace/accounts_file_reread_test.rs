//! Terminals and the Changes tab act on accounts.toml as saved right now, like agent launches, not on the
//! window's copy of it: an outside edit that rebinds or unbinds a folder applies to them at once.

use super::changes_model_test::git;
use super::publish_test::{error, set_input, thread_and_changes};
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use super::*;
use gpui::TestAppContext;
use skyelab_core::Binding;

#[gpui::test]
fn a_terminal_follows_the_saved_file(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    let accounts = f.project.parent().expect("parent").join("accounts.toml");
    let mut unbound = f.config.clone();
    unbound.remove_binding(&f.project).expect("unbind");
    unbound.save(&accounts).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(accounts)));

    let job = ws.read_with(cx, |ws, cx| ws.terminal_job(cx));
    let (opts, account) = job().expect("options");
    assert!(
        account.is_none(),
        "an unbound folder's tab shows no account"
    );
    assert_ne!(
        opts.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        f.account_dir.to_str(),
        "the folder is unbound in the file, so the terminal gets no account"
    );
}

#[gpui::test]
fn commit_is_refused_once_the_file_rebinds_the_folder(cx: &mut TestAppContext) {
    let f = fixture();
    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&f.project, &["config", k, v]);
    }
    std::fs::write(f.project.join("new.txt"), "hi\n").expect("write");
    changes.update(cx, |c, cx| c.refresh(cx));
    wait_until(cx, "new.txt listed", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes
                .as_ref()
                .is_some_and(|ch| ch.files.iter().any(|f| f.path.ends_with("new.txt")))
        })
    });

    let accounts = f.project.parent().expect("parent").join("accounts.toml");
    let mut rebound = f.config.clone();
    rebound.remove_binding(&f.project).expect("unbind");
    rebound
        .add_binding(Binding::new(f.project.clone(), "other".into()))
        .expect("rebind");
    rebound.save(&accounts).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(accounts)));

    set_input(
        &changes.read_with(cx, |c, _| c.commit_message.clone()),
        "Add new.txt",
        cx,
    );
    changes.update_in(cx, |c, window, cx| c.commit(window, cx));
    wait_until(cx, "commit answered", |cx| {
        changes.read_with(cx, |c, _| {
            c.publish_error.is_some() || c.publish_note.is_some()
        })
    });
    let e = error(&changes, cx).expect("refused");
    assert!(e.contains("bound to other"), "{e}");
    assert_eq!(
        String::from_utf8_lossy(
            &skyelab_core::process::output(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&f.project)
                    .args(["rev-list", "--count", "HEAD"]),
                None,
            )
            .expect("git")
            .stdout
        )
        .trim(),
        "1",
        "nothing was committed"
    );
}

#[gpui::test]
fn push_uses_a_github_login_added_to_the_file_meanwhile(cx: &mut TestAppContext) {
    let f = fixture(); // the window's config has no gh dir for "acme"
    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    let root = f.project.parent().expect("parent").to_path_buf();
    let gh_dir = root.join("gh-acme");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    let mut saved = f.config.clone();
    saved.accounts[0].gh_config_dir = Some(gh_dir);
    let accounts = root.join("accounts.toml");
    saved.save(&accounts).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(accounts)));

    changes.update_in(cx, |c, window, cx| c.push(window, cx));
    wait_until(cx, "push answered", |cx| {
        changes.read_with(cx, |c, _| {
            c.publish_error.is_some() || c.publish_note.is_some()
        })
    });
    // The repo has no remote, so the push itself fails, but past the login check.
    let e = error(&changes, cx).unwrap_or_default();
    assert!(!e.contains("no GitHub login"), "{e}");
}

/// The terminal env loads in the background: a folder switch meanwhile drops the terminal asked for before it.
#[gpui::test]
fn a_terminal_for_a_folder_left_meanwhile_does_not_open(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    let other = f.project.parent().expect("parent").to_path_buf();
    ws.update_in(cx, |ws, window, cx| {
        ws.new_terminal(&NewTerminal, window, cx);
        ws.folder = other; // before the options come back
    });
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 0);

    let back = f.project.clone();
    ws.update_in(cx, |ws, window, cx| {
        ws.folder = back;
        ws.new_terminal(&NewTerminal, window, cx);
    });
    for _ in 0..200 {
        cx.run_until_parked();
        if ws.read_with(cx, |ws, _| ws.terminals.len()) == 1 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the terminal for the current folder opened");
}
