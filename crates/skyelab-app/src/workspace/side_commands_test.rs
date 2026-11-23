//! Acceptance: `/side` and `/btw` (SkyeLab's own commands) lead the `/` menu for Claude and Codex threads alike, and
//! a side chat, where they do nothing, doesn't list them.

use super::composer_menu::command_items;
use skyelab_core::Provider;

fn labels(provider: Provider, side: bool) -> Vec<String> {
    let tmp = tempfile::tempdir().expect("temp dir");
    command_items(provider, &tmp.path().join("acct"), tmp.path(), side)
        .into_iter()
        .map(|i| i.label)
        .collect()
}

#[test]
fn side_commands_are_listed() {
    let claude = labels(Provider::Claude, true);
    assert_eq!(claude[..2], ["/side", "/btw"], "{claude:?}");
    assert!(claude.contains(&"/compact".to_owned()));
    assert_eq!(labels(Provider::Codex, true), ["/side", "/btw"]);
    for provider in [Provider::Claude, Provider::Codex] {
        assert!(
            !labels(provider, false)
                .iter()
                .any(|l| l == "/side" || l == "/btw"),
            "not in a side chat"
        );
    }
}
