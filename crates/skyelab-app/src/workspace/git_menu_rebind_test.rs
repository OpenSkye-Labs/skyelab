//! The details card's Commit & push page says who it pushes as only while the folder still resolves to the thread's
//! account in accounts.toml as saved now; after a rebind it shows the refusal Push itself gives.

use super::publish_test::thread_and_changes;
use super::worktree_thread_test::{fixture, wait_until};
use super::*;
use gpui::TestAppContext;
use skyelab_core::Binding;

#[gpui::test]
fn the_git_menu_refuses_after_the_file_rebinds_the_folder(cx: &mut TestAppContext) {
    let f = fixture();
    let (thread, _changes, cx) = thread_and_changes(&f, cx);
    let accounts = f.project.parent().expect("parent").join("accounts.toml");
    let mut rebound = f.config.clone();
    rebound.remove_binding(&f.project).expect("unbind");
    rebound
        .add_binding(Binding::new(f.project.clone(), "other".into()))
        .expect("rebind");
    rebound.save(&accounts).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(accounts)));

    thread.update(cx, |t, cx| t.open_git_menu(cx));
    wait_until(cx, "the login line answered", |cx| {
        thread.read_with(cx, |t, _| t.details.gh_login.is_some())
    });
    let line = thread
        .read_with(cx, |t, _| t.details.gh_login.clone())
        .expect("answered");
    assert!(
        line.contains("is bound to other, not this thread's account"),
        "Push's refusal, not a login: {line}"
    );
}
