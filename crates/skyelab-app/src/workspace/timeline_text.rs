//! The timeline's text: agent markdown (streamed words fade in, code blocks get a copy button, tables a copy menu,
//! mermaid fences draw as diagrams, links are checked),
//! the turn divider and the usage-limit line.

use super::files::{lexically_inside, link_line, linkify};
use super::thread_items::ItemsView;
use super::thread_render::opens_in_browser;
use super::tool_cards;
use gpui::{Context, SharedString, div, prelude::*};
use gpui_component::text::{TextView, TextViewState};
pub(crate) use skyelab_agent::timeline::cards::{count, divider_text, limited_text, wait};

/// Agent markdown: selectable, a copy button on each code block, web links open in the browser and file links in
/// the Preview tab (agent text is untrusted, so nothing else opens). Paths in `root` become links, file links outside
/// it plain text (see `files::linkify`). `live` fades streamed words in. `find` (the find bar's query, and which of
/// this text's matches is the current one) marks the matched words.
pub(super) fn markdown<V: ItemsView>(
    id: impl Into<gpui::ElementId>,
    text: &str,
    live: bool,
    root: Option<&std::path::Path>,
    find: Option<(&str, Option<usize>)>,
    window: &mut gpui::Window,
    cx: &mut Context<V>,
) -> TextView {
    let id = id.into();
    let view = cx.entity().downgrade();
    let text: SharedString = match root {
        None => text.to_owned().into(),
        // Linkified once per text, not on every frame: the element keeps the last result.
        Some(root) => {
            let cache = window.use_keyed_state(id.clone(), cx, |_, _| Linked::default());
            let hit = cache.read(cx);
            if hit.source == text && hit.root == root {
                hit.linked.clone()
            } else {
                let linked: SharedString = linkify(text, root).into();
                cache.update(cx, |c, _| {
                    *c = Linked {
                        source: text.to_owned(),
                        root: root.to_path_buf(),
                        linked: linked.clone(),
                    }
                });
                linked
            }
        }
    };
    // The view's own state (not `TextView::markdown`'s hidden one), so find can mark words in it.
    let state = window.use_keyed_state(SharedString::from(format!("{id}/text")), cx, |_, cx| {
        TextViewState::markdown(&text, cx)
    });
    state.update(cx, |s, cx| s.set_text(&text, cx));
    super::find_marks::mark_words(&state, find, window, cx);
    let root = root.map(std::path::Path::to_path_buf);
    TextView::new(&state)
        .selectable(true)
        .stream_fade(live)
        // A closed ```mermaid fence draws as a diagram (see `mermaid`).
        .markdown_block_parser(super::mermaid::parse)
        .markdown_block_renderer(super::mermaid::NODE, super::mermaid_block::render)
        .code_block_actions(move |block, _, _| {
            let code = block.code();
            let key = code.len() as u64
                ^ code
                    .bytes()
                    .fold(0u64, |h, b| h.rotate_left(5) ^ u64::from(b));
            div()
                .debug_selector(|| "code-copy".into())
                .child(tool_cards::copy(("copy-code", key), code))
        })
        .table_actions(super::table_copy::actions)
        .on_link_click(move |url, _, _, cx| {
            if opens_in_browser(url) {
                cx.open_url(url);
            } else if let Some(path) = file_link(url)
                // A link form linkify didn't unwrap (a reference link) still opens nothing outside the folder.
                && root.as_deref().is_some_and(|r| lexically_inside(&path, r))
            {
                let line = link_line(url);
                let _ = view.update(cx, |v, cx| v.preview_path(&path, line, cx));
            } else if let Some(path) = file_url(url)
                && root.is_some()
            {
                // agy links its files as `file:///…`: in the folder they preview; outside it only the plan it wrote
                // for review opens (as its card, never read from disk).
                let inside = root.as_deref().is_some_and(|r| lexically_inside(&path, r));
                let _ = view.update(cx, |v, cx| {
                    if !v.reveal_plan(&path, cx) && inside {
                        v.preview_path(&path, None, cx)
                    }
                });
            }
        })
}

/// A markdown block's last linkified text, kept by its element.
#[derive(Default)]
struct Linked {
    source: String,
    root: std::path::PathBuf,
    linked: SharedString,
}

/// A markdown link target that names a file (`src/a.rs`, `/abs/b.rs:12`), without its line suffix. Anything with a
/// scheme (`file://`, `mailto:`, an app's URL) is refused.
pub(super) fn file_link(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme = url.split_once(':').is_some_and(|(s, rest)| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            && !rest.starts_with(|c: char| c.is_ascii_digit())
            // `C:/repo/a.rs` is a Windows path, not a scheme.
            && !(s.len() == 1 && rest.starts_with(['/', '\\']))
    });
    if url.is_empty() || url.contains("://") || scheme || url.starts_with('#') {
        return None;
    }
    // `path:12` or `path:12:3`: the line goes, the file stays.
    let mut path = url;
    for _ in 0..2 {
        if let Some((head, tail)) = path.rsplit_once(':')
            && !tail.is_empty()
            && tail.chars().all(|c| c.is_ascii_digit())
        {
            path = head;
        }
    }
    Some(path.to_owned())
}

/// The absolute path a `file:///abs/path` link names (`None` for any other link, or a relative or `..` path).
pub(super) fn file_url(url: &str) -> Option<String> {
    let path = url.trim().strip_prefix("file://")?;
    let ok = path.starts_with('/')
        && !std::path::Path::new(path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    ok.then(|| path.to_owned())
}

/// A streaming reply's row. A long reply streams as several rows (`timeline::split_live`); its parts sit together,
/// with no row padding between them, like one message.
pub(super) fn streaming_row<V: ItemsView>(
    this: &V,
    ix: usize,
    text: &str,
    root: Option<&std::path::Path>,
    window: &mut gpui::Window,
    cx: &mut Context<V>,
) -> gpui::AnyElement {
    use super::thread_model::Item;
    use skyelab_agent::DeltaKind;
    let part = |i: Option<&Item>| {
        matches!(
            i,
            Some(Item::Live {
                kind: DeltaKind::Text,
                ..
            })
        )
    };
    let after = part(this.items().get(ix + 1));
    let before = ix > 0 && part(this.items().get(ix - 1));
    let key = this.items().key(ix);
    gpui_component::h_flex()
        .w_full()
        .justify_center()
        .px_6()
        .child(
            div()
                .w_full()
                .max_w(crate::ui::CHAT_MAX_W)
                .when(!before, |d| d.pt_2())
                .when(!after, |d| d.pb_2())
                .debug_selector(|| "streaming-text".into())
                .px_1()
                .text_size(crate::ui::BODY)
                .child(markdown(
                    ("md", key),
                    text,
                    true,
                    root,
                    this.find_words(ix),
                    window,
                    cx,
                )),
        )
        .into_any_element()
}
