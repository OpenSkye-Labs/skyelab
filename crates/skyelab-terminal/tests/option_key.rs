//! Option as meta (ESC + key, for shell word motions) or as the layout's composed character (German Option-L = `@`).

use alacritty_terminal::term::TermMode;
use gpui::{Keystroke, Modifiers};
use skyelab_terminal::{OptionKey, key_to_bytes};

/// `alt-<key>` as macOS reports it: `key` is the bare key, `key_char` what the layout composes with Option.
fn option(key: &str, composed: &str) -> Keystroke {
    Keystroke {
        modifiers: Modifiers {
            alt: true,
            ..Modifiers::default()
        },
        key: key.into(),
        key_char: Some(composed.into()),
    }
}

#[test]
fn option_key_meta_sends_escape_prefix() {
    let n = TermMode::NONE;
    assert_eq!(OptionKey::default(), OptionKey::Meta);
    let bytes = key_to_bytes(&option("b", "∫"), n, OptionKey::Meta);
    assert_eq!(bytes.as_deref(), Some(&b"\x1bb"[..]));
    let bytes = key_to_bytes(&option("l", "@"), n, OptionKey::Meta);
    assert_eq!(bytes.as_deref(), Some(&b"\x1bl"[..]));
}

#[test]
fn option_key_characters_sends_the_composed_char() {
    let n = TermMode::NONE;
    let bytes = key_to_bytes(&option("l", "@"), n, OptionKey::Characters);
    assert_eq!(bytes.as_deref(), Some("@".as_bytes()));
    let bytes = key_to_bytes(&option("5", "["), n, OptionKey::Characters);
    assert_eq!(bytes.as_deref(), Some("[".as_bytes()));
    // Keys with their own encoding keep it: alt-left is still a modified cursor key.
    let Ok(left) = Keystroke::parse("alt-left") else {
        panic!("bad keystroke")
    };
    let bytes = key_to_bytes(&left, n, OptionKey::Characters);
    assert_eq!(bytes.as_deref(), Some(&b"\x1b[1;3D"[..]));
}
