//! Acceptance: the Quit binding survives a keybindings file reload. The reload puts back only the bindings made
//! before the keymap took its snapshot of the defaults, so the app's own keys must all be bound before it.

use super::*;

#[gpui::test]
fn quit_survives_a_reload(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("keybindings.toml");
    std::fs::write(&path, "").expect("write");
    let quit = |cx: &mut gpui::TestAppContext| {
        cx.update(|cx| {
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&Quit)
                .count()
        })
    };
    cx.update(|cx| init_keys(Some(path.clone()), cx));
    let before = quit(cx);
    assert_eq!(before > 0, cfg!(target_os = "macos"), "⌘Q is a macOS key");

    std::fs::write(&path, "# edited\n").expect("rewrite");
    cx.executor().advance_clock(keymap::POLL * 2);
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| cx.global::<keymap::Keymap>().generation),
        2,
        "the file was read again"
    );
    assert_eq!(quit(cx), before, "Quit is still bound after the reload");
}
