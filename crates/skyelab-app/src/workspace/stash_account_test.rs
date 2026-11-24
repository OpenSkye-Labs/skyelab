//! A restored stash brings everything back, as T3's does, whatever account it was stashed under: its text and its
//! unbound chips (a quote, terminal output, diff comments).

use super::composer_context::ChipKind;
use super::composer_review_test::view_with;
use super::composer_stash::StashPrompt;
use super::thread::ThreadView;
use super::threads_test::{test_backend, wait_until};
use gpui::AppContext as _;
use skyelab_core::Binding;

#[gpui::test]
fn stashed_quote_comes_back_in_another_accounts_thread(cx: &mut gpui::TestAppContext) {
    let (home, _dir, mut config, acme, cx) = view_with(cx, None);
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let other = root.join("other");
    std::fs::create_dir_all(&other).expect("dir");
    config
        .add_binding(Binding::new(other.clone(), "beta".into()))
        .expect("bind beta");
    let store = acme.read_with(cx, |t, _| t.backend.store.clone());
    let project = (store.lock().expect("lock"))
        .create_project("other", &other)
        .expect("project");

    acme.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value("ask", window, cx));
        t.push_context(
            ChipKind::Quote,
            "acme".into(),
            "ACME_ONLY_TEXT".into(),
            None,
            cx,
        );
        t.stash_prompt(&StashPrompt, window, cx);
    });
    let stored = || {
        (store.lock().expect("lock"))
            .drafts_with_prefix("stash:")
            .expect("list")
    };
    wait_until(cx, "stashed", |_| stored().len() == 1);
    let owner = stored()[0].owner.clone();

    let mut backend = test_backend();
    backend.store = store.clone();
    let beta = cx
        .update(|window, app| app.new(|cx| ThreadView::new(&project, config, backend, window, cx)));
    cx.run_until_parked();
    beta.update_in(cx, |t, window, cx| t.restore_stash(owner, window, cx));
    wait_until(cx, "restored", |_| stored().is_empty());
    cx.run_until_parked();
    let (text, sent) = beta.read_with(cx, |t, cx| {
        (
            t.prompt.read(cx).value().to_string(),
            t.with_contexts("go", false),
        )
    });
    assert_eq!(text, "ask", "the typed text comes back");
    assert!(
        sent.contains("ACME_ONLY_TEXT"),
        "the quote comes back: {sent:?}"
    );
    assert_eq!(
        beta.read_with(cx, |t, _| t.composer.notice.clone()),
        None,
        "nothing was left out"
    );
}
