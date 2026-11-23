//! The timeline's text: agent markdown (streamed words fade in, code blocks get a copy button, links are checked),
//! the turn divider and the usage-limit line.

use super::thread_items::ItemsView;
use super::thread_render::opens_in_browser;
use super::tool_cards;
use gpui::{Context, div, prelude::*};
use gpui_component::text::TextView;
use skyelab_agent::Usage;

/// The turn divider's text: "Worked for 4.2s · 12.3k in · 175 out · $0.0421".
pub(super) fn divider_text(verb: &str, secs: f64, usage: Option<Usage>, cost: f64) -> String {
    let tokens = usage
        .map(|u| format!(" · {} in · {} out", count(u.input), count(u.output)))
        .unwrap_or_default();
    let cost = if cost > 0.0 {
        format!(" · ${cost:.4}")
    } else {
        String::new()
    };
    format!("{verb} {secs:.1}s{tokens}{cost}")
}

/// A token count as T3 writes it: 175, 12.3k, 1.2M.
fn count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// "Limited until …": how long until the limit resets (the reset is in Unix seconds), when known.
pub(super) fn limited_text(resets_at: Option<i64>, now: i64) -> String {
    match resets_at.map(|r| r - now) {
        Some(left) if left > 0 => {
            let (h, m) = (left / 3600, (left % 3600 + 59) / 60);
            let wait = match (h, m) {
                (0, m) => format!("{m}m"),
                (h, 0) => format!("{h}h"),
                (h, m) => format!("{h}h {m}m"),
            };
            format!("Usage limit reached. Limited until it resets in {wait}.")
        }
        Some(_) => "Usage limit reached. It should have reset; send again to retry.".into(),
        None => "Usage limit reached. Limited until it resets.".into(),
    }
}

/// Agent markdown: selectable, a copy button on each code block, web links open in the browser and file links in
/// Changes (agent text is untrusted, so nothing else opens). `live` fades streamed words in.
pub(super) fn markdown<V: ItemsView>(
    ix: usize,
    text: &str,
    live: bool,
    cx: &mut Context<V>,
) -> TextView {
    let view = cx.entity().downgrade();
    TextView::markdown(("md", ix), text.to_owned())
        .selectable(true)
        .stream_fade(live)
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
        .on_link_click(move |url, _, _, cx| {
            if opens_in_browser(url) {
                cx.open_url(url);
            } else if let Some(path) = file_link(url) {
                let _ = view.update(cx, |v, cx| v.open_path(&path, cx));
            }
        })
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
