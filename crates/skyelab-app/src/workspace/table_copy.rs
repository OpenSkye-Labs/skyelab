//! The copy menu under each markdown table in a reply (T3's `MarkdownTable`, #33286): Markdown, TSV, or HTML. HTML
//! goes on the clipboard as a real `text/html` table (pasting into mail or a doc gives a table) with the TSV as the
//! plain text.

pub(super) use gpui_component::text::TableData;

#[cfg(test)]
#[path = "table_copy_test.rs"]
mod tests;

/// Tab-separated rows, one per line; a tab or line break inside a cell becomes a space.
pub(super) fn tsv(t: &TableData) -> String {
    let mut out = String::new();
    for row in std::iter::once(&t.headers).chain(&t.rows) {
        let cells: Vec<String> = row
            .iter()
            .map(|c| c.replace(['\t', '\r', '\n'], " "))
            .collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// The table as HTML, every cell escaped (agent text is untrusted).
pub(super) fn html(t: &TableData) -> String {
    let row = |cells: &[String], tag: &str| {
        let cells: String = cells
            .iter()
            .map(|c| format!("<{tag}>{}</{tag}>", escape(c)))
            .collect();
        format!("<tr>{cells}</tr>")
    };
    let body: String = t.rows.iter().map(|r| row(r, "td")).collect();
    format!(
        "<table><thead>{}</thead><tbody>{body}</tbody></table>",
        row(&t.headers, "th")
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

use crate::ui::Button;
use gpui::{App, ClipboardItem, IntoElement, ParentElement as _, Styled as _, Window};
use gpui_component::{
    WindowExt as _, h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::sync::Mutex;

/// The table's actions row: one copy button, right-aligned, whose menu picks the format.
pub(super) fn actions(t: &TableData, _: &mut Window, _: &mut App) -> gpui::AnyElement {
    let t = t.clone();
    h_flex()
        .justify_end()
        .child(
            Button::new("table-copy")
                .ghost()
                .xsmall()
                .icon(crate::ui::SkyeIcon::Copy)
                .tooltip("Copy table")
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                    let (md, tsv, html) = (t.markdown.clone(), tsv(&t), html(&t));
                    let plain = tsv.clone();
                    menu.item(
                        PopupMenuItem::new("Copy as Markdown").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(md.clone()))
                        }),
                    )
                    .item(PopupMenuItem::new("Copy as TSV").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(tsv.clone()))
                    }))
                    .item(
                        PopupMenuItem::new("Copy as HTML").on_click(move |_, window, cx| {
                            if let Err(e) = copy_html(&html, &plain) {
                                window.push_notification(
                                    format!("Couldn't copy the table as HTML: {e}"),
                                    cx,
                                );
                            }
                        }),
                    )
                }),
        )
        .into_any_element()
}

/// Puts `html` on the clipboard as HTML, with `plain` for apps that only take text.
fn copy_html(html: &str, plain: &str) -> Result<(), arboard::Error> {
    // One clipboard for the app's life: on Linux the owner serves pastes, so it must outlive the copy.
    static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);
    let mut held = CLIPBOARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if held.is_none() {
        *held = Some(arboard::Clipboard::new()?);
    }
    let clipboard = held.as_mut().expect("set just above");
    clipboard.set().html(
        format!("<meta charset=\"utf-8\">{html}"),
        Some(plain.to_owned()),
    )
}
