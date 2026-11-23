//! The timeline's text: agent markdown (streamed words fade in, code blocks get a copy button, links are checked),
//! the turn divider and the usage-limit line.

use super::files::{lexically_inside, link_line, linkify};
use super::thread_items::ItemsView;
use super::thread_render::opens_in_browser;
use super::tool_cards;
use gpui::{Context, SharedString, div, prelude::*};
use gpui_component::text::TextView;
use skyelab_agent::Usage;

/// The turn divider's text: "Worked for 4.2s · 12.3k in · 175 out · $0.0421".
/// A turn that recorded nothing (an imported session's) reads "Worked", not "Worked for 0.0s".
pub(super) fn divider_text(verb: &str, secs: f64, usage: Option<Usage>, cost: f64) -> String {
    if secs <= 0.0 && usage.is_none() && cost <= 0.0 {
        return verb
            .trim_end_matches(" for")
            .trim_end_matches(" after")
            .to_owned();
    }
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
pub(in crate::workspace) fn count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..999_950 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// "Limited until …": how long until the limit resets (the reset is in Unix seconds), when known.
pub(super) fn limited_text(resets_at: Option<i64>, now: i64) -> String {
    match resets_at.map(|r| r - now) {
        Some(left) if left > 0 => {
            format!(
                "Usage limit reached. Limited until it resets in {}.",
                wait(left)
            )
        }
        Some(_) => "Usage limit reached. It should have reset; send again to retry.".into(),
        None => "Usage limit reached. Limited until it resets.".into(),
    }
}

/// `left` seconds as "2h 14m", rounded up to the minute.
pub(in crate::workspace) fn wait(left: i64) -> String {
    let minutes = (left + 59) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Agent markdown: selectable, a copy button on each code block, web links open in the browser and file links in
/// the Preview tab (agent text is untrusted, so nothing else opens). Paths in `root` become links, file links outside
/// it plain text (see `files::linkify`). `live` fades streamed words in.
pub(super) fn markdown<V: ItemsView>(
    id: impl Into<gpui::ElementId>,
    text: &str,
    live: bool,
    root: Option<&std::path::Path>,
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
    let root = root.map(std::path::Path::to_path_buf);
    TextView::markdown(id, text)
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
            } else if let Some(path) = file_link(url)
                // A link form linkify didn't unwrap (a reference link) still opens nothing outside the folder.
                && root.as_deref().is_some_and(|r| lexically_inside(&path, r))
            {
                let line = link_line(url);
                let _ = view.update(cx, |v, cx| v.preview_path(&path, line, cx));
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
