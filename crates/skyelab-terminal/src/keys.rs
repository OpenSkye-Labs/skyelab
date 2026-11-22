use std::borrow::Cow;

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

/// The bytes a key press sends to the PTY, or `None` when the terminal doesn't handle it
/// (cmd/super combos are left to app keybindings such as paste).
pub fn key_to_bytes(key: &Keystroke, mode: TermMode) -> Option<Cow<'static, [u8]>> {
    let m = key.modifiers;
    if m.platform || m.function {
        return None;
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    // xterm encodes modified cursor keys as CSI 1;<1 + shift + 2·alt + 4·ctrl><final>.
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.control);
    let cursor_final = match key.key.as_str() {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let (Some(f), true) = (cursor_final, modifier > 1) {
        return Some(Cow::Owned(format!("\x1b[1;{modifier}{f}").into_bytes()));
    }
    let fixed: Option<&'static [u8]> = match (key.key.as_str(), m.shift) {
        ("enter", _) => Some(b"\r"),
        ("backspace", _) if m.control => Some(b"\x08"),
        ("backspace", _) => Some(b"\x7f"),
        ("tab", true) => Some(b"\x1b[Z"),
        ("tab", false) => Some(b"\t"),
        ("escape", _) => Some(b"\x1b"),
        ("up", _) if app_cursor => Some(b"\x1bOA"),
        ("down", _) if app_cursor => Some(b"\x1bOB"),
        ("right", _) if app_cursor => Some(b"\x1bOC"),
        ("left", _) if app_cursor => Some(b"\x1bOD"),
        ("home", _) if app_cursor => Some(b"\x1bOH"),
        ("end", _) if app_cursor => Some(b"\x1bOF"),
        ("up", _) => Some(b"\x1b[A"),
        ("down", _) => Some(b"\x1b[B"),
        ("right", _) => Some(b"\x1b[C"),
        ("left", _) => Some(b"\x1b[D"),
        ("home", _) => Some(b"\x1b[H"),
        ("end", _) => Some(b"\x1b[F"),
        ("pageup", _) => Some(b"\x1b[5~"),
        ("pagedown", _) => Some(b"\x1b[6~"),
        ("insert", _) => Some(b"\x1b[2~"),
        ("delete", _) => Some(b"\x1b[3~"),
        ("space", _) if m.control => Some(b"\x00"),
        _ => None,
    };
    if let Some(bytes) = fixed {
        return Some(alt_prefixed(m.alt && !matches!(bytes[0], 0x1b), bytes));
    }
    if m.control {
        // ctrl-a..z → 0x01..0x1a, plus the punctuation that has a control code.
        let [c] = key.key.as_bytes() else { return None };
        let code = match c {
            b'a'..=b'z' => c - b'a' + 1,
            b'@' | b'2' => 0,
            b'[' | b'3' => 0x1b,
            b'\\' | b'4' => 0x1c,
            b']' | b'5' => 0x1d,
            b'^' | b'6' => 0x1e,
            b'_' | b'-' | b'7' => 0x1f,
            _ => return None,
        };
        return Some(alt_prefixed(m.alt, &[code]));
    }
    if m.alt {
        // Option sends meta (ESC + key), the way shells expect for word motions.
        let text = if m.shift {
            key.key.to_uppercase()
        } else {
            key.key.clone()
        };
        return (text.chars().count() == 1).then(|| alt_prefixed(true, text.as_bytes()));
    }
    let text = match key.key.as_str() {
        "space" => " ".to_string(),
        // Real key events carry `key_char`; a bare single-char key (as from `Keystroke::parse`) is its own text.
        k => key
            .key_char
            .clone()
            .or_else(|| (k.chars().count() == 1).then(|| k.to_string()))?,
    };
    Some(Cow::Owned(text.into_bytes()))
}

fn alt_prefixed(alt: bool, bytes: &[u8]) -> Cow<'static, [u8]> {
    if alt {
        Cow::Owned([b"\x1b", bytes].concat())
    } else {
        Cow::Owned(bytes.to_vec())
    }
}

/// The bytes a paste sends. Newlines become CR, as typed. In bracketed mode every ESC and C1 CSI is
/// dropped so pasted text can't end the bracket early and run as typed commands.
pub(crate) fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if !bracketed {
        return text.into_bytes();
    }
    let body: String = text
        .chars()
        .filter(|c| !matches!(c, '\x1b' | '\u{9b}'))
        .collect();
    format!("\x1b[200~{body}\x1b[201~").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::paste_bytes;

    #[test]
    fn bracketed_paste_cannot_close_early() {
        let out = paste_bytes("\x1b[20\x1b[201~1~rm -rf ~\n", true);
        let out = String::from_utf8(out).expect("utf8");
        assert_eq!(out.matches("\x1b[201~").count(), 1, "{out:?}");
        assert!(out.ends_with("\x1b[201~"));
        assert!(out.starts_with("\x1b[200~"));
        assert_eq!(out.matches('\x1b').count(), 2, "{out:?}");
    }

    #[test]
    fn plain_paste_maps_newlines() {
        assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc");
    }
}
