//! Acceptance: SkyeLab's own commands (`/side`, and T3's `/model`, `/plan`, `/default`) lead the `/` menu for Claude
//! and Codex threads alike, and a side chat, where they do nothing, doesn't list them. The `/btw` alias works when typed
//! but isn't listed, so it never leads the menu.

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
    let ours = ["/side", "/model", "/plan", "/default"];
    let claude = labels(Provider::Claude, true);
    assert_eq!(claude[..4], ours, "{claude:?}");
    assert!(claude.contains(&"/compact".to_owned()));
    assert!(!claude.contains(&"/btw".to_owned()), "{claude:?}");
    assert_eq!(labels(Provider::Codex, true), ours);
    assert_eq!(labels(Provider::Cursor, true), ours);
    for provider in [Provider::Claude, Provider::Codex, Provider::Cursor] {
        assert!(
            !labels(provider, false)
                .iter()
                .any(|l| ours.contains(&l.as_str())),
            "not in a side chat"
        );
    }
}
