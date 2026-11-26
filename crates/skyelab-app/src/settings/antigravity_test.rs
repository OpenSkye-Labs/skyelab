//! Acceptance: Settings adds an Antigravity account, `agent_login_options` builds an isolated
//! `agy acp --gemini-dir <dir> --app-data-dir antigravity-acp --login` terminal with keyring-isolation env,
//! and project terminals isolate `GEMINI_HOME` to a private empty dir when unbound or on another provider.

use super::*;
use gpui::TestAppContext;
use std::cell::RefCell;

#[gpui::test]
fn add_an_antigravity_account_and_prepare_isolated_login(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("cfg").join("accounts.toml");
    let gemini_home = tmp.path().join("antigravity-work");
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
    set(&id, "agy", cx);
    set(&label, "Antigravity Work", cx);
    set(&dir, gemini_home.to_str().expect("utf-8"), cx);
    cx.run_until_parked();
    let toggle = cx
        .debug_bounds("provider-pick-Antigravity")
        .expect("the Antigravity toggle is drawn");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    view.update_in(cx, |v, window, cx| {
        assert_eq!(v.provider, Provider::Antigravity);
        v.save_account(&SaveAccount, window, cx);
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("provider-agy").is_some(),
        "the card shows its provider"
    );
    assert!(
        cx.debug_bounds("antigravity-login-agy").is_some(),
        "an Antigravity card offers Log in"
    );
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(saved.accounts[0].provider, Provider::Antigravity);
    view.read_with(cx, |v, _| {
        assert_eq!(v.provider, Provider::Claude, "saving resets the form")
    });

    let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = asked.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &SettingsEvent, _| {
            if let SettingsEvent::AgentLogin { id } = e {
                seen.borrow_mut().push(id.clone());
            }
        })
    });
    view.update(cx, |v, cx| v.log_in("agy", cx));
    cx.run_until_parked();
    assert_eq!(*asked.borrow(), vec!["agy".to_string()]);

    use crate::workspace::github::{agent_login_options, login_args};
    let opts = agent_login_options(
        Path::new("/opt/agy"),
        login_args(Provider::Antigravity),
        &saved.accounts[0],
    )
    .expect("options");
    let home_str = gemini_home.to_str().expect("utf-8").to_string();
    let (cmd, args) = opts.command.expect("command");
    assert_eq!(cmd, "/opt/agy");
    assert_eq!(
        args,
        vec![
            "acp",
            "--gemini-dir",
            home_str.as_str(),
            "--app-data-dir",
            "antigravity-acp",
            "--login",
        ]
    );
    assert_eq!(
        opts.env.get("GEMINI_HOME").map(String::as_str),
        Some(home_str.as_str())
    );
    assert_eq!(
        opts.env
            .get("AGY_ACP_FORCE_FILE_STORAGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        opts.env
            .get("AGY_CLI_DISABLE_AUTO_UPDATE")
            .map(String::as_str),
        Some("true")
    );
    assert!(
        opts.env.contains_key("SSH_CONNECTION"),
        "login terminal must set SSH_CONNECTION to isolate agy's keyring"
    );
    assert!(gemini_home.is_dir());
}

/// Regression: the four provider cards sat in one row, so in the Settings dialog (about 600 px of content)
/// Antigravity was drawn past the form's edge, clipped, and could not be picked.
#[gpui::test]
fn every_provider_card_fits_inside_the_add_form(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let (_view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(None, AccountsConfig::default(), None, window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(900.)));
    cx.run_until_parked();
    let form = cx
        .debug_bounds("account-form")
        .expect("the add form is drawn");
    for pick in [
        "provider-pick-Claude",
        "provider-pick-Codex",
        "provider-pick-Cursor",
        "provider-pick-Antigravity",
    ] {
        let card = cx.debug_bounds(pick).expect("each provider card is drawn");
        assert!(
            card.right() <= form.right(),
            "{pick} ends at {:?}, past the form's edge at {:?}",
            card.right(),
            form.right()
        );
    }
}
