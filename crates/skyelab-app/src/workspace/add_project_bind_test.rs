//! Acceptance: adding a folder asks which account it runs on unless it already resolves, and only a pick binds.

use super::add_project_rig::{choose, dialog_open, rig, shown, type_in};
use gpui::TestAppContext;

#[gpui::test]
fn an_unbound_folder_is_bound_only_to_the_picked_account(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    let app = rig.home.join("code/app");
    rig.open_add(cx);
    type_in("~/code/app", cx);
    choose("Add ~/code/app", cx);
    let s = shown(cx);
    let titles: Vec<_> = s.rows.iter().map(|r| r.1.as_str()).collect();
    assert_eq!(titles, ["WORK", "HOME", "Add account…", "Leave unbound"]);
    assert_eq!(rig.bound_to(&app), None, "the page alone binds nothing");
    choose("HOME", cx);
    rig.wait_project(&app, cx);
    assert!(!dialog_open(cx));
    assert_eq!(rig.bound_to(&app).as_deref(), Some("home"));
}

#[gpui::test]
fn leave_unbound_adds_the_project_and_binds_nothing(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    let api = rig.home.join("code/api");
    rig.open_add(cx);
    type_in("~/code/api", cx);
    choose("Add ~/code/api", cx);
    choose("Leave unbound", cx);
    rig.wait_project(&api, cx);
    assert_eq!(rig.bound_to(&api), None);
    assert_eq!(rig.saved().bindings.len(), 1, "only the rig's own binding");
}

#[gpui::test]
fn a_folder_that_resolves_is_added_on_its_account_without_asking(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    let inner = rig.home.join("bound/inner");
    rig.open_add(cx);
    type_in("~/bound/inner", cx);
    let s = shown(cx);
    assert_eq!(s.rows[0].2, "Runs on WORK, the account its folder is bound to");
    choose("Add ~/bound/inner", cx);
    rig.wait_project(&inner, cx);
    assert!(!dialog_open(cx), "no account question");
    assert_eq!(rig.saved().bindings.len(), 1, "no new binding: the parent's applies");
}
