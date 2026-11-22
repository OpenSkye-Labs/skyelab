use super::threads_test::test_backend;
use super::*;
use crate::settings::RemoveBinding;
use gpui::TestAppContext;
use skyelab_core::{Account, Binding};

#[gpui::test]
fn status_follows_an_unbind_made_in_settings(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let accounts_path = home.path().join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme Team".into(),
            config_dir: home.path().join(".claude-acme"),
        })
        .expect("add account");
    config
        .add_binding(Binding::new(folder.clone(), "acme".into()))
        .expect("bind");
    config.save(&accounts_path).expect("save");

    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            folder.clone(),
            Some(accounts_path.clone()),
            config,
            None,
            test_backend(),
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.account.as_deref(), Some("Acme Team"))
    });

    cx.simulate_keystrokes(OPEN_SETTINGS_KEY);
    cx.run_until_parked();
    cx.dispatch_action(RemoveBinding {
        path: folder.clone(),
    });
    cx.run_until_parked();

    let on_disk = AccountsConfig::load(&accounts_path).expect("load");
    assert!(
        on_disk.resolve(&folder).is_none(),
        "precondition: unbind was saved"
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.account, None,
            "STALE_STATUS: status still shows an account after unbinding"
        )
    });
}
