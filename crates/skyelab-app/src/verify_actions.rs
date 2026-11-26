//! Debug builds only: `SKYELAB_VERIFY_ACTIONS` drives the app for `/loop-verify-ui`, which can't send input to a
//! topmost window.

use gpui::App;

/// Long enough for each entry's result (a page loading, a tab switch) to be drawn.
const VERIFY_ACTION_GAP: std::time::Duration = std::time::Duration::from_secs(2);

/// Debug builds only: `SKYELAB_VERIFY_ACTIONS=skyelab::NewTerminal,keys:h i cmd-enter,..` runs each
/// entry in turn, [`VERIFY_ACTION_GAP`] apart so async work (store loads, page loads, new tabs) lands in between, because
/// `/loop-verify-ui` can't send input to a topmost window. `keys:` types keystrokes into the focus,
/// `click:x;y` clicks at a window point (entries are comma-separated, hence the `;`), `hover:x;y` only moves the mouse
/// there (hover cards), `drag:x;y;x2;y2` drags with the left button, and `scroll:x;y;lines` scrolls the wheel there
/// (positive: up), `wait` sends nothing (one more gap). `window:N` sends the entries after it to the Nth workspace window (0 is the first, in open order).
pub(crate) fn dispatch_verify_actions(cx: &mut App) {
    if !cfg!(debug_assertions) {
        return;
    }
    let entries = std::env::var("SKYELAB_VERIFY_ACTIONS").ok();
    // Lines appended to this file later run the same way, so a live check can follow a real agent's timing.
    let file = std::env::var_os("SKYELAB_VERIFY_ACTIONS_FILE").map(std::path::PathBuf::from);
    if entries.is_none() && file.is_none() {
        return;
    }
    cx.spawn(async move |cx| {
        let mut queue: std::collections::VecDeque<String> = entries
            .iter()
            .flat_map(|e| e.split(','))
            .map(str::to_owned)
            .collect();
        let (mut target, mut seen) = (0, 0);
        loop {
            let Some(entry) = queue.pop_front() else {
                let Some(file) = &file else { break };
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                // Off the main thread: a read that stalls (a busy external disk) must not freeze the app.
                let path = file.clone();
                let text = cx
                    .background_executor()
                    .spawn(async move { std::fs::read_to_string(path).unwrap_or_default() })
                    .await;
                let lines: Vec<&str> = text.lines().collect();
                if lines.len() > seen {
                    queue.extend(
                        lines[seen..]
                            .iter()
                            .flat_map(|l| l.split(','))
                            .map(str::to_owned),
                    );
                    seen = lines.len();
                }
                continue;
            };
            cx.background_executor().timer(VERIFY_ACTION_GAP).await;
            if entry == "wait" {
                continue; // one more gap, with no input (idle and streaming measurements)
            }
            if let Some(n) = entry.strip_prefix("window:") {
                target = n.trim().parse().unwrap_or(0);
                continue;
            }
            let windows = cx.update(|cx| crate::workspace::window_handles(cx));
            let Some(handle) = windows.get(target).copied() else {
                eprintln!("SKYELAB_VERIFY_ACTIONS: no window {target}");
                continue;
            };
            let _ = handle.update(cx, |_, window, cx| {
                if let Some(keys) = entry.strip_prefix("keys:") {
                    for key in keys.split_whitespace() {
                        match gpui::Keystroke::parse_with_os(key, cx.platform_info().os) {
                            Ok(k) => _ = window.dispatch_keystroke(k, cx),
                            Err(_) => eprintln!("SKYELAB_VERIFY_ACTIONS: bad key {key}"),
                        }
                    }
                    return;
                }
                if let Some(at) = entry.strip_prefix("drag:") {
                    let n: Vec<f32> = at
                        .split(';')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                    match n[..] {
                        [x, y, x2, y2] => drag(
                            window,
                            cx,
                            gpui::point(gpui::px(x), gpui::px(y)),
                            gpui::point(gpui::px(x2), gpui::px(y2)),
                        ),
                        _ => eprintln!("SKYELAB_VERIFY_ACTIONS: bad drag {at}"),
                    }
                    return;
                }
                if let Some(at) = entry.strip_prefix("scroll:") {
                    let n: Vec<f32> = at
                        .split(';')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                    match n[..] {
                        [x, y, lines] => {
                            _ = window.dispatch_event(
                                gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                                    position: gpui::point(gpui::px(x), gpui::px(y)),
                                    delta: gpui::ScrollDelta::Lines(gpui::point(0., lines)),
                                    modifiers: Default::default(),
                                    touch_phase: Default::default(),
                                    source: Default::default(),
                                }),
                                cx,
                            )
                        }
                        _ => eprintln!("SKYELAB_VERIFY_ACTIONS: bad scroll {at}"),
                    };
                    return;
                }
                if let Some(at) = entry.strip_prefix("hover:") {
                    match parse_point(at) {
                        Some(position) => {
                            _ = window.dispatch_event(
                                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                    position,
                                    pressed_button: None,
                                    modifiers: Default::default(),
                                }),
                                cx,
                            )
                        }
                        None => eprintln!("SKYELAB_VERIFY_ACTIONS: bad point {at}"),
                    }
                    return;
                }
                if let Some((at, button)) = entry
                    .strip_prefix("click:")
                    .map(|at| (at, gpui::MouseButton::Left))
                    .or_else(|| {
                        let at = entry.strip_prefix("rclick:")?;
                        Some((at, gpui::MouseButton::Right))
                    })
                {
                    match parse_point(at) {
                        Some(position) => click(window, cx, position, button),
                        None => eprintln!("SKYELAB_VERIFY_ACTIONS: bad point {at}"),
                    }
                    return;
                }
                match cx.build_action(&entry, None) {
                    Ok(action) => window.dispatch_action(action, cx),
                    Err(e) => eprintln!("SKYELAB_VERIFY_ACTIONS: {entry}: {e:#}"),
                }
            });
        }
    })
    .detach();
}

/// `x;y` in window points, as a screenshot of the window shows them at 1x.
fn parse_point(at: &str) -> Option<gpui::Point<gpui::Pixels>> {
    let (x, y) = at.split_once(';')?;
    Some(gpui::point(
        gpui::px(x.trim().parse().ok()?),
        gpui::px(y.trim().parse().ok()?),
    ))
}

/// A click of `button` at `position` (`rclick:` is the right one): move there, press, release.
fn click(
    window: &mut gpui::Window,
    cx: &mut App,
    position: gpui::Point<gpui::Pixels>,
    button: gpui::MouseButton,
) {
    use gpui::{Modifiers, PlatformInput as P};
    let modifiers = Modifiers::default();
    window.dispatch_event(
        P::MouseMove(gpui::MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    window.dispatch_event(
        P::MouseDown(gpui::MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        P::MouseUp(gpui::MouseUpEvent {
            button,
            position,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// A left-button drag from `from` to `to`: press, move with the button held, release.
fn drag(
    window: &mut gpui::Window,
    cx: &mut App,
    from: gpui::Point<gpui::Pixels>,
    to: gpui::Point<gpui::Pixels>,
) {
    use gpui::{Modifiers, MouseButton, PlatformInput as P};
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    let events = [
        P::MouseMove(gpui::MouseMoveEvent {
            position: from,
            pressed_button: None,
            modifiers,
        }),
        P::MouseDown(gpui::MouseDownEvent {
            button,
            position: from,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        // GPUI's first move only starts an `on_drag`; `on_drag_move` listeners hear the moves after it.
        P::MouseMove(gpui::MouseMoveEvent {
            position: from + (to - from) / 2.,
            pressed_button: Some(button),
            modifiers,
        }),
        P::MouseMove(gpui::MouseMoveEvent {
            position: to,
            pressed_button: Some(button),
            modifiers,
        }),
        P::MouseUp(gpui::MouseUpEvent {
            button,
            position: to,
            modifiers,
            click_count: 1,
        }),
    ];
    for event in events {
        window.dispatch_event(event, cx);
    }
}
