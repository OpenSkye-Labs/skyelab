//! Layout pieces every settings section uses (T3's settings layout): a section's bold title and one-line
//! description over one rounded card, rows with a label, a muted description and the control on the right, paths
//! truncated in the middle with a copy button, and short errors with details on demand.

use crate::ui;
use gpui::{AnyElement, App, ClipboardItem, SharedString, div, prelude::*, rems};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    tooltip::Tooltip,
    v_flex,
};
use std::path::Path;

/// A row description's size: 13 px at the default text size.
pub(super) const DESC: gpui::Rems = rems(13. / 14.);

/// A section's heading: bold title, then a muted line saying what it is for.
pub(super) fn heading(title: &str, about: &str, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .px_1()
        .child(
            div()
                .text_size(ui::SECTION)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
        .when(!about.is_empty(), |d| {
            d.child(
                div()
                    .text_size(DESC)
                    .text_color(theme.muted_foreground)
                    .child(about.to_string()),
            )
        })
}

/// A settings section: its heading, then its rows in one rounded card.
pub(super) fn section(
    title: &str,
    about: &str,
    rows: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> gpui::Div {
    v_flex()
        .gap_3()
        .child(heading(title, about, cx))
        .child(ui::group(cx.theme()).children(rows))
}

/// A row of a [`section`]; `first` has no hairline above it.
pub(super) fn row(first: bool, cx: &App) -> gpui::Div {
    ui::group_row(first, cx.theme())
}

/// The standard row: label and description on the left, the control on the right.
pub(super) fn setting(
    first: bool,
    title: &str,
    about: &str,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    row(first, cx)
        .gap_6()
        .child(label(title, about, cx))
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

/// A row's label and muted description.
pub(super) fn label(title: &str, about: &str, cx: &App) -> gpui::Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(
            div()
                .text_size(ui::BODY)
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(title.to_string()),
        )
        .when(!about.is_empty(), |d| {
            d.child(
                div()
                    .text_size(DESC)
                    .text_color(cx.theme().muted_foreground)
                    .child(about.to_string()),
            )
        })
}

/// Middle-truncated so both the root and the folder name stay visible; full path on hover.
pub(super) fn path_text(id: String, path: &Path) -> gpui::Stateful<gpui::Div> {
    let full = SharedString::from(path.display().to_string());
    div()
        .id(SharedString::from(id))
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .child(full.clone())
        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
}

/// [`path_text`] in small muted type with a copy button after it.
pub(super) fn path_line(id: String, path: &Path, cx: &App) -> gpui::Div {
    let text = path.display().to_string();
    h_flex()
        .min_w_0()
        .gap_1()
        .text_size(ui::META)
        .text_color(cx.theme().muted_foreground)
        .child(path_text(id.clone(), path))
        .child(
            Button::new(SharedString::from(format!("copy-{id}")))
                .ghost()
                .xsmall()
                .icon(IconName::Copy)
                .tooltip("Copy path")
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                }),
        )
}

/// What an error banner shows first: the first sentence, with paths shortened to their last folder, at most ~120
/// characters. The full text is behind "Details".
pub(super) fn short_error(full: &str) -> String {
    let first = full.split(['\n']).next().unwrap_or(full);
    let first = first.split(": ").next().unwrap_or(first);
    let words: Vec<String> = first
        .split(' ')
        .map(|w| {
            let path = Path::new(w.trim_matches(|c| c == '\'' || c == '"' || c == ','));
            match path.file_name() {
                Some(name) if w.contains('/') && path.components().count() > 2 => {
                    format!("…/{}", name.to_string_lossy())
                }
                _ => w.to_owned(),
            }
        })
        .collect();
    let mut short = words.join(" ");
    if short.chars().count() > 120 {
        short = short.chars().take(117).collect::<String>() + "…";
    }
    let mut c = short.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::short_error;

    #[test]
    fn errors_are_short_and_paths_shortened() {
        let e = short_error(
            "/private/tmp/claude-501/scratchpad/home-dark/demo is now bound to acme, but the task was saved under home",
        );
        assert_eq!(
            e,
            "…/demo is now bound to acme, but the task was saved under home"
        );
        assert_eq!(short_error("parsing x: bad toml\nat line 3"), "Parsing x");
        assert!(short_error(&"word ".repeat(60)).chars().count() <= 120);
    }
}
