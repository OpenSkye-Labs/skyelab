//! The launch checks the folder's binding as accounts.toml says right before it spawns, not as the
//! view's copy of the config said when the prompt was sent.

use super::thread::ThreadView;
use super::thread_close_starting_test::{backend, rig, row, wait_until};
use super::*;
use gpui::TestAppContext;
use skyelab_core::Binding;

#[gpui::test]
fn a_rebind_saved_just_before_launch_is_refused(cx: &mut TestAppContext) {
    let r = rig(cx);
    let accounts = r.home.path().join("accounts.toml");
    let mut rebound = r.config.clone();
    rebound.remove_binding(&r.dir).expect("unbind");
    rebound
        .add_binding(Binding::new(r.dir.clone(), "me".into()))
        .expect("rebind");
    assert_eq!(rebound.resolve(&r.dir).map(|a| a.id.as_str()), Some("me"));
    rebound.save(&accounts).expect("save");
    cx.update(|cx| cx.set_global(AccountsFile(accounts.clone())));

    // The view still believes the folder is work's.
    let (config, backend, project) = (r.config.clone(), backend(&r), r.project.clone());
    let (view, cx) =
        cx.add_window_view(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    view.update_in(cx, |v, window, cx| v.submit_remote("hold", window, cx))
        .expect("prompt taken");
    wait_until(cx, "the launch to finish", |cx| {
        view.read_with(cx, |v, _| !v.starting)
    });
    let error = view
        .read_with(cx, |v, _| v.error.clone().map(|e| e.to_string()))
        .unwrap_or_default();
    assert!(
        view.read_with(cx, |v, _| v.session.is_none()),
        "no agent started"
    );
    assert!(error.contains("now bound to 'me'"), "{error}");
    assert_ne!(row(&r).map(|t| t.status).as_deref(), Some("running"));
}
