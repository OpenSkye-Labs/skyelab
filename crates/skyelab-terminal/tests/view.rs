//! The terminal view driven like a user: mouse, wheel, keys and clipboard on a real PTY child.
#![cfg(unix)]

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use alacritty_terminal::term::TermMode;
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollEvent, TestAppContext, TouchPhase, VisualTestContext, point,
};
use skyelab_terminal::{SpawnOptions, Terminal, TerminalEvent, TerminalView};

const COPY: &str = if cfg!(target_os = "macos") {
    "cmd-c"
} else {
    "ctrl-shift-c"
};

fn open<'a>(
    cx: &'a mut TestAppContext,
    script: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        skyelab_terminal::init(cx);
    });
    let (term, rx) = Terminal::spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        command: Some(("/bin/sh".into(), vec!["-c".into(), script.into()])),
        env: HashMap::new(),
        cols: 80,
        lines: 24,
    })
    .expect("spawn");
    let (view, cx) = cx.add_window_view(|_, cx| TerminalView::new(term, rx, cx));
    cx.update(|window, cx| gpui::Focusable::focus_handle(view.read(cx), cx).focus(window, cx));
    (view, cx)
}

fn wait(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn text(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |v, _| v.terminal().text())
}

fn offset(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |v, _| v.terminal().display_offset())
}

fn cell(
    view: &Entity<TerminalView>,
    cx: &mut VisualTestContext,
    row: usize,
    col: usize,
) -> Point<Pixels> {
    wait(cx, "first layout", |cx| {
        view.read_with(cx, |v, _| v.cell_position(0, 0).is_some())
    });
    view.read_with(cx, |v, _| v.cell_position(row, col))
        .expect("cell inside the grid")
}

/// A point in the left half of the cell centred on `p`, so a selection starting there includes that cell.
fn left_half(p: Point<Pixels>) -> Point<Pixels> {
    point(p.x - gpui::px(2.), p.y)
}

fn click(cx: &mut VisualTestContext, at: Point<Pixels>, modifiers: Modifiers, count: usize) {
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: count,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: count,
    });
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|c| c.text())
}

fn flag(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("skyelab-view-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn wait_for(flag: &Path) -> String {
    format!("while [ ! -e '{}' ]; do sleep 0.05; done", flag.display())
}

#[gpui::test]
fn drag_select_then_copy_puts_text_on_clipboard(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "printf 'hello world\\nsecond line\\n'; sleep 30");
    wait(cx, "output", |cx| text(&view, cx).contains("second"));
    let (from, to) = (left_half(cell(&view, cx, 0, 0)), cell(&view, cx, 0, 4));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.simulate_keystrokes(COPY);
    assert_eq!(clipboard(cx).as_deref(), Some("hello"));

    let word = cell(&view, cx, 0, 8);
    click(cx, word, Modifiers::none(), 2);
    cx.simulate_keystrokes(COPY);
    assert_eq!(clipboard(cx).as_deref(), Some("world"));

    let line = cell(&view, cx, 1, 2);
    click(cx, line, Modifiers::none(), 3);
    cx.simulate_keystrokes(COPY);
    assert_eq!(
        clipboard(cx).as_deref().map(str::trim_end),
        Some("second line")
    );
}

#[gpui::test]
fn wheel_scroll_moves_offset_and_output_keeps_it(cx: &mut TestAppContext) {
    let f = flag("wheel");
    let (view, cx) = open(
        cx,
        &format!("seq 1 100; {}; seq 101 130; sleep 30", wait_for(&f)),
    );
    wait(cx, "output", |cx| text(&view, cx).contains("100"));
    let at = cell(&view, cx, 5, 5);
    cx.simulate_event(ScrollEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., 10.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: Default::default(),
    });
    cx.run_until_parked();
    let scrolled = offset(&view, cx);
    assert!(scrolled > 0, "wheel did not scroll the history");
    let before = text(&view, cx);
    std::fs::write(&f, "").expect("flag");
    wait(cx, "more output", |cx| offset(&view, cx) == scrolled + 30);
    assert_eq!(text(&view, cx), before, "output moved the scrolled-up view");
    let _ = std::fs::remove_file(f);
}

#[gpui::test]
fn shift_page_up_scrolls_history(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "stty raw -echo; seq 1 100; exec cat -v");
    wait(cx, "output", |cx| text(&view, cx).contains("100"));
    cx.simulate_keystrokes("shift-pageup");
    cx.run_until_parked();
    assert!(offset(&view, cx) > 0, "shift-pageup did not scroll");
    cx.simulate_keystrokes("shift-pagedown");
    cx.run_until_parked();
    assert_eq!(offset(&view, cx), 0);
    std::thread::sleep(Duration::from_millis(200));
    cx.run_until_parked();
    assert!(
        !text(&view, cx).contains("[5~"),
        "page keys reached the program"
    );
}

#[gpui::test]
fn shift_page_up_goes_to_the_program_in_the_alt_screen(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "stty raw -echo; printf '\\033[?1049hALT'; exec cat -v");
    wait(cx, "alt screen", |cx| text(&view, cx).contains("ALT"));
    cx.simulate_keystrokes("shift-pageup");
    wait(cx, "program got the key", |cx| {
        text(&view, cx).contains("[5")
    });
    assert_eq!(offset(&view, cx), 0);
}

#[gpui::test]
fn click_in_mouse_mode_writes_sgr_to_pty(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "stty raw -echo; printf 'hello world\\r\\n\\033[?1000h\\033[?1006h'; exec cat -v",
    );
    wait(cx, "mouse mode", |cx| {
        view.read_with(cx, |v, _| {
            v.terminal()
                .term()
                .mode()
                .contains(TermMode::MOUSE_REPORT_CLICK)
        })
    });
    let at = cell(&view, cx, 2, 3);
    click(cx, at, Modifiers::none(), 1);
    // Press and release can come back in separate reads from the pty.
    wait(cx, "press and release reports", |cx| {
        let t = text(&view, cx);
        t.contains("[<0;4;3M") && t.contains("[<0;4;3m")
    });
    assert_eq!(
        view.read_with(cx, |v, _| v.terminal().selection_text()),
        None
    );
}

#[gpui::test]
fn shift_drag_selects_in_mouse_mode(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "stty raw -echo; printf 'hello world\\r\\n\\033[?1000h\\033[?1006h'; exec cat -v",
    );
    wait(cx, "mouse mode", |cx| {
        view.read_with(cx, |v, _| {
            v.terminal()
                .term()
                .mode()
                .contains(TermMode::MOUSE_REPORT_CLICK)
        })
    });
    let (from, to) = (left_half(cell(&view, cx, 0, 0)), cell(&view, cx, 0, 4));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::shift());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::shift());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::shift());
    std::thread::sleep(Duration::from_millis(200));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, _| v.terminal().selection_text())
            .as_deref(),
        Some("hello")
    );
    assert!(!text(&view, cx).contains("[<"), "shift-drag was reported");
}

#[gpui::test]
fn cmd_click_opens_only_http(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "printf 'https://a.test file:///etc/passwd ssh://h/x ./x.rs:3 sub/../y.rs\\n'; sleep 30",
    );
    wait(cx, "output", |cx| text(&view, cx).contains("x.rs"));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, e: &TerminalEvent, _| {
            sink.borrow_mut().push(e.clone())
        })
        .detach()
    });
    let secondary = cx.update(|_, cx| Modifiers::secondary_key(cx));
    for col in [20, 36] {
        let at = cell(&view, cx, 0, col);
        click(cx, at, secondary, 1);
    }
    let url = cell(&view, cx, 0, 2);
    click(cx, url, Modifiers::none(), 1);
    assert_eq!(
        cx.opened_url(),
        None,
        "file://, ssh:// or a plain click opened something"
    );
    click(cx, url, secondary, 1);
    assert_eq!(cx.opened_url().as_deref(), Some("https://a.test"));
    let path = cell(&view, cx, 0, 46);
    click(cx, path, secondary, 1);
    let parent = cell(&view, cx, 0, 56);
    click(cx, parent, secondary, 1);
    cx.run_until_parked();
    assert_eq!(
        *events.borrow(),
        [
            TerminalEvent::OpenPath(std::env::temp_dir().join("x.rs")),
            TerminalEvent::OpenPath(std::env::temp_dir().join("y.rs")),
        ]
    );
}

#[gpui::test]
fn paste_is_bracketed_and_stripped(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "stty raw -echo; printf '\\033[?2004hREADY'; exec cat -v",
    );
    wait(cx, "bracketed paste mode", |cx| {
        text(&view, cx).contains("READY")
    });
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("a\x1b[201~b".into()));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-shift-v"
    });
    wait(cx, "paste", |cx| text(&view, cx).contains("b^[[201~"));
    let shown = text(&view, cx);
    assert!(shown.contains("^[[200~a[201~b^[[201~"), "{shown}");
    assert_eq!(shown.matches("^[[201~").count(), 1, "{shown}");
}
