//! Mouse reporting: the bytes a program that turned on xterm mouse tracking (vim, htop) expects.

use alacritty_terminal::term::TermMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Motion with no button held.
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// The report for one mouse event at a 0-based viewport cell, or `None` when the program didn't ask for it.
/// SGR (mode 1006) when on, else the legacy encoding, which can't name cells past column or row 223 (1-based).
pub fn mouse_report(
    action: MouseAction,
    button: MouseButton,
    col: usize,
    row: usize,
    mods: MouseMods,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    if action == MouseAction::Motion {
        let held = button != MouseButton::None;
        if !(mode.contains(TermMode::MOUSE_MOTION) || (held && mode.contains(TermMode::MOUSE_DRAG)))
        {
            return None;
        }
    }
    let wheel = matches!(button, MouseButton::WheelUp | MouseButton::WheelDown);
    if wheel && action == MouseAction::Release {
        return None;
    }
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let mut code: u8 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    if action == MouseAction::Release && !sgr {
        code = 3;
    }
    if action == MouseAction::Motion {
        code += 32;
    }
    code += 4 * u8::from(mods.shift) + 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    let (x, y) = (col + 1, row + 1);
    if sgr {
        let end = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    let byte = |v: usize| u8::try_from(v + 32).ok();
    Some(vec![0x1b, b'[', b'M', 32 + code, byte(x)?, byte(y)?])
}
