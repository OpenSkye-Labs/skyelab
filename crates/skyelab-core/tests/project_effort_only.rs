//! Criterion 2: with no project *model* override, the project's remembered model still beats the app default,
//! even when the project overrides only the effort (criterion 3: fields inherit one by one).

use skyelab_core::thread_defaults::ThreadDefaults;
use skyelab_core::{Provider, ThreadSettings};
use std::path::Path;

#[test]
fn effort_only_override_keeps_the_remembered_model() {
    let mut t = ThreadSettings {
        claude_model: Some("haiku".into()),
        ..Default::default()
    };
    t.projects.insert(
        "/work".into(),
        ThreadDefaults {
            claude_effort: Some("high".into()),
            ..Default::default()
        },
    );
    let (model, effort) = t.resolve_model(
        Path::new("/work"),
        Provider::Claude,
        (None, None),
        (Some("sonnet".into()), None),
    );
    assert_eq!(
        model.as_deref(),
        Some("sonnet"),
        "remembered model lost to the app default under an effort-only project override"
    );
    assert_eq!(effort.as_deref(), Some("high"), "the project's own effort");
}
