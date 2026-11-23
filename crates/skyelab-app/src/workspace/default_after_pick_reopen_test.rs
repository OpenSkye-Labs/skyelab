//! A run made under an explicit pick must not become the "Account default" after the pick is cleared and the thread
//! is reopened, and an alias pick names a version only when a run on that alias reported it.

use super::Backend;
use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread::ThreadView;
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use skyelab_core::Provider;
use skyelab_store::ModelChoice;
use std::path::Path;

fn chip(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> (String, bool) {
    view.read_with(cx, |v, _| v.chip_label(false))
}

fn pick(model: Option<&str>) -> ModelChoice {
    ModelChoice {
        model: model.map(Into::into),
        effort: None,
    }
}

#[gpui::test]
fn a_picked_runs_model_is_not_the_account_default_after_reopen(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin.clone(), None);
    let dir = one
        .view
        .read_with(cx, |v, _| v.config.accounts[0].config_dir.clone());
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("settings.json"), r#"{"model":"sonnet"}"#).expect("write");
    one.view.update(cx, |v, cx| v.load_account_model(cx));
    wait_until(cx, "the account's sonnet default", |cx| {
        chip(&one.view, cx) == ("Sonnet".into(), true)
    });
    // Run once under an explicit Opus pick (the init line reports claude-opus-5-5).
    one.view
        .update(cx, |v, cx| v.pick_model(pick(Some("opus")), cx));
    cx.run_until_parked();
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("turn", window, cx))
        .expect("started");
    wait_until(cx, "the picked run's model", |cx| {
        chip(&one.view, cx) == ("Opus 5.5".into(), false)
    });
    // Back to the account default: Sonnet runs next.
    one.view.update(cx, |v, cx| v.pick_model(pick(None), cx));
    assert_eq!(chip(&one.view, cx), ("Sonnet".into(), true));
    // A Haiku pick never ran: no version is borrowed from the Opus run.
    one.view
        .update(cx, |v, cx| v.pick_model(pick(Some("haiku")), cx));
    assert_eq!(chip(&one.view, cx), ("Haiku".into(), false));
    one.view.update(cx, |v, cx| v.pick_model(pick(None), cx));
    cx.run_until_parked();
    let (thread, config) = one.view.read_with(cx, |v, _| {
        (v.thread.clone().expect("thread"), v.config.clone())
    });
    let store = one.store.clone();
    let reopened = cx.update(|window, cx| {
        cx.new(|cx| {
            let backend = Backend {
                store,
                claude_bin: bin,
            };
            ThreadView::open(thread, config, backend, window, cx)
        })
    });
    wait_until(cx, "the replay and the account read", |cx| {
        reopened.read_with(cx, |v, _| {
            v.composer.account_model.is_some() && v.composer.reported_model.is_some()
        })
    });
    cx.run_until_parked();
    assert_eq!(chip(&reopened, cx), ("Sonnet".into(), true));
}
