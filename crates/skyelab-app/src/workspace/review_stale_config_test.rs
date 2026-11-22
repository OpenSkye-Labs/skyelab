use super::threads_test::test_backend;
use super::*;
use gpui::TestAppContext;
use skyelab_core::{Account, Binding};

/// A broken accounts.toml must bind nothing: not the status bar, not terminals, not threads.
#[gpui::test]
fn a_broken_accounts_file_launches_nothing_under_the_old_binding(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).expect("dir");
    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: root.join(".claude-acme"),
        })
        .expect("account");
    config
        .add_binding(Binding::new(dir.clone(), "acme".into()))
        .expect("bind");
    config.save(&accounts_path).expect("save");
    let (folder, path) = (dir.clone(), accounts_path.clone());
    let (ws, cx) = cx.add_window_view(move |window, cx| {
        Workspace::new(folder, Some(path), config, None, test_backend(), window, cx)
    });
    std::fs::write(&accounts_path, "not toml [").expect("break the file");
    ws.update(cx, |ws, _| ws.reload_accounts());
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.account, None);
        assert!(
            terminal_env(&ws.config, &dir).expect("env").is_empty(),
            "STALE_BINDING_STILL_LAUNCHES"
        );
    });
}
