use alacritty_terminal::{
    term::{TermMode, cell::Flags, color::Colors},
    vte::ansi::{Color, CursorShape, NamedColor, Rgb},
};
use futures::{StreamExt as _, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla, KeyBinding,
    KeyDownEvent, MouseButton, Pixels, Rgba, ShapedLine, SharedString, Task, TextAlign, TextRun,
    UnderlineStyle, Window, actions, canvas, div, fill, font, outline, point, prelude::*, px, size,
};
use gpui_component::{ActiveTheme as _, theme::ThemeColor};

use crate::{Event, Terminal, key_to_bytes, keys::paste_bytes};

actions!(terminal, [Paste]);

const CONTEXT: &str = "Terminal";

// `KeyBinding::new` can't resolve `secondary-` (only `KeyBinding::load` can), so pick per OS.
// ctrl-v is literal-next in a shell, hence ctrl-shift-v off macOS.
const PASTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-v"
} else {
    "ctrl-shift-v"
};

/// Binds the terminal's keys. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(PASTE_KEY, Paste, Some(CONTEXT))]);
}

pub struct TerminalView {
    terminal: Terminal,
    focus: FocusHandle,
    _events: Task<()>,
}

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
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        Self {
            terminal,
            focus: cx.focus_handle(),
            _events,
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let mode = *self.terminal.term().mode();
        if let Some(bytes) = key_to_bytes(&event.keystroke, mode) {
            self.terminal.write(bytes);
            cx.stop_propagation();
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
        let style = Style {
            palette: Palette::new(theme),
            font: font(theme.mono_font_family.clone()),
            font_size: theme.mono_font_size,
        };
        let view = cx.entity();
        div()
            .id("terminal")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::paste))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
            )
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
    }
}

struct Style {
    palette: Palette,
    font: Font,
    font_size: Pixels,
}

/// Everything one frame paints, built in prepaint so paint is just drawing.
struct Frame {
    rects: Vec<(Bounds<Pixels>, Hsla)>,
    cursor: Option<(Bounds<Pixels>, Hsla, bool)>,
    lines: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    line_height: Pixels,
}

impl Frame {
    fn paint(self, window: &mut Window, cx: &mut App) {
        for (bounds, color) in self.rects {
            window.paint_quad(fill(bounds, color));
        }
        if let Some((bounds, color, filled)) = self.cursor {
            window.paint_quad(if filled {
                fill(bounds, color)
            } else {
                outline(bounds, color, gpui::BorderStyle::Solid)
            });
        }
        for (origin, line) in self.lines {
            // A failed glyph raster only loses that run's text for one frame.
            let _ = line.paint(origin, self.line_height, TextAlign::Left, None, window, cx);
        }
    }
}

/// A stretch of same-styled cells on one row, shaped as one line.
struct Run {
    row: usize,
    col: usize,
    cells: usize,
    text: String,
    fg: Hsla,
    flags: Flags,
    ends: bool,
}

fn layout(
    view: &gpui::Entity<TerminalView>,
    style: &Style,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Frame {
    let text_system = window.text_system().clone();
    let font_id = text_system.resolve_font(&style.font);
    let cell_width = text_system
        .advance(font_id, style.font_size, 'm')
        .map(|s| s.width)
        .unwrap_or(style.font_size * 0.6);
    let line_height = (style.font_size * 1.3).round();
    let cols = (bounds.size.width / cell_width).floor().max(1.) as u16;
    let lines = (bounds.size.height / line_height).floor().max(1.) as u16;
    view.update(cx, |this, _| {
        this.terminal
            .resize(cols, lines, f32::from(cell_width), f32::from(line_height));
    });

    let this = view.read(cx);
    let focused = this.focus.is_focused(window);
    let term = this.terminal.term();
    let content = term.renderable_content();
    let palette = &style.palette;
    let offset = content.display_offset as i32;
    let cursor_at = {
        let p = content.cursor.point;
        ((p.line.0 + offset) as usize, p.column.0)
    };
    let block_cursor = focused && content.cursor.shape == CursorShape::Block;
    let origin = |row: usize, col: usize| {
        point(
            bounds.origin.x + cell_width * col as f32,
            bounds.origin.y + line_height * row as f32,
        )
    };

    let mut rects: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    for cell in content.display_iter {
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let (row, col) = ((cell.point.line.0 + offset) as usize, cell.point.column.0);
        let width = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let (mut fg, mut bg) = (
            palette.color(cell.fg, content.colors),
            palette.color(cell.bg, content.colors),
        );
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::DIM) {
            fg = fg.opacity(0.7);
        }
        if block_cursor && (row, col) == cursor_at {
            fg = palette.background;
        }
        if bg != palette.background {
            let cell_bounds = Bounds::new(
                origin(row, col),
                size(cell_width * width as f32, line_height),
            );
            match rects.last_mut() {
                Some((b, c)) if *c == bg && b.top_right() == cell_bounds.origin => {
                    b.size.width += cell_bounds.size.width;
                }
                _ => rects.push((cell_bounds, bg)),
            }
        }
        let flags = cell.flags & (Flags::BOLD | Flags::ITALIC | Flags::UNDERLINE);
        match runs.last_mut() {
            Some(r)
                if !r.ends
                    && r.row == row
                    && r.col + r.cells == col
                    && r.fg == fg
                    && r.flags == flags =>
            {
                r.text.push(cell.c);
                r.cells += width;
                r.ends = width == 2;
            }
            _ => runs.push(Run {
                row,
                col,
                cells: width,
                text: cell.c.to_string(),
                fg,
                flags,
                // Shape a wide char on its own so cells after it stay on the grid.
                ends: width == 2,
            }),
        }
    }

    let cursor = (content.cursor.shape != CursorShape::Hidden).then(|| {
        let (row, col) = cursor_at;
        let full = size(cell_width, line_height);
        let size = match content.cursor.shape {
            CursorShape::Beam => size(px(2.), line_height),
            CursorShape::Underline => size(cell_width, px(2.)),
            _ => full,
        };
        let mut at = origin(row, col);
        if content.cursor.shape == CursorShape::Underline {
            at.y += line_height - px(2.);
        }
        (
            Bounds::new(at, size),
            palette.cursor,
            focused || size != full,
        )
    });

    let lines = runs
        .into_iter()
        .filter(|r| !r.text.trim().is_empty() || r.flags.contains(Flags::UNDERLINE))
        .map(|r| {
            let mut font = style.font.clone();
            if r.flags.contains(Flags::BOLD) {
                font.weight = FontWeight::BOLD;
            }
            if r.flags.contains(Flags::ITALIC) {
                font.style = FontStyle::Italic;
            }
            let underline = r
                .flags
                .contains(Flags::UNDERLINE)
                .then_some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(r.fg),
                    wavy: false,
                });
            let text = SharedString::from(r.text);
            let run = TextRun {
                len: text.len(),
                font,
                color: r.fg,
                background_color: None,
                underline,
                strikethrough: None,
            };
            let shaped = text_system.shape_line(text, style.font_size, &[run], Some(cell_width));
            (origin(r.row, r.col), shaped)
        })
        .collect();

    Frame {
        rects,
        cursor,
        lines,
        line_height,
    }
}

/// The 16 ANSI colors plus the defaults, taken from the app theme where it has a matching color.
struct Palette {
    ansi: [Hsla; 16],
    foreground: Hsla,
    background: Hsla,
    cursor: Hsla,
}

impl Palette {
    fn new(t: &ThemeColor) -> Self {
        let gray = |v: u8| rgb(Rgb { r: v, g: v, b: v });
        Self {
            ansi: [
                gray(0x00),
                t.red,
                t.green,
                t.yellow,
                t.blue,
                t.magenta,
                t.cyan,
                gray(0xe5),
                t.muted_foreground,
                t.red_light,
                t.green_light,
                t.yellow_light,
                t.blue_light,
                t.magenta_light,
                t.cyan_light,
                gray(0xff),
            ],
            foreground: t.foreground,
            background: t.background,
            cursor: t.caret,
        }
    }

    fn color(&self, color: Color, overrides: &Colors) -> Hsla {
        match color {
            Color::Spec(c) => rgb(c),
            Color::Indexed(i) => self.indexed(i.into(), overrides),
            Color::Named(n) => self.indexed(n as usize, overrides),
        }
    }

    fn indexed(&self, i: usize, overrides: &Colors) -> Hsla {
        // Programs can redefine palette entries (OSC 4/10/11); honour that first.
        if let Some(c) = overrides[i] {
            return rgb(c);
        }
        const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let dim = |c: Hsla| c.opacity(0.7);
        match i {
            0..16 => self.ansi[i],
            16..232 => {
                let i = i - 16;
                rgb(Rgb {
                    r: CUBE[i / 36],
                    g: CUBE[i / 6 % 6],
                    b: CUBE[i % 6],
                })
            }
            232..256 => {
                let v = (8 + 10 * (i - 232)) as u8;
                rgb(Rgb { r: v, g: v, b: v })
            }
            _ if i == NamedColor::Background as usize => self.background,
            _ if i == NamedColor::Cursor as usize => self.cursor,
            _ if i == NamedColor::DimForeground as usize => dim(self.foreground),
            _ if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
                dim(self.ansi[i - NamedColor::DimBlack as usize])
            }
            _ => self.foreground,
        }
    }
}

fn rgb(c: Rgb) -> Hsla {
    Rgba {
        r: f32::from(c.r) / 255.,
        g: f32::from(c.g) / 255.,
        b: f32::from(c.b) / 255.,
        a: 1.,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_binding_parses() {
        // Panics on a keystroke string GPUI can't parse.
        KeyBinding::new(PASTE_KEY, Paste, Some(CONTEXT));
    }
}
