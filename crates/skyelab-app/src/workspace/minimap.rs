//! The timeline minimap (T3's `TimelineMinimap`): a rail at the timeline's left edge with one short tick per prompt.
//! The tick of the turn being read is brighter; hovering shows the prompt and the turn's last reply; a click scrolls
//! that prompt to the top.
//!
//! Redraw: it notifies only when the hovered tick changes and on a jump. Each frame scans the items for prompt
//! rows (a tag check per row) and draws at most [`MAX_TICKS`] ticks; preview text is derived only for the hovered
//! tick. A tick per prompt in a long thread stacked hundreds of overlapping quads in the rail, and GPUI's paint
//! order tree pays for every overlap: a third of each streaming frame went there (docs/gpui/redraw.md).

use crate::ui;
use crate::workspace::thread::ThreadView;
use crate::workspace::thread_model::Item;
use gpui::{AnyElement, Context, ListOffset, SharedString, Window, div, prelude::*, px, relative};
use gpui_component::{ActiveTheme as _, StyledExt as _};

#[cfg(test)]
#[path = "minimap_test.rs"]
mod tests;

/// Fewer prompts than this show no rail.
const MIN_PROMPTS: usize = 2;
/// Pixels between ticks while they fit; a longer thread squeezes them into the cap.
const SPACING: f32 = 8.0;
/// The rail's tallest, as a share of the timeline's height.
const MAX_SHARE: f32 = 0.7;
const TICK_W: f32 = 24.0;
/// Each tick's hit box: as tall as the spacing, wider than the tick.
const HIT_W: f32 = 40.0;
/// The most ticks drawn: past this, evenly spread prompts (plus the current and hovered ones) stand for the rest.
/// About the rail's cap (70% of a tall timeline) at 8 px apart.
pub(super) const MAX_TICKS: usize = 64;
/// How long the previews may be, in characters.
const PREVIEW: usize = 300;

/// The rows that start a turn: the user's prompts.
pub(super) fn prompt_rows(items: &[Item]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, i)| matches!(i, Item::Prompt(_)))
        .map(|(ix, _)| ix)
        .collect()
}

/// Tick `i` of `n`'s place down the rail, 0.0 (top) to 1.0 (bottom).
pub(super) fn tick_at(i: usize, n: usize) -> f32 {
    if n <= 1 {
        0.0
    } else {
        i.min(n - 1) as f32 / (n - 1) as f32
    }
}

/// The tick for the turn being read: the last prompt at or above the list's top row.
pub(super) fn current(rows: &[usize], top: usize) -> Option<usize> {
    rows.iter().rposition(|&r| r <= top)
}

/// Which of `n` prompts get a tick: all of them up to [`MAX_TICKS`], else [`MAX_TICKS`] spread evenly from the
/// first to the last, plus `keep` (the current and hovered ones). Sorted, no repeats.
pub(super) fn shown_ticks(n: usize, keep: [Option<usize>; 2]) -> Vec<usize> {
    if n <= MAX_TICKS {
        return (0..n).collect();
    }
    let mut shown: Vec<usize> = (0..MAX_TICKS)
        .map(|k| (k * (n - 1) + (MAX_TICKS - 1) / 2) / (MAX_TICKS - 1))
        .chain(keep.into_iter().flatten().filter(|&i| i < n))
        .collect();
    shown.sort_unstable();
    shown.dedup();
    shown
}

/// T3's fisheye: the hovered tick full width, its neighbours shorter.
fn width(distance: Option<usize>) -> f32 {
    TICK_W
        * match distance {
            Some(0) => 1.0,
            Some(1) => 0.667,
            Some(2) => 0.417,
            _ => 0.333,
        }
}

/// A reply (markdown) as plain text on one line, cut for the card.
fn reply_preview(text: &str) -> SharedString {
    preview(&markdown_plain(text))
}

/// `text` on one line, cut for the card. A prompt is plain text already, shown as typed.
fn preview(text: &str) -> SharedString {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(PREVIEW) {
        Some((cut, _)) => format!("{}…", &flat[..cut]).into(),
        None => flat.into(),
    }
}

/// The words of a markdown tree as the timeline draws them (gpui-base's rendered text, which find marks): markup, link
/// targets and HTML tags left out, a soft line break and runs of spaces as one space, a newline after each block and
/// table row, a space between table cells.
fn plain_text(node: &markdown::mdast::Node, out: &mut String) {
    use markdown::mdast::Node;
    match node {
        Node::Text(t) => collapse_white_space(&t.value, out),
        Node::InlineCode(c) => out.push_str(&c.value),
        Node::Code(c) => out.push_str(&c.value),
        Node::Html(h) => strip_tags(&h.value, out),
        Node::Break(_) => out.push('\n'),
        _ => {}
    }
    for child in node.children().into_iter().flatten() {
        plain_text(child, out);
    }
    match node {
        Node::Paragraph(_) | Node::Heading(_) | Node::Code(_) => out.push('\n'),
        Node::TableCell(_) => out.push(' '),
        Node::TableRow(_) => {
            if out.ends_with(' ') {
                out.pop(); // cells are joined by a space, not ended by one
            }
            out.push('\n');
        }
        _ => {}
    }
}

/// `text` with each line break and run of spaces or tabs as one space, as gpui-base lays out a paragraph (a space
/// already ending `out` counts, so "a " + a dropped tag + " b" reads "a b").
fn collapse_white_space(text: &str, out: &mut String) {
    let mut space = out.ends_with(' ');
    for c in text.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
}

/// HTML's text without its tags: gpui-base draws `<b>x</b>` as "x" and a lone `<Button>` as nothing.
fn strip_tags(html: &str, out: &mut String) {
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
}

/// A reply's markdown as [`plain_text`]. markdown-rs panics on some inputs (gpui-base guards the same call), so a
/// panic falls back to the raw text rather than taking the app down.
pub(in crate::workspace) fn markdown_plain(text: &str) -> String {
    let parsed =
        std::panic::catch_unwind(|| markdown::to_mdast(text, &markdown::ParseOptions::gfm()));
    let mut plain = String::new();
    match parsed {
        Ok(Ok(tree)) => plain_text(&tree, &mut plain),
        // GFM never fails to parse (only MDX can); the raw text then, as after a panic.
        _ => plain.push_str(text),
    }
    plain
}

/// The turn's last reply text: the last text row before the next prompt.
fn reply(items: &[Item], prompt: usize) -> Option<&str> {
    items[prompt + 1..]
        .iter()
        .take_while(|i| !matches!(i, Item::Prompt(_)))
        .filter_map(|i| match i {
            Item::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .last()
}

impl ThreadView {
    /// Scrolls prompt row `row` to the top of the timeline.
    pub(in crate::workspace) fn jump_to_prompt(
        &mut self,
        row: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let list = self.list_in(window.window_handle()).clone();
        let target = ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        };
        self.timeline.scroll = Some(crate::workspace::smooth_scroll::scroll_to(list, target, cx));
    }

    pub(in crate::workspace) fn render_minimap(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rows = prompt_rows(&self.items);
        let n = rows.len();
        if n < MIN_PROMPTS {
            return None;
        }
        let top = self
            .list_in(window.window_handle())
            .logical_scroll_top()
            .item_ix;
        let now = current(&rows, top);
        let hovered = self.timeline.minimap_hover.filter(|&h| h < n);
        let theme = cx.theme();
        let (dim, lit, card_bg, border, muted) = (
            theme.muted_foreground.opacity(0.3),
            theme.foreground.opacity(0.6),
            theme.popover,
            theme.border,
            theme.muted_foreground,
        );
        let shown = shown_ticks(n, [now, hovered]);
        // The fisheye counts drawn ticks, so a squeezed rail still narrows its neighbours.
        let hovered_at = hovered.and_then(|h| shown.iter().position(|&i| i == h));
        let ticks = shown.iter().enumerate().map(|(at, &i)| {
            let row = rows[i];
            let distance = hovered_at.map(|h| h.abs_diff(at));
            let color = if distance == Some(0) || now == Some(i) {
                lit
            } else {
                dim
            };
            div()
                .id(("minimap-tick", self.items.key(row)))
                .debug_selector(move || format!("minimap-tick-{i}"))
                .absolute()
                .left_0()
                .top(relative(tick_at(i, n)))
                .mt(px(-SPACING / 2.))
                .h(px(SPACING))
                .w(px(HIT_W))
                .flex()
                .items_center()
                .cursor_pointer()
                .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                    let next = if *on {
                        Some(i)
                    } else {
                        this.timeline.minimap_hover.filter(|&h| h != i)
                    };
                    if this.timeline.minimap_hover != next {
                        this.timeline.minimap_hover = next;
                        cx.notify();
                    }
                }))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.jump_to_prompt(row, window, cx)),
                )
                .child(
                    div()
                        .h(px(2.))
                        .w(px(width(distance)))
                        .rounded_full()
                        .bg(color),
                )
        });
        let card = hovered.map(|h| {
            let row = rows[h];
            let prompt = match &self.items[row] {
                Item::Prompt(p) => preview(p),
                _ => SharedString::default(),
            };
            div()
                .absolute()
                .left(px(HIT_W + 4.))
                .top(relative(tick_at(h, n)))
                .mt(px(-18.))
                .w(px(320.))
                .p_3()
                .rounded_xl()
                .border_1()
                .border_color(border)
                .bg(card_bg)
                .shadow_lg()
                .debug_selector(|| "minimap-card".into())
                .child(
                    div()
                        .text_size(ui::BODY)
                        .font_medium()
                        .truncate()
                        .child(prompt),
                )
                .children(reply(&self.items, row).map(|r| {
                    div()
                        .mt_1()
                        .text_size(ui::BODY)
                        .text_color(muted)
                        .line_clamp(3)
                        .child(reply_preview(r))
                }))
        });
        let height = (n - 1) as f32 * SPACING;
        Some(
            // Only the ticks take the pointer; the rest of the strip lets clicks through to the timeline.
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_3()
                .flex()
                .items_center()
                .child(
                    div()
                        .relative()
                        .w(px(HIT_W))
                        .h(px(height))
                        .max_h(relative(MAX_SHARE))
                        .children(ticks)
                        .children(card),
                )
                .into_any_element(),
        )
    }
}
