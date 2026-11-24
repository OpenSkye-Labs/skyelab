//! Acceptance: Settings edits an account's GitHub config dir, shows who is logged in there, and
//! Log in asks for a terminal running `gh auth login` under that dir only.

use super::*;
use gpui::{TestAppContext, VisualTestContext};
use std::cell::RefCell;

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh")
}

fn set(input: &Entity<InputState>, text: &str, cx: &mut VisualTestContext) {
    let text = text.to_string();
    input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
}

#[gpui::test]
fn github_dir_is_saved_login_shown_and_log_in_requested(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake gh is a real process
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
        cx.set_global(crate::workspace::GhBin(fake_gh()));
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let path = root.join("accounts.toml");
    let gh_dir = root.join("gh-acme");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    std::fs::write(gh_dir.join("fake-login"), "octo-acme").expect("login");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(
            Some(path.clone()),
            AccountsConfig::default(),
            None,
            window,
            cx,
        )
    });
    let inputs = view.read_with(cx, |v, _| {
        [
            v.id_input.clone(),
            v.label_input.clone(),
            v.dir_input.clone(),
            v.gh_input.clone(),
        ]
    });
    let gh_text = gh_dir.to_str().expect("utf-8");
    for (input, text) in inputs
        .iter()
        .zip(["acme", "Acme", "/home/me/.claude-acme", gh_text])
    {
        set(input, text, cx);
    }
    view.update_in(cx, |v, window, cx| v.save_account(&SaveAccount, window, cx));
    cx.run_until_parked();
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(
        saved.accounts[0].gh_config_dir.as_deref(),
        Some(gh_dir.as_path())
    );

    // The row learns who is logged in there, off the UI thread.
    crate::test_wait::wait_until(cx, "the gh login", |cx| {
        view.read_with(cx, |v, _| v.gh_logins.contains_key("acme"))
    });
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.gh_logins.get("acme").map(|l| l.as_deref()),
            Some(Some("octo-acme"))
        )
    });

    // Log in asks the workspace for a login terminal on that dir.
    let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = asked.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &SettingsEvent, _| {
            if let SettingsEvent::GhLogin { dir } = e {
                seen.borrow_mut().push(dir.clone());
            }
        })
    });
    view.update_in(cx, |v, window, cx| {
        v.gh_log_in(&GhLogIn { id: "acme".into() }, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(*asked.borrow(), vec![gh_dir.clone()]);

    let opts = crate::workspace::gh_login_options(&fake_gh(), &gh_dir).expect("options");
    let (program, args) = opts.command.expect("runs gh");
    assert_eq!(Path::new(&program), fake_gh());
    assert_eq!(args, ["auth", "login", "--insecure-storage"]);
    assert_eq!(
        opts.env.get("GH_CONFIG_DIR").map(String::as_str),
        Some(gh_text)
    );
    assert_eq!(opts.env.len(), 1, "{:?}", opts.env);
}

#[gpui::test]
fn log_in_without_a_github_folder_sets_the_default_one_first(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
        cx.set_global(crate::workspace::GhBin(fake_gh()));
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            provider: Provider::Claude,
            config_dir: "/home/me/.claude-acme".into(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add");
    config.save(&path).expect("save");
    let (view, cx) = cx
        .add_window_view(|window, cx| Settings::new(Some(path.clone()), config, None, window, cx));
    let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = asked.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &SettingsEvent, _| {
            if let SettingsEvent::GhLogin { dir } = e {
                seen.borrow_mut().push(dir.clone());
            }
        })
    });
    view.update_in(cx, |v, window, cx| {
        v.gh_log_in(&GhLogIn { id: "acme".into() }, window, cx)
    });
    cx.run_until_parked();
    let want = dirs::home_dir().expect("home").join(".config/gh-acme");
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(
        saved.accounts[0].gh_config_dir.as_deref(),
        Some(want.as_path())
    );
    assert_eq!(
        *asked.borrow(),
        vec![want],
        "then the login terminal is asked for"
    );
}

#[gpui::test]
fn set_up_keeps_what_the_file_says_about_the_account(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
        cx.set_global(crate::workspace::GhBin(fake_gh()));
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    let base = Account {
        id: "acme".into(),
        label: "Acme".into(),
        provider: Provider::Claude,
        config_dir: "/home/me/.claude-acme".into(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let mut config = AccountsConfig::default();
    config.add_account(base.clone()).expect("add");
    config.save(&path).expect("save");
    let (view, cx) = cx
        .add_window_view(|window, cx| Settings::new(Some(path.clone()), config, None, window, cx));
    // Edited elsewhere after the page loaded: Set up must not put the page's old copy back.
    let mut disk = AccountsConfig::load(&path).expect("load");
    disk.update_account(Account {
        label: "Acme Corp".into(),
        ssh_agent: true,
        ..base
    })
    .expect("edit");
    disk.save(&path).expect("save");
    view.update_in(cx, |v, window, cx| {
        v.gh_log_in(&GhLogIn { id: "acme".into() }, window, cx)
    });
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(saved.accounts[0].label, "Acme Corp");
    assert!(
        saved.accounts[0].ssh_agent,
        "ssh_agent kept as the file has it"
    );
    assert!(saved.accounts[0].gh_config_dir.is_some());
}
