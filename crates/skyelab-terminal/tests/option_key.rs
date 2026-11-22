use alacritty_terminal::term::TermMode;
use gpui::{Keystroke, Modifiers};
use skyelab_terminal::{OptionKey, OptionKeys, key_to_bytes};

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
    let Ok(left) = Keystroke::parse("alt-left") else {
        panic!("bad keystroke")
    };
    let bytes = key_to_bytes(&left, n, OptionKey::Characters);
    assert_eq!(bytes.as_deref(), Some(&b"\x1b[1;3D"[..]));
}

/// macOS device-dependent modifier bits: left Option 0x20, right Option 0x40.
const LEFT: usize = 0x20;
const RIGHT: usize = 0x40;

#[test]
fn option_keys_default_left_meta_right_characters() {
    let keys = OptionKeys::default();
    assert_eq!(keys.resolve(LEFT), OptionKey::Meta);
    assert_eq!(keys.resolve(RIGHT), OptionKey::Characters);
    assert_eq!(
        keys.resolve(LEFT | RIGHT),
        OptionKey::Meta,
        "both held: left wins"
    );
    assert_eq!(
        keys.resolve(0),
        OptionKey::Meta,
        "side unknown (not macOS): left"
    );

    let n = TermMode::NONE;
    let right = keys.resolve(RIGHT);
    assert_eq!(
        key_to_bytes(&option("l", "@"), n, right).as_deref(),
        Some("@".as_bytes())
    );
    let left = keys.resolve(LEFT);
    assert_eq!(
        key_to_bytes(&option("b", "∫"), n, left).as_deref(),
        Some(&b"\x1bb"[..])
    );

    let swapped = OptionKeys {
        left: OptionKey::Characters,
        right: OptionKey::Meta,
    };
    assert_eq!(swapped.resolve(LEFT), OptionKey::Characters);
    assert_eq!(swapped.resolve(RIGHT), OptionKey::Meta);
}
