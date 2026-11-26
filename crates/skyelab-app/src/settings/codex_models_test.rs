//! Acceptance: Settings → Chat's Codex model is a menu of what Codex lists (asked once, under the bound folder's own
//! account), keeps a saved custom id as an entry, and "Custom model id…" shows the text field without saving.

use super::super::*;
use crate::workspace::github::CodexBin;
use crate::workspace::thread_extras::prefs;
use gpui::TestAppContext;
use skyelab_core::{Account, Binding};
use std::path::Path;

#[gpui::test]
fn the_codex_model_menu_lists_what_codex_offers(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
        cx.set_global(CodexBin(fake));
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).expect("project dir");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "oai".into(),
            label: "OpenAI".into(),
            provider: Provider::Codex,
            config_dir: root.join(".codex-oai"),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add account");
    config
        .add_binding(Binding::new(project, "oai".into()))
        .expect("bind");
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut s = Settings::new(None, config, None, window, cx);
        s.show(Section::Chat, window, cx);
        s
    });
    crate::test_wait::wait_until(cx, "codex listed its models", |cx| {
        cx.update(|_, cx| {
            cx.try_global::<KnownCodexModels>()
                .is_some_and(|k| !k.0.is_empty())
        })
    });
    let labels = |cx: &mut gpui::VisualTestContext| {
        view.read_with(cx, |s, cx| {
            s.codex_options(cx)
                .into_iter()
                .map(|(_, l)| l)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        labels(cx),
        ["Default model", "GPT Fake", "Custom model id…"]
    );

    // A listed model saves; "Custom model id…" shows the field and saves nothing.
    view.update(cx, |s, cx| s.pick_codex_model(Some("gpt-fake".into()), cx));
    assert_eq!(
        cx.update(|_, cx| prefs::get(cx)).codex_model.as_deref(),
        Some("gpt-fake")
    );
    let custom = view.read_with(cx, |s, cx| {
        s.codex_options(cx).last().and_then(|(v, _)| v.clone())
    });
    view.update(cx, |s, cx| s.pick_codex_model(custom, cx));
    assert!(view.read_with(cx, |s, _| s.codex_custom), "the field shows");
    assert_eq!(
        cx.update(|_, cx| prefs::get(cx)).codex_model.as_deref(),
        Some("gpt-fake"),
        "CUSTOM_ENTRY_SAVED"
    );

    // A typed id that Codex doesn't list stays in the menu as itself.
    let input = view.read_with(cx, |s, _| s.codex_model_input.clone());
    input.update_in(cx, |i, window, cx| i.set_value("my-model", window, cx));
    view.update(cx, |s, cx| s.save_codex_model(cx));
    assert!(labels(cx).contains(&"my-model (custom)".to_owned()));
}
