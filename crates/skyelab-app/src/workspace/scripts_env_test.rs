//! Acceptance: a project script's terminal gets only the terminal env SkyeLab builds for the folder's bound account
//! (skyelab-terminal then adds just the terminal allowlist of the parent, see its env_policy_test), plus the
//! project and worktree paths; an unbound or rebound folder is refused; and a setup script runs in every new
//! worktree, typed into a real shell there.

use super::scripts::{script_options, typed};
use super::thread_worktree::ToggleWorktree;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::TestAppContext;

#[test]
fn a_script_terminal_gets_only_the_bound_accounts_env() {
    let f = fixture();
    let (options, account) =
        script_options(&f.config, &f.project, &f.project, None).expect("bound folder");
    assert_eq!(account.map(|a| a.id).as_deref(), Some("acme"));
    assert_eq!(options.cwd, f.project);
    assert_eq!(
        options.command, None,
        "the login shell, with the command typed in"
    );
    assert_eq!(
        options.env["CLAUDE_CONFIG_DIR"],
        f.account_dir.to_str().expect("utf-8")
    );
    assert_eq!(
        options.env["SKYELAB_PROJECT_ROOT"],
        f.project.to_str().expect("utf-8")
    );
    assert!(!options.env.contains_key("SKYELAB_WORKTREE_PATH"));
    for k in options.env.keys() {
        let known = [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "GEMINI_HOME",
            "GIT_CONFIG_COUNT",
            "SKYELAB_PROJECT_ROOT",
        ]
        .contains(&k.as_str())
            || k.starts_with("GIT_CONFIG_KEY_")
            || k.starts_with("GIT_CONFIG_VALUE_");
        assert!(known, "unexpected {k} in a script's env");
    }

    // A worktree names itself too; a thread's account must still be the folder's.
    let wt = f.project.join(".skyelab/worktrees/w");
    std::fs::create_dir_all(&wt).expect("mkdir");
    let (options, _) = script_options(&f.config, &wt, &f.project, Some("acme")).expect("worktree");
    assert_eq!(
        options.env["SKYELAB_WORKTREE_PATH"],
        wt.to_str().expect("utf-8")
    );
    assert!(script_options(&f.config, &wt, &f.project, Some("other")).is_err());
    let unbound = f.project.parent().expect("parent").to_path_buf();
    assert!(script_options(&f.config, &unbound, &unbound, None).is_err());

    assert_eq!(typed("npm run dev ").expect("typed"), " npm run dev\r");
    assert!(typed("  ").is_err());
    assert!(typed("a\0b").is_err());
}

#[gpui::test]
fn a_setup_script_runs_in_each_new_worktree(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    let project = ws.read_with(cx, |ws, _| ws.project.clone().expect("project"));
    {
        let store = f.store.lock().expect("lock");
        store
            .add_project_script(
                project.id,
                "Setup",
                r#"printf '%s|%s' "$SKYELAB_WORKTREE_PATH" "$CLAUDE_CONFIG_DIR" > setup-ran.txt"#,
                true,
            )
            .expect("add");
        store
            .add_project_script(project.id, "Dev", "touch dev-ran.txt", false)
            .expect("add");
    }
    cx.dispatch_action(ToggleWorktree);
    send(cx);
    wait_until(cx, "thread row", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() || t.error.is_some())
    });
    let wt = thread
        .read_with(cx, |t, _| t.thread.clone())
        .and_then(|t| t.worktree_path)
        .expect("worktree");
    let out = wt.join("setup-ran.txt");
    crate::test_wait::wait_until(cx, "the setup script", |_| {
        std::fs::read_to_string(&out).is_ok_and(|s| s.contains('|'))
    });
    let ran = std::fs::read_to_string(&out).expect("the setup script ran in the worktree");
    assert_eq!(
        ran,
        format!("{}|{}", wt.display(), f.account_dir.display()),
        "in the worktree, on the thread's account"
    );
    assert!(
        !wt.join("dev-ran.txt").exists(),
        "only setup scripts run on their own"
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.terminals.len(), 1, "the setup script's terminal tab");
    });
}
