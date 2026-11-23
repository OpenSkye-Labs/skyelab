use alacritty_terminal::term::TermMode;
use skyelab_terminal::{MouseAction, MouseButton, MouseMods, mouse_report};

const NONE: MouseMods = MouseMods {
    shift: false,
    alt: false,
    ctrl: false,
};

fn report(a: MouseAction, b: MouseButton, mode: TermMode) -> Option<String> {
    mouse_report(a, b, 4, 2, NONE, mode).map(|v| String::from_utf8_lossy(&v).into_owned())
}

#[test]
fn sgr_press_release() {
    let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
    assert_eq!(
        report(MouseAction::Press, MouseButton::Left, mode).as_deref(),
        Some("\x1b[<0;5;3M")
    );
    assert_eq!(
        report(MouseAction::Release, MouseButton::Left, mode).as_deref(),
        Some("\x1b[<0;5;3m")
    );
    let ctrl = MouseMods { ctrl: true, ..NONE };
    assert_eq!(
        mouse_report(MouseAction::Press, MouseButton::Right, 0, 0, ctrl, mode),
        Some(b"\x1b[<18;1;1M".to_vec())
    );
}

#[test]
fn normal_encoding() {
    let mode = TermMode::MOUSE_REPORT_CLICK;
    assert_eq!(
        mouse_report(MouseAction::Press, MouseButton::Left, 4, 2, NONE, mode),
        Some(vec![0x1b, b'[', b'M', 32, 32 + 5, 32 + 3])
    );
    assert_eq!(
        mouse_report(MouseAction::Release, MouseButton::Left, 4, 2, NONE, mode),
        Some(vec![0x1b, b'[', b'M', 32 + 3, 32 + 5, 32 + 3])
    );
    // Column 223 (1-based) is the last one a byte can name; past it nothing is sent.
    assert_eq!(
        mouse_report(MouseAction::Press, MouseButton::Left, 222, 0, NONE, mode),
        Some(vec![0x1b, b'[', b'M', 32, 255, 33])
    );
    assert_eq!(
        mouse_report(MouseAction::Press, MouseButton::Left, 300, 2, NONE, mode),
        None
    );
}

#[test]
fn wheel_reports_buttons_64_65() {
    let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
    assert_eq!(
        report(MouseAction::Press, MouseButton::WheelUp, mode).as_deref(),
        Some("\x1b[<64;5;3M")
    );
    assert_eq!(
        report(MouseAction::Press, MouseButton::WheelDown, mode).as_deref(),
        Some("\x1b[<65;5;3M")
    );
}

#[test]
fn motion_needs_drag_or_motion_mode() {
    let click = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
    assert_eq!(report(MouseAction::Motion, MouseButton::Left, click), None);
    let drag = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
    assert_eq!(
        report(MouseAction::Motion, MouseButton::Left, drag).as_deref(),
        Some("\x1b[<32;5;3M")
    );
    assert_eq!(report(MouseAction::Motion, MouseButton::None, drag), None);
    let motion = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
    assert_eq!(
        report(MouseAction::Motion, MouseButton::None, motion).as_deref(),
        Some("\x1b[<35;5;3M")
    );
}

#[test]
fn no_report_without_mouse_mode() {
    assert_eq!(
        report(MouseAction::Press, MouseButton::Left, TermMode::SGR_MOUSE),
        None
    );
    assert_eq!(
        report(MouseAction::Press, MouseButton::Left, TermMode::empty()),
        None
    );
}
