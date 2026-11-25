//! The model picker's keyboard (T3's): its search takes the focus and narrows the list, ↑/↓ and Enter pick,
//! ⌘N picks the Nth row, Esc closes, and an effort pick keeps the model. It lists only the bound account's models.

use super::claude_folded_turns_test::one_view;
use super::thread::ThreadView;
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_core::Provider;
use std::path::Path;

fn labels(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |v, cx| {
        v.picker_rows(cx).into_iter().map(|(_, l)| l).collect()
    })
}

fn open(view: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |v, window, cx| v.toggle_picker(window, cx));
    cx.run_until_parked();
}

fn picked(
    view: &Entity<ThreadView>,
    cx: &mut VisualTestContext,
) -> (Option<String>, Option<String>) {
    view.read_with(cx, |v, _| {
        (
            v.composer.pick.model.clone(),
            v.composer.pick.effort.clone(),
        )
    })
}

fn is_open(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |v, _| v.composer.picker.is_some())
}

#[gpui::test]
fn picker_keys_search_pick_and_close(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    cx.run_until_parked();
    let view = one.view.clone();

    // Only the bound Claude account's models, its default first: nothing from another provider or account.
    open(&view, cx);
    assert_eq!(
        labels(&view, cx),
        [
            "Account default (Claude default)",
            "Opus",
            "Sonnet",
            "Haiku",
            "Fable"
        ]
    );

    // The search has the focus: typing narrows the list, Enter picks the highlighted row and closes.
    cx.simulate_input("son");
    assert_eq!(labels(&view, cx), ["Sonnet"]);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(picked(&view, cx), (Some("sonnet".into()), None));
    assert!(!is_open(&view, cx));

    // Reopened, the highlight starts on the current pick; ↓ moves it to the next row.
    open(&view, cx);
    assert_eq!(
        view.read_with(cx, |v, _| v.composer.picker.as_ref().map(|p| p.hi)),
        Some(2)
    );
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(picked(&view, cx).0.as_deref(), Some("haiku"));

    // ⌘2 (Ctrl+2 elsewhere) picks the second row: Opus.
    open(&view, cx);
    cx.simulate_keystrokes("secondary-2");
    cx.run_until_parked();
    assert_eq!(picked(&view, cx).0.as_deref(), Some("opus"));
    assert!(!is_open(&view, cx));

    // An effort keeps the model; Esc closes without a pick.
    open(&view, cx);
    view.update(cx, |v, cx| v.pick_effort(Some("high".into()), cx));
    cx.run_until_parked();
    assert_eq!(
        picked(&view, cx),
        (Some("opus".into()), Some("high".into()))
    );
    open(&view, cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!is_open(&view, cx));
    assert_eq!(
        picked(&view, cx),
        (Some("opus".into()), Some("high".into()))
    );
}

fn wait_listed(view: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    super::claude_folded_turns_test::wait_until(cx, "Codex's model list", |cx| {
        view.read_with(
            cx,
            |v, _| matches!(&v.composer.codex_models, Some(Ok(l)) if !l.is_empty()),
        )
    });
}

/// A Codex picker opened before its model list arrives still highlights the current pick once it does, so Enter
/// keeps it rather than switching to the account default.
#[gpui::test]
fn codex_picker_highlights_the_current_pick_once_listed(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, None);
    let view = one.view.clone();
    wait_listed(&view, cx);
    // An account whose config.toml sets a model lists nothing until the picker opens.
    view.update(cx, |v, _| {
        v.composer.codex_models = None;
        v.composer.pick.model = Some("gpt-fake".into());
    });
    view.update_in(cx, |v, window, cx| v.toggle_picker(window, cx));
    wait_listed(&view, cx);
    let rows: Vec<Option<String>> = view.read_with(cx, |v, cx| {
        v.picker_rows(cx).into_iter().map(|(id, _)| id).collect()
    });
    assert_eq!(rows, [None, Some("gpt-fake".to_owned())]);
    assert_eq!(
        view.read_with(cx, |v, _| v.composer.picker.as_ref().map(|p| p.hi)),
        Some(1)
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(picked(&view, cx).0.as_deref(), Some("gpt-fake"));
}
