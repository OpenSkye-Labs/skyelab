//! The browser tab's own commands (zoom, find in page, the ⋯ menu) and their keys.

use gpui::{App, KeyBinding, actions};

actions!(
    skyelab,
    [
        BrowserZoomIn,
        BrowserZoomOut,
        BrowserZoomReset,
        BrowserFind,
        BrowserCloseFind,
        BrowserReload,
        BrowserDuplicate,
        BrowserCopyUrl,
        BrowserOpenExternal,
        BrowserDevTools,
    ]
);

/// The key context of a browser tab, and of its find bar.
pub(crate) const CONTEXT: &str = "BrowserView";
pub(crate) const FIND_CONTEXT: &str = "BrowserFind";

macro_rules! mod_key {
    ($k:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $k)
        } else {
            concat!("ctrl-", $k)
        }
    };
}
pub const FIND_KEY: &str = mod_key!("f");
pub const ZOOM_IN_KEY: &str = mod_key!("=");
pub const ZOOM_OUT_KEY: &str = mod_key!("-");
pub const ZOOM_RESET_KEY: &str = mod_key!("0");

/// Binds the tab's keys. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    let tab = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new(FIND_KEY, BrowserFind, tab),
        KeyBinding::new(ZOOM_IN_KEY, BrowserZoomIn, tab),
        // ⌘⇧= arrives as ⌘+ on macOS.
        KeyBinding::new(mod_key!("+"), BrowserZoomIn, tab),
        KeyBinding::new(ZOOM_OUT_KEY, BrowserZoomOut, tab),
        KeyBinding::new(ZOOM_RESET_KEY, BrowserZoomReset, tab),
        KeyBinding::new("escape", BrowserCloseFind, Some(FIND_CONTEXT)),
    ]);
}

/// Chrome's zoom levels.
const ZOOM_STEPS: [f64; 17] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// The next zoom level up (`zoom_in`) or down from `current`, staying within 25%..500%.
pub(crate) fn zoom_step(current: f64, zoom_in: bool) -> f64 {
    let (first, last) = (ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
    let next = match zoom_in {
        true => ZOOM_STEPS.iter().find(|&&z| z > current + 1e-6),
        false => ZOOM_STEPS.iter().rev().find(|&&z| z < current - 1e-6),
    };
    next.copied()
        .unwrap_or(if zoom_in { last } else { first })
        .clamp(first, last)
}

/// A script that finds `query` in the page (WebKit and Chromium's `window.find`: selects the match and scrolls to
/// it, wrapping) and evaluates to whether it did. `restart` starts at the current match, so a longer query grows it
/// in place; an empty query clears the selection instead.
pub(crate) fn find_script(query: &str, backwards: bool, restart: bool) -> String {
    if query.is_empty() {
        return "(getSelection().removeAllRanges(), false)".into();
    }
    // A JSON string is a valid JS string literal.
    let query = serde_json::Value::from(query).to_string();
    let start = match restart {
        true => "{ const s = getSelection(); if (s.rangeCount) s.collapseToStart(); } ",
        false => "",
    };
    format!("(() => {{ {start}return window.find({query}, false, {backwards}, true); }})()")
}

/// The page's URL when the system browser may open it: http and https only, since the OS would run anything else
/// (a `file://` script, a custom scheme) with whatever app claims it.
pub(crate) fn external_url(url: &str) -> Option<url::Url> {
    url::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
}

#[cfg(test)]
#[path = "controls_test.rs"]
mod tests;
