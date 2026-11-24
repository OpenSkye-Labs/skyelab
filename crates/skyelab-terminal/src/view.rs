use std::path::{Component, PathBuf};

use alacritty_terminal::{grid::Scroll, index::Side, selection::SelectionType, term::TermMode};
use futures::{StreamExt as _, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyBinding, KeyDownEvent, Modifiers,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    ScrollEvent, Task, Window, actions, canvas, div, font, point, prelude::*,
};
use gpui_component::{ActiveTheme as _, menu::ContextMenuExt as _};

use crate::{
    Event, Link, LinkKind, OptionKeys, Terminal, key_to_bytes,
    keys::paste_bytes,
    links::path_target,
    mouse::{self, MouseAction, MouseMods, mouse_report},
    paint::{Palette, Style, layout},
};

actions!(terminal, [Copy, Paste, AddToPrompt]);

/// What a terminal asks its host to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// A file path was cmd-clicked (absolute, resolved against the terminal's start folder).
    OpenPath(PathBuf),
    /// "Add to prompt" with this selected text.
    AddToPrompt(String),
}

const CONTEXT: &str = "Terminal";

// `KeyBinding::new` can't resolve `secondary-` (only `KeyBinding::load` can), so pick per OS.
// ctrl-v is literal-next in a shell, hence ctrl-shift-v off macOS.
const PASTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-v"
} else {
    "ctrl-shift-v"
};
// ctrl-l clears the screen in a shell, hence ctrl-shift-l off macOS.
const ADD_TO_PROMPT_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-l"
} else {
    "ctrl-shift-l"
};
// ctrl-c is interrupt in a shell, hence ctrl-shift-c off macOS.
const COPY_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-c"
} else {
    "ctrl-shift-c"
};

/// Binds the terminal's keys. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(PASTE_KEY, Paste, Some(CONTEXT)),
        KeyBinding::new(COPY_KEY, Copy, Some(CONTEXT)),
        KeyBinding::new(ADD_TO_PROMPT_KEY, AddToPrompt, Some(CONTEXT)),
    ]);
}

/// Where the last frame put the grid, so mouse positions map to cells.
#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub(crate) origin: gpui::Point<Pixels>,
    pub(crate) cell_width: Pixels,
    pub(crate) line_height: Pixels,
}

pub struct TerminalView {
    pub(crate) terminal: Terminal,
    option_keys: OptionKeys,
    pub(crate) focus: FocusHandle,
    pub(crate) geometry: Option<Geometry>,
    /// A left-button drag is extending the selection.
    selecting: bool,
    /// The last cell a mouse report named, so motion reports only go out when the cell changes.
    reported_cell: Option<(usize, usize)>,
    /// The link under the mouse while cmd (ctrl off macOS) is held; drawn underlined.
    pub(crate) hovered: Option<Link>,
    mouse_at: Option<gpui::Point<Pixels>>,
    /// Wheel movement smaller than a line, carried to the next event (trackpads send fractions).
    wheel_rest: f32,
    _events: Task<()>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl TerminalView {
    /// Wraps a spawned terminal; `events` is the receiver `Terminal::spawn` returned.
    pub fn new(
        terminal: Terminal,
        mut events: UnboundedReceiver<Event>,
        cx: &mut Context<Self>,
    ) -> Self {
        let _events = cx.spawn(async move |this, cx| {
            while let Some(first) = events.next().await {
                // Take everything already queued so a burst of output costs one notify, not one per read.
                let mut batch = vec![first];
                while let Ok(ev) = events.try_recv() {
                    batch.push(ev);
                }
                let applied = this.update(cx, |this, cx| {
                    for ev in &batch {
                        this.terminal.handle_event(ev);
                    }
                    // Output may have scrolled the hovered link away; the next mouse move finds it again.
                    this.hovered = None;
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        Self {
            terminal,
            option_keys: OptionKeys::default(),
            focus: cx.focus_handle(),
            geometry: None,
            selecting: false,
            reported_cell: None,
            hovered: None,
            mouse_at: None,
            wheel_rest: 0.,
            _events,
        }
    }

    pub fn set_option_keys(&mut self, option_keys: OptionKeys) {
        self.option_keys = option_keys;
    }

    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// The window position of a viewport cell's centre, once the view has been laid out.
    pub fn cell_position(&self, row: usize, col: usize) -> Option<gpui::Point<Pixels>> {
        let g = self.geometry?;
        Some(point(
            g.origin.x + g.cell_width * (col as f32 + 0.5),
            g.origin.y + g.line_height * (row as f32 + 0.5),
        ))
    }

    /// The viewport cell under a window position, and which half of it.
    fn cell_at(&self, at: gpui::Point<Pixels>) -> Option<(usize, usize, Side)> {
        let g = self.geometry?;
        let (cols, lines) = self.terminal.size();
        let x = ((at.x - g.origin.x) / g.cell_width).max(0.);
        let y = ((at.y - g.origin.y) / g.line_height).max(0.);
        let col = (x.floor() as usize).min(usize::from(cols).saturating_sub(1));
        let row = (y.floor() as usize).min(usize::from(lines).saturating_sub(1));
        let side = if x.fract() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        Some((row, col, side))
    }

    /// Mouse events go to the program when it asked for them, unless Shift is held.
    fn reporting(&self, modifiers: &Modifiers) -> bool {
        !modifiers.shift && self.terminal.term().mode().intersects(TermMode::MOUSE_MODE)
    }

    fn report(
        &mut self,
        action: MouseAction,
        button: mouse::MouseButton,
        row: usize,
        col: usize,
        m: &Modifiers,
    ) {
        let mods = MouseMods {
            shift: m.shift,
            alt: m.alt,
            ctrl: m.control,
        };
        let mode = *self.terminal.term().mode();
        if let Some(bytes) = mouse_report(action, button, col, row, mods, mode) {
            self.terminal.write(bytes);
        }
        self.reported_cell = Some((row, col));
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let Some((row, col, side)) = self.cell_at(event.position) else {
            return;
        };
        if event.button == MouseButton::Left
            && event.modifiers.secondary(cx)
            && let Some(link) = self.terminal.link_at(row, col)
        {
            self.open_link(&link, cx);
            return;
        }
        if self.reporting(&event.modifiers) {
            if let Some(button) = report_button(event.button) {
                self.report(MouseAction::Press, button, row, col, &event.modifiers);
            }
            cx.stop_propagation(); // the program gets the right click, not the context menu
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let kind = match event.click_count {
            0 | 1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        self.terminal.select(kind, row, col, side);
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.mouse_at = Some(event.position);
        self.update_hover(&event.modifiers, cx);
        let Some((row, col, side)) = self.cell_at(event.position) else {
            return;
        };
        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            self.terminal.extend_selection(row, col, side);
            cx.notify();
        } else if self.reporting(&event.modifiers) && self.reported_cell != Some((row, col)) {
            let button = event
                .pressed_button
                .and_then(report_button)
                .unwrap_or(mouse::MouseButton::None);
            self.report(MouseAction::Motion, button, row, col, &event.modifiers);
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        if event.button == MouseButton::Left {
            self.selecting = false;
        }
        if !self.reporting(&event.modifiers) {
            return;
        }
        if let (Some((row, col, _)), Some(button)) =
            (self.cell_at(event.position), report_button(event.button))
        {
            self.report(MouseAction::Release, button, row, col, &event.modifiers);
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(g) = self.geometry else { return };
        let lines = self.wheel_rest + event.delta.pixel_delta(g.line_height).y / g.line_height;
        let whole = lines.trunc();
        self.wheel_rest = lines - whole;
        let n = whole as i32;
        cx.stop_propagation();
        if n == 0 {
            return;
        }
        let mode = *self.terminal.term().mode();
        let up = n > 0;
        // shortcut: one wheel event reports at most 10 lines, so a flung trackpad doesn't flood the program.
        let count = n.unsigned_abs().min(10);
        if self.reporting(&event.modifiers) {
            if let Some((row, col, _)) = self.cell_at(event.position) {
                let button = if up {
                    mouse::MouseButton::WheelUp
                } else {
                    mouse::MouseButton::WheelDown
                };
                for _ in 0..count {
                    self.report(MouseAction::Press, button, row, col, &event.modifiers);
                }
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
            && !event.modifiers.shift
        {
            // A full-screen program without mouse tracking (less, man) scrolls on arrow keys.
            let key: &[u8] = match (up, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.terminal.write(key.repeat(count as usize));
        } else {
            self.terminal.scroll(Scroll::Delta(n));
            cx.notify();
        }
    }

    fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_hover(&event.modifiers, cx);
    }

    fn update_hover(&mut self, modifiers: &Modifiers, cx: &mut Context<Self>) {
        let link = modifiers
            .secondary(cx)
            .then(|| self.mouse_at.and_then(|at| self.cell_at(at)))
            .flatten()
            .and_then(|(row, col, _)| self.terminal.link_at(row, col));
        if link != self.hovered {
            self.hovered = link;
            cx.notify();
        }
    }

    fn open_link(&mut self, link: &Link, cx: &mut Context<Self>) {
        match link.kind {
            // Only http(s) leaves the app: never file:// or a scheme handler a program printed.
            LinkKind::Url
                if link.text.starts_with("https://") || link.text.starts_with("http://") =>
            {
                cx.open_url(&link.text)
            }
            LinkKind::Url => {}
            LinkKind::Path => {
                // shortcut: relative paths resolve against the folder the terminal started in, not the shell's
                // current one (we don't track `cd`); upgrade with OSC 7 cwd reports.
                let path = self.terminal.cwd().join(path_target(&link.text));
                let mut normal = PathBuf::new();
                for c in path.components() {
                    match c {
                        Component::CurDir => {}
                        Component::ParentDir => _ = normal.pop(),
                        c => normal.push(c),
                    }
                }
                let path = normal;
                cx.emit(TerminalEvent::OpenPath(path));
            }
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let mode = *self.terminal.term().mode();
        let k = &event.keystroke;
        let only_shift =
            k.modifiers.shift && !k.modifiers.control && !k.modifiers.alt && !k.modifiers.platform;
        if only_shift && !mode.contains(TermMode::ALT_SCREEN) {
            let page = match k.key.as_str() {
                "pageup" => Some(Scroll::PageUp),
                "pagedown" => Some(Scroll::PageDown),
                _ => None,
            };
            if let Some(page) = page {
                self.terminal.scroll(page);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        if let Some(bytes) = key_to_bytes(k, mode, self.option_keys.resolve(device_flags())) {
            self.terminal.scroll(Scroll::Bottom);
            self.terminal.write(bytes);
            cx.stop_propagation();
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.terminal.selection_text() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    fn add_to_prompt(&mut self, _: &AddToPrompt, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.terminal.selection_text() {
            cx.emit(TerminalEvent::AddToPrompt(text));
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let bracketed = self
            .terminal
            .term()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        let bytes = paste_bytes(&text, bracketed);
        self.terminal.scroll(Scroll::Bottom);
        self.terminal.write(bytes);
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let ansi = cx.try_global::<crate::AnsiColors>().map(|a| {
            if theme.mode.is_dark() {
                a.dark
            } else {
                a.light
            }
        });
        let style = Style {
            palette: Palette::new(theme, ansi),
            font: font(theme.mono_font_family.clone()),
            font_size: theme.mono_font_size,
        };
        let view = cx.entity();
        let (focus, selected) = (self.focus.clone(), self.terminal.selection_text().is_some());
        div()
            .id("terminal")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::add_to_prompt))
            .on_key_down(cx.listener(Self::key_down))
            .on_any_mouse_down(cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_scroll(cx.listener(Self::scroll_wheel))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .map(|d| {
                if self.hovered.is_some() {
                    d.cursor_pointer()
                } else {
                    d.cursor_text()
                }
            })
            .size_full()
            .p_1()
            .bg(style.palette.background)
            .child(
                canvas(
                    move |bounds, window, cx| layout(&view, &style, bounds, window, cx),
                    |_, frame, window, cx| frame.paint(window, cx),
                )
                .size_full(),
            )
            .context_menu(move |menu, _, _| {
                menu.action_context(focus.clone())
                    .menu_with_disabled("Copy", Box::new(Copy), !selected)
                    .menu("Paste", Box::new(Paste))
                    .separator()
                    .menu_with_disabled("Add to prompt", Box::new(AddToPrompt), !selected)
            })
    }
}

/// The mouse button a report names; GPUI's back/forward buttons have no xterm code.
fn report_button(button: MouseButton) -> Option<mouse::MouseButton> {
    match button {
        MouseButton::Left => Some(mouse::MouseButton::Left),
        MouseButton::Middle => Some(mouse::MouseButton::Middle),
        MouseButton::Right => Some(mouse::MouseButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

/// macOS's device-dependent modifier bits, which tell left from right Option; GPUI's keystroke has one `alt` flag.
/// This reads the live state, which still holds the key while its key-down event is handled.
#[cfg(target_os = "macos")]
fn device_flags() -> usize {
    objc2_app_kit::NSEvent::modifierFlags_class().bits()
}

#[cfg(not(target_os = "macos"))]
fn device_flags() -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_parse() {
        // Panics on a keystroke string GPUI can't parse.
        KeyBinding::new(PASTE_KEY, Paste, Some(CONTEXT));
        KeyBinding::new(COPY_KEY, Copy, Some(CONTEXT));
        KeyBinding::new(ADD_TO_PROMPT_KEY, AddToPrompt, Some(CONTEXT));
    }
}
