use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn add_account_bind_and_test_folder(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cfg").join("accounts.toml");
    let work = dir.path().join("work");
    std::fs::create_dir_all(work.join("api")).expect("mkdir");
    let (view, cx) = cx.add_window_view(|window, cx| {
        let view = Settings::new(
            Some(path.clone()),
            AccountsConfig::default(),
            None,
            window,
            cx,
        );
        window.focus(&view.focus, cx);
        view
    });

    let set = |input: &Entity<InputState>, text: &str, cx: &mut gpui::VisualTestContext| {
        let text = text.to_string();
        input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
    };
    let (id, label, dir_in, test) = view.read_with(cx, |v, _| {
        (
            v.id_input.clone(),
            v.label_input.clone(),
            v.dir_input.clone(),
            v.test_input.clone(),
        )
    });
    set(&id, "acme", cx);
    set(&label, "Acme Team", cx);
    set(&dir_in, "/home/me/.claude-acme", cx);
    view.update_in(cx, |v, window, cx| v.focus.focus(window, cx));
    cx.simulate_keystrokes("cmd-enter"); // the SaveAccount binding
    cx.run_until_parked();
    assert_eq!(
        AccountsConfig::load(&path).expect("saved").accounts[0].id,
        "acme"
    );

    // Duplicate id: core error shows inline, nothing saved.
    set(&id, "acme", cx);
    set(&dir_in, "/home/me/.claude-acme", cx);
    view.update_in(cx, |v, window, cx| v.save_account(&SaveAccount, window, cx));
    view.read_with(cx, |v, _| {
        assert!(
            v.error
                .as_deref()
                .is_some_and(|e| e.contains("already exists"))
        );
    });

    // The picker needs a real OS dialog, so bind the way its callback does.
    view.update_in(cx, |v, window, cx| {
        let account = v
            .account_select
            .read(cx)
            .selected_value()
            .cloned()
            .expect("acme preselected");
        let path = work.clone();
        v.apply(
            |c| c.add_binding(Binding::new(path, account.to_string())),
            window,
            cx,
        );
    });
    assert_eq!(
        AccountsConfig::load(&path).expect("saved").bindings.len(),
        1
    );

    set(&test, &work.join("api").display().to_string(), cx);
    view.update(cx, |v, cx| v.refresh_fs(cx));
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.test_result.clone().flatten().map(|a| a.id),
            Some("acme".into())
        );
    });
    set(&test, "/nonexistent/elsewhere", cx);
    view.update(cx, |v, cx| v.refresh_fs(cx));
    view.read_with(cx, |v, _| assert_eq!(v.test_result, Some(None)));

    // Removing a bound account is refused inline.
    view.update_in(cx, |v, window, cx| {
        v.apply(|c| c.remove_account("acme"), window, cx)
    });
    view.read_with(cx, |v, _| {
        assert!(
            v.error
                .as_deref()
                .is_some_and(|e| e.contains("still bound"))
        );
        assert_eq!(v.config.accounts.len(), 1);
    });
}

#[gpui::test]
fn load_error_locks_editing_so_the_file_is_not_overwritten(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("accounts.toml");
    let broken = "[[binding]]\npath = \"/w\"\naccount = \"typo\"\n";
    std::fs::write(&path, broken).expect("write");
    let error = AccountsConfig::load(&path).expect_err("unknown account");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(
            Some(path.clone()),
            AccountsConfig::default(),
            Some(format!("{error:#}")),
            window,
            cx,
        )
    });
    let saved = view.update_in(cx, |v, window, cx| {
        v.apply(
            |c| {
                c.add_account(Account {
                    id: "x".into(),
                    label: "X".into(),
                    config_dir: "/home/me/.claude-x".into(),
                    provider: Default::default(),
                    gh_config_dir: None,
                    ssh_agent: false,
                })
            },
            window,
            cx,
        )
    });
    assert!(!saved);
    assert_eq!(std::fs::read_to_string(&path).expect("read"), broken);
    view.read_with(cx, |v, _| {
        assert!(
            v.error
                .as_deref()
                .is_some_and(|e| e.contains("Editing is off"))
        );
    });
}

#[cfg(unix)]
#[gpui::test]
fn reconfirm_stale_binding(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let path = root.join("accounts.toml");
    let acme = root.join("acme");
    let other = root.join("other");
    std::fs::create_dir(&acme).expect("mkdir");
    std::fs::create_dir(&other).expect("mkdir");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: "/home/me/.claude-acme".into(),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
    config
        .add_binding(Binding::new(acme.clone(), "acme".into()))
        .expect("bind");
    std::fs::remove_dir(&acme).expect("rm");
    std::os::unix::fs::symlink(&other, &acme).expect("swap for a symlink");

    let (view, cx) = cx
        .add_window_view(|window, cx| Settings::new(Some(path.clone()), config, None, window, cx));
    view.read_with(cx, |v, _| {
        assert_eq!(v.stale, vec![(acme.clone(), Some(other.clone()))])
    });

    let reconfirm = ReconfirmBinding {
        path: acme.clone(),
        now: other.clone(),
    };
    view.update_in(cx, |v, window, cx| {
        v.reconfirm_binding(&reconfirm, window, cx)
    });
    view.read_with(cx, |v, _| {
        assert!(v.stale.is_empty());
        assert_eq!(
            v.config.resolve(&other).map(|a| a.id.as_str()),
            Some("acme")
        );
    });
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(saved.bindings[0].resolved(), Some(other.as_path()));

    // Swapped again while the screen is open: the next refresh shows it.
    std::fs::remove_file(&acme).expect("rm link");
    std::os::unix::fs::symlink(&root, &acme).expect("retarget");
    view.update(cx, |v, cx| v.refresh_fs(cx));
    view.read_with(cx, |v, _| {
        assert_eq!(v.stale, vec![(acme.clone(), Some(root.clone()))])
    });
}
