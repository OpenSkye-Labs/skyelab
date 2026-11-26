//! Before the first read lands, the Changes tab says it is loading: never "Nothing to review" or a "Nothing to
//! commit" button for a folder with changes (QA: 2 changed files read as none for about 2 s).

use super::changes::ChangesView;
use super::changes_model_test::repo;
use gpui::TestAppContext;

#[gpui::test]
fn loading_is_never_an_empty_state(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let (_tmp, root) = repo(&[("a.rs", b"fn a() {}\n")]);
    std::fs::write(root.join("a.rs"), "fn b() {}\n").expect("write");
    let (view, cx) = cx
        .add_window_view(|window, cx| ChangesView::new(root, None, "app".into(), None, window, cx));
    cx.run_until_parked();
    // The state before the read lands (it runs in the background).
    view.update(cx, |v, cx| {
        assert!(v.changes.is_some(), "the read landed");
        v.changes = None;
        cx.notify()
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("changes-loading").is_some(), "says loading");
    assert!(cx.debug_bounds("changes-empty").is_none(), "no empty state");

    view.update(cx, |v, cx| v.refresh(cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("changes-loading").is_none(), "loaded");
    assert!(cx.debug_bounds("changes-empty").is_none(), "one file changed");
}
