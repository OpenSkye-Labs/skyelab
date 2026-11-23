//! Settings is a page kept for the whole session: reopening it must not save a stale copy of accounts.toml over an
//! edit made to the file meanwhile.
#![cfg(unix)]

use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::TestAppContext;
use skyelab_core::Binding;

#[gpui::test]
fn reopened_settings_does_not_clobber_an_outside_edit(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (_ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    cx.dispatch_action(GoBack);
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
