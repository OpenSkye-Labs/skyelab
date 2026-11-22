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
            |c| {
                c.add_binding(Binding {
                    path,
                    account: account.to_string(),
                })
            },
            window,
            cx,
        );
    });
    assert_eq!(
        AccountsConfig::load(&path).expect("saved").bindings.len(),
        1
    );

    set(&test, &work.join("api").display().to_string(), cx);
    view.update(cx, |v, cx| v.refresh_test(cx));
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.test_result.clone().flatten().map(|a| a.id),
            Some("acme".into())
        );
    });
    set(&test, "/nonexistent/elsewhere", cx);
    view.update(cx, |v, cx| v.refresh_test(cx));
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
