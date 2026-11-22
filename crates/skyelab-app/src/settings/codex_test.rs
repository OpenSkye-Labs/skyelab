//! Acceptance: Settings adds a Codex account, and its Log in asks for a `codex login` terminal under that
//! account's `CODEX_HOME` only.

use super::*;
use gpui::TestAppContext;
use std::cell::RefCell;

#[gpui::test]
fn add_a_codex_account_and_log_in(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("cfg").join("accounts.toml");
    let codex_home = tmp.path().join("codex-oai");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(
            Some(path.clone()),
            AccountsConfig::default(),
            None,
            window,
            cx,
        )
    });
    let set = |input: &Entity<InputState>, text: &str, cx: &mut gpui::VisualTestContext| {
        let text = text.to_string();
        input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
    };
    let (id, label, dir) = view.read_with(cx, |v, _| {
        (
            v.id_input.clone(),
            v.label_input.clone(),
            v.dir_input.clone(),
        )
    });
    set(&id, "oai", cx);
    set(&label, "OpenAI", cx);
    set(&dir, codex_home.to_str().expect("utf-8"), cx);
    view.update_in(cx, |v, window, cx| {
        assert_eq!(v.provider, Provider::Claude, "the form starts on Claude");
        v.provider = Provider::Codex; // what the "Codex" toggle does
        v.save_account(&SaveAccount, window, cx);
    });
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(saved.accounts[0].provider, Provider::Codex);
    view.read_with(cx, |v, _| {
        assert_eq!(v.provider, Provider::Claude, "saving resets the form")
    });

    // Editing loads the provider; cancelling resets it.
    view.update_in(cx, |v, window, cx| {
        v.edit_account(&EditAccount { id: "oai".into() }, window, cx);
        assert_eq!(v.provider, Provider::Codex);
        v.cancel_edit(&CancelEdit, window, cx);
        assert_eq!(v.provider, Provider::Claude);
    });

    let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = asked.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &SettingsEvent, _| {
            if let SettingsEvent::CodexLogin { dir } = e {
                seen.borrow_mut().push(dir.clone());
            }
        })
    });
    view.update(cx, |v, cx| v.codex_log_in("oai", cx));
    cx.run_until_parked();
    assert_eq!(*asked.borrow(), vec![codex_home.clone()]);

    let opts = crate::workspace::github::codex_login_options(Path::new("/opt/codex"), &codex_home)
        .expect("options");
    assert_eq!(
        opts.command,
        Some(("/opt/codex".into(), vec!["login".into()]))
    );
    assert_eq!(
        opts.env,
        HashMap::from([(
            "CODEX_HOME".into(),
            codex_home.to_str().expect("utf-8").into()
        )])
    );
    assert!(codex_home.is_dir(), "codex needs CODEX_HOME to exist");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&codex_home)
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}

#[gpui::test]
fn a_claude_account_has_no_codex_login(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            provider: Provider::Claude,
            config_dir: "/home/me/.claude-acme".into(),
            gh_config_dir: None,
        })
        .expect("add");
    let (view, cx) = cx.add_window_view(|window, cx| Settings::new(None, config, None, window, cx));
    let asked = std::rc::Rc::new(RefCell::new(0));
    let seen = asked.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, _: &SettingsEvent, _| {
            *seen.borrow_mut() += 1
        })
    });
    view.update(cx, |v, cx| v.codex_log_in("acme", cx));
    cx.run_until_parked();
    assert_eq!(*asked.borrow(), 0);
    view.read_with(cx, |v, _| {
        assert!(
            v.error
                .as_deref()
                .is_some_and(|e| e.contains("not a Codex account"))
        )
    });
}
