//! The composer's model chip names the model that runs, never "Default model": the init line's model once a run
//! reported one (also after reopening), the account's own default before that, and Codex's listed default.

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

#[gpui::test]
fn effective_model_comes_from_the_init_line_and_survives_a_reopen(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin.clone(), None);
    cx.run_until_parked();
    assert_eq!(chip(&one.view, cx), ("Claude default".into(), true));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("turn", window, cx))
        .expect("started");
    wait_until(cx, "the init line's model", |cx| {
        chip(&one.view, cx) == ("Opus 5.5".into(), true)
    });
    // An explicit pick of the same family keeps the reported version; effort follows T3's "Opus 5.5 High".
    one.view.update(cx, |v, cx| {
        v.pick_model(
            ModelChoice {
                model: Some("opus".into()),
                effort: Some("high".into()),
            },
            cx,
        )
    });
    assert_eq!(chip(&one.view, cx), ("Opus 5.5 High".into(), false));
    cx.run_until_parked(); // each pick is saved by its own background task
    one.view.update(cx, |v, cx| {
        v.pick_model(
            ModelChoice {
                model: Some("sonnet".into()),
                effort: None,
            },
            cx,
        )
    });
    assert_eq!(chip(&one.view, cx), ("Sonnet".into(), false));
    cx.run_until_parked();
    // Back to the default: the old pick's run no longer names it, the account's own default does.
    one.view
        .update(cx, |v, cx| v.pick_model(ModelChoice::default(), cx));
    assert_eq!(chip(&one.view, cx), ("Claude default".into(), true));
    cx.run_until_parked();
    // Reopened from the store: the replayed init line names it again.
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
    wait_until(cx, "the replayed model", |cx| {
        chip(&reopened, cx) == ("Opus 5.5".into(), true)
    });
}

#[gpui::test]
fn before_any_run_the_account_settings_model_shows(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin.clone(), None);
    let config = one.view.read_with(cx, |v, _| v.config.clone());
    let dir = config.accounts[0].config_dir.clone();
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("settings.json"), r#"{"model":"opus"}"#).expect("write");
    let project = {
        let s = one.store.lock().expect("lock");
        s.list_projects().expect("projects").remove(0)
    };
    let store = one.store.clone();
    let fresh = cx.update(|window, cx| {
        cx.new(|cx| {
            let backend = Backend {
                store,
                claude_bin: bin,
            };
            ThreadView::new(&project, config, backend, window, cx)
        })
    });
    wait_until(cx, "the account's model", |cx| {
        chip(&fresh, cx) == ("Opus".into(), true)
    });
}

#[gpui::test]
fn codex_shows_its_listed_default(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, None);
    wait_until(cx, "Codex's default model", |cx| {
        chip(&one.view, cx) == ("GPT Fake".into(), true)
    });
}
