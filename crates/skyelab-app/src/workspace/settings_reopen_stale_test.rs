//! Settings is a view kept for the whole session: reopening it must not save a stale copy of accounts.toml over an
//! edit made to the file meanwhile.

use crate::workspace::threads_test::{fixture, open_workspace, setup};
use crate::workspace::*;
use gpui::TestAppContext;
use skyelab_core::Binding;

#[gpui::test]
fn reopened_settings_does_not_clobber_an_outside_edit(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (_ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    _ws.update_in(cx, |ws, window, cx| ws.close_settings(window, cx));
    cx.run_until_parked();

    // Outside edit while Settings isn't shown.
    let other = f.project_dir.parent().expect("root").join("other");
    std::fs::create_dir_all(&other).expect("mkdir");
    let mut edited = AccountsConfig::load(&f.accounts_path).expect("load");
    edited
        .add_binding(Binding::new(other.clone(), "acme".into()))
        .expect("bind");
    edited.save(&f.accounts_path).expect("save");

    // Reopen Settings and make an unrelated edit there.
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    cx.dispatch_action(crate::settings::RemoveBinding {
        path: f.project_dir.clone(),
    });
    cx.run_until_parked();

    let saved = AccountsConfig::load(&f.accounts_path).expect("load");
    assert!(
        saved.resolve(&f.project_dir).is_none(),
        "the Settings edit reached disk"
    );
    assert!(
        saved.resolve(&other).is_some(),
        "STALE SETTINGS CLOBBERED the outside binding"
    );
}

#[gpui::test]
fn an_account_edit_keeps_the_files_ssh_agent(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let mut on = AccountsConfig::load(&f.accounts_path).expect("load");
    let mut acme = on.accounts[0].clone();
    acme.ssh_agent = true;
    on.update_account(acme).expect("ssh on");
    on.save(&f.accounts_path).expect("save");
    let (_ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    cx.dispatch_action(crate::settings::EditAccount { id: "acme".into() });
    cx.run_until_parked();

    // Revoked in the file while the form is open.
    let mut off = AccountsConfig::load(&f.accounts_path).expect("load");
    let mut acme = off.accounts[0].clone();
    acme.ssh_agent = false;
    off.update_account(acme).expect("ssh off");
    off.save(&f.accounts_path).expect("save");

    cx.dispatch_action(crate::settings::SaveAccount);
    cx.run_until_parked();
    let saved = AccountsConfig::load(&f.accounts_path).expect("load");
    assert!(
        !saved.accounts[0].ssh_agent,
        "SSH_AGENT_RESTORED by a stale form"
    );
}
