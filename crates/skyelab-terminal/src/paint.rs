//! Drawing the grid: one frame's rects, cursor and shaped text runs, in the app theme's colours.

use alacritty_terminal::{
    grid::Dimensions,
    term::{cell::Flags, color::Colors},
    vte::ansi::{Color, CursorShape, NamedColor, Rgb},
};
use gpui::{
    App, Bounds, Entity, Font, FontStyle, FontWeight, Hsla, Pixels, Rgba, ShapedLine, SharedString,
    TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px, size,
};
use gpui_component::theme::ThemeColor;

use crate::view::{Geometry, TerminalView};

pub(crate) struct Style {
    pub(crate) palette: Palette,
    pub(crate) font: Font,
    pub(crate) font_size: Pixels,
}

/// Everything one frame paints, built in prepaint so paint is just drawing.
pub(crate) struct Frame {
    rects: Vec<(Bounds<Pixels>, Hsla)>,
    cursor: Option<(Bounds<Pixels>, Hsla, bool)>,
    lines: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    line_height: Pixels,
}

impl Frame {
    pub(crate) fn paint(self, window: &mut Window, cx: &mut App) {
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

pub(crate) fn layout(
    view: &Entity<TerminalView>,
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
        this.geometry = Some(Geometry {
            origin: bounds.origin,
            cell_width,
            line_height,
        });
    });

    let this = view.read(cx);
    let focused = this.focus.is_focused(window);
    let term = this.terminal.term();
    let content = term.renderable_content();
    let selection = content.selection;
    let hovered = this.hovered.as_ref().map(|l| (l.start, l.end));
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
    let mut selected: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut cursor_cells = 1;
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
        if (row, col) == cursor_at {
            cursor_cells = width;
        }
        if block_cursor && (row, col) == cursor_at {
            fg = palette.background;
        }
        let cell_bounds = Bounds::new(
            origin(row, col),
            size(cell_width * width as f32, line_height),
        );
        if bg != palette.background {
            push_rect(&mut rects, cell_bounds, bg);
        }
        if selection.is_some_and(|s| s.contains(cell.point)) {
            push_rect(&mut selected, cell_bounds, palette.selection);
        }
        let mut flags = cell.flags & (Flags::BOLD | Flags::ITALIC | Flags::UNDERLINE);
        if hovered.is_some_and(|(start, end)| (start..=end).contains(&cell.point)) {
            flags |= Flags::UNDERLINE;
            fg = palette.link;
        }
        match runs.last_mut() {
            Some(r)
                if !r.ends
                    && r.row == row
                    && r.col + r.cells == col
                    && r.fg == fg
                    && r.flags == flags =>
            {
                r.text.push(cell.c);
                r.text.extend(cell.zerowidth().into_iter().flatten());
                r.cells += width;
                r.ends = width == 2;
            }
            _ => runs.push(Run {
                row,
                col,
                cells: width,
                text: std::iter::once(cell.c)
                    .chain(cell.zerowidth().into_iter().flatten().copied())
                    .collect(),
                fg,
                flags,
                // Shape a wide char on its own so cells after it stay on the grid.
                ends: width == 2,
            }),
        }
    }

    // Scrolled far enough up, the cursor's row is below the view: draw none.
    let on_screen = cursor_at.0 < term.screen_lines();
    let cursor = (content.cursor.shape != CursorShape::Hidden && on_screen).then(|| {
        let (row, col) = cursor_at;
        let full = size(cell_width * cursor_cells as f32, line_height);
        let size = match content.cursor.shape {
            CursorShape::Beam => size(px(2.), line_height),
            CursorShape::Underline => size(full.width, px(2.)),
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

    rects.extend(selected);
    Frame {
        rects,
        cursor,
        lines,
        line_height,
    }
}

/// Adds a cell's rect, merged into the previous one when it continues it in the same color.
fn push_rect(rects: &mut Vec<(Bounds<Pixels>, Hsla)>, cell: Bounds<Pixels>, color: Hsla) {
    match rects.last_mut() {
        Some((b, c)) if *c == color && b.top_right() == cell.origin => {
            b.size.width += cell.size.width;
        }
        _ => rects.push((cell, color)),
    }
}

/// The 16 ANSI colors the app's theme family sets for its light and dark theme (`terminal.ansi` in its JSON). Without
/// it the palette falls back to the theme's base colors.
#[derive(Clone, Copy)]
pub struct AnsiColors {
    pub light: [Hsla; 16],
    pub dark: [Hsla; 16],
}

impl gpui::Global for AnsiColors {}

/// The 16 ANSI colors plus the defaults, taken from the app theme where it has a matching color.
pub(crate) struct Palette {
    ansi: [Hsla; 16],
    foreground: Hsla,
    pub(crate) background: Hsla,
    cursor: Hsla,
    /// Drawn over selected cells; the theme's selection color is translucent.
    selection: Hsla,
    link: Hsla,
}

impl Palette {
    pub(crate) fn new(t: &ThemeColor, ansi: Option<[Hsla; 16]>) -> Self {
        let gray = |v: u8| rgb(Rgb { r: v, g: v, b: v });
        Self {
            ansi: ansi.unwrap_or([
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
            ]),
            foreground: t.foreground,
            background: t.background,
            cursor: t.caret,
            selection: t.selection,
            link: t.link,
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
#[path = "palette_test.rs"]
mod palette_test;
