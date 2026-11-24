//! Acceptance: a Claude account's card offers Log in, which asks for a `claude auth login` terminal with that
//! account's launch env only (its CLAUDE_CONFIG_DIR, and its GH_CONFIG_DIR when set).

use super::*;
use gpui::TestAppContext;
use std::cell::RefCell;
use std::collections::HashMap;

fn claude_account(dir: &std::path::Path, gh: Option<PathBuf>) -> Account {
    Account {
        id: "acme".into(),
        label: "Acme".into(),
        provider: Provider::Claude,
        config_dir: dir.to_owned(),
        gh_config_dir: gh,
        ssh_agent: false,
    }
}

#[test]
fn claude_login_runs_auth_login_with_the_accounts_launch_env() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("claude-acme");
    let gh = tmp.path().join("gh-acme");
    let account = claude_account(&dir, Some(gh.clone()));
    use crate::workspace::github::{agent_login_options, login_args};
    let opts = agent_login_options(
        Path::new("/opt/claude"),
        login_args(account.provider),
        &account,
    )
    .expect("options");
    assert_eq!(
        opts.command,
        Some(("/opt/claude".into(), vec!["auth".into(), "login".into()]))
    );
    let utf8 = |p: &Path| p.to_str().expect("utf-8").to_string();
    assert_eq!(
        opts.env,
        HashMap::from([
            ("CLAUDE_CONFIG_DIR".into(), utf8(&dir)),
            ("GH_CONFIG_DIR".into(), utf8(&gh)),
        ])
    );
    let launch: HashMap<String, String> = account
        .launch_env()
        .expect("env")
        .into_iter()
        .map(|(k, v)| (k.to_string(), utf8(&v)))
        .collect();
    assert_eq!(opts.env, launch, "exactly the account's launch env");
    assert!(dir.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&dir).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}

#[gpui::test]
fn a_claude_card_offers_log_in_for_that_account(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let mut config = AccountsConfig::default();
    config
        .add_account(claude_account(Path::new("/home/me/.claude-acme"), None))
        .expect("add");
    let (view, cx) = cx.add_window_view(|window, cx| Settings::new(None, config, None, window, cx));
    let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
    let seen = asked.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &SettingsEvent, _| {
            if let SettingsEvent::AgentLogin { id } = e {
                seen.borrow_mut().push(id.clone());
            }
        })
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("claude-login-acme").is_some(),
        "a Claude row offers Log in"
    );
    assert!(cx.debug_bounds("codex-login-acme").is_none());
    let button = cx.debug_bounds("claude-login-acme").expect("drawn");
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*asked.borrow(), vec!["acme".to_string()]);

    view.update(cx, |v, cx| v.log_in("nope", cx));
    cx.run_until_parked();
    assert_eq!(asked.borrow().len(), 1, "an unknown account asks nothing");
    view.read_with(cx, |v, _| {
        assert!(
            v.error
                .as_deref()
                .is_some_and(|e| e.contains("no account 'nope'"))
        )
    });
}
