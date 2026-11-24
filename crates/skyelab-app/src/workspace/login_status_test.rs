//! Acceptance: each account card says whether that account is signed in, asked of its own CLI with its own config
//! dir (the fakes read a `fake-login` file there), and asks again once its Log in terminal ends.

use super::login_status::{LoginStatus, Status};
use super::threads_test::test_backend;
use super::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_core::Account;

fn claude(id: &str, dir: &std::path::Path) -> Account {
    Account {
        id: id.into(),
        label: id.to_uppercase(),
        provider: skyelab_core::Provider::Claude,
        config_dir: dir.to_owned(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

#[gpui::test]
fn account_cards_show_each_accounts_own_sign_in(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // the fake CLIs and the Log in terminal are real processes
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let (a, b) = (home.path().join("a"), home.path().join("b"));
    std::fs::create_dir_all(&a).expect("mkdir");
    std::fs::create_dir_all(&b).expect("mkdir");
    std::fs::write(a.join("fake-login"), "a@example.com\n").expect("sign a in");
    let mut config = AccountsConfig::default();
    config.add_account(claude("a", &a)).expect("add a");
    config.add_account(claude("b", &b)).expect("add b");
    let folder = home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(folder, None, config, None, test_backend(), window, cx);
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    cx.dispatch_action(OpenSettings);
    let login = cx.update(|_, cx| LoginStatus::shared(cx));
    let status = |id: &str, cx: &mut gpui::VisualTestContext| {
        let account = claude(id, if id == "a" { &a } else { &b });
        login.read_with(cx, |l, _| l.get(&account).cloned())
    };
    wait_until(cx, "both answers", |cx| {
        cx.run_until_parked();
        [status("a", cx), status("b", cx)]
            .iter()
            .all(|s| !matches!(s, None | Some(Status::Checking)))
    });
    assert_eq!(
        status("a", cx),
        Some(Status::SignedIn(Some("a@example.com".into())))
    );
    assert_eq!(
        status("b", cx),
        Some(Status::SignedOut),
        "b's own folder has no login"
    );
    let settings = ws
        .read_with(cx, |ws, _| ws.settings.clone())
        .expect("settings");
    settings.update_in(cx, |s, window, cx| {
        s.section = crate::settings::Section::Accounts;
        s.set_config(s.config.clone(), window, cx)
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("login-status-a").is_some(),
        "the card shows a's sign-in"
    );
    assert!(cx.debug_bounds("login-status-b").is_some());

    // Log in on b: the fake writes b's login and exits; once the terminal ends, b is asked again.
    let button = cx.debug_bounds("claude-login-b").expect("Log in is drawn");
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    wait_until(cx, "b signed in after its Log in", |cx| {
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        status("b", cx) == Some(Status::SignedIn(Some("fake@example.com".into())))
    });
}
