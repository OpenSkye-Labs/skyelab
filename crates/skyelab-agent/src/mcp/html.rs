//! Visual replies, after T3's `html_render` and `html_preview`: an agent shows a self-contained HTML page in its own
//! thread, or checks one first as a PNG. The limits are T3's. The page itself is untrusted; the app renders it in a
//! locked, private webview (`skyelab_browser::page`).

use super::tools::bounded;
use serde_json::{Map, Value, json};

pub const RENDER_TOOL: &str = "html_render";
pub const PREVIEW_TOOL: &str = "html_preview";
/// Longest page, in characters.
pub const MAX_HTML: usize = 512_000;
pub const MAX_TITLE: usize = 200;
/// The frame height an agent may ask for, in CSS pixels.
pub const MIN_HEIGHT: u64 = 80;
pub const MAX_HEIGHT: u64 = 2000;
/// The width pages are laid out at: T3's reply column.
pub const COLUMN_WIDTH: u32 = 728;
pub const MIN_WIDTH: u32 = 240;
pub const MAX_WIDTH: u32 = 1600;

/// What the agent reads after a render, as T3 words it.
pub const RENDERED: &str = "Shown to the reader above your reply. Don't mention or describe the page; reply with \
    only what it doesn't already say.";

const THEME_GUIDE: &str = "Use theme CSS variables on :root, not fixed colors: --background, --foreground, --muted, \
    --card, --popover, --secondary, --border, --ring, --primary, --accent, --destructive, --warning, --success, \
    --info, --code-background (most have a matching -foreground; accent, destructive and warning also have -surface), \
    --chart-1 to --chart-6, --radius, --font-sans, --font-mono. The base sheet sets background, text and font.";

const LAYOUT_GUIDE: &str = "Frame width is the reply width (728px), on the thread background. Leave html, body and \
    the outer element with no background color, no outer card, border, banner title or side padding. SkyeLab adds \
    an 8px html gutter. A box that needs its own background gets 16px padding and var(--radius) corners. Give charts \
    fixed pixel heights. Let content set the height: no vh units, no height:100% on html or body.";

const PAGE_RULES: &str = "Self-contained: inline <style> and <script>. Scripts, styles and fonts may load only \
    from the hosts the user allows (by default https://cdnjs.cloudflare.com, https://cdn.jsdelivr.net and \
    https://unpkg.com); images and media never load from the network, so inline them as data: URIs or build them \
    in script (SVG, canvas, blob:). fetch, XHR, WebSocket, frames, forms and http: are blocked, and links don't \
    navigate. A host the page was refused shows on its card, and only the user can allow it.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlTool {
    /// Shows the page in the caller's thread. The tool call itself (stored with the thread's events) is the record.
    Render {
        html: String,
        title: String,
        height: u32,
    },
    /// A PNG of the page at `width`, in the dark or light theme.
    Preview {
        html: String,
        width: u32,
        dark: bool,
    },
}

pub(super) fn list() -> [Value; 2] {
    let html = json!({"type": "string", "minLength": 1, "maxLength": MAX_HTML, "description": "Inline HTML."});
    [
        json!({
            "name": PREVIEW_TOOL,
            "description": format!("Check an HTML page before {RENDER_TOOL}. Returns a PNG and contentHeight. \
                {PAGE_RULES} Theme and layout rules are in {RENDER_TOOL}."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "html": html,
                    "width": {"type": "integer", "minimum": MIN_WIDTH, "maximum": MAX_WIDTH,
                        "description": format!("CSS pixels. Default {COLUMN_WIDTH}.")},
                    "appearance": {"type": "string", "enum": ["dark", "light"], "description": "Default dark."},
                },
                "required": ["html"],
            },
        }),
        json!({
            "name": RENDER_TOOL,
            "description": format!("Show a finished HTML page (chart, diagram, mockup) inline in this thread, \
                above your final reply. Do not announce or restate the page. {PAGE_RULES} {LAYOUT_GUIDE} \
                {THEME_GUIDE}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "html": html,
                    "title": {"type": "string", "minLength": 1, "maxLength": MAX_TITLE},
                    "height": {"type": "integer", "minimum": MIN_HEIGHT, "maximum": MAX_HEIGHT,
                        "description": format!("Frame height in CSS pixels, {MIN_HEIGHT} to {MAX_HEIGHT}. Use the \
                            contentHeight from {PREVIEW_TOOL}.")},
                },
                "required": ["html", "title", "height"],
            },
        }),
    ]
}

pub(super) fn parse(name: &str, args: &Map<String, Value>) -> Option<Result<HtmlTool, String>> {
    Some(match name {
        RENDER_TOOL => (|| {
            Ok(HtmlTool::Render {
                html: page(args)?,
                title: bounded(args, "title", MAX_TITLE)?.trim().to_owned(),
                height: int(args, "height", MIN_HEIGHT, MAX_HEIGHT)?,
            })
        })(),
        PREVIEW_TOOL => (|| {
            Ok(HtmlTool::Preview {
                html: page(args)?,
                width: match args.get("width") {
                    None => COLUMN_WIDTH,
                    Some(_) => int(args, "width", MIN_WIDTH.into(), MAX_WIDTH.into())?,
                },
                dark: match args.get("appearance").map(|v| v.as_str()) {
                    None | Some(Some("dark")) => true,
                    Some(Some("light")) => false,
                    Some(_) => return Err("appearance must be \"dark\" or \"light\"".into()),
                },
            })
        })(),
        _ => return None,
    })
}

/// The `html` field, refused (as T3 does) when it sizes anything by the viewport's height: the page is measured
/// at a small height and drawn at its content's, so a `vh` size measures wrong and draws clipped.
fn page(args: &Map<String, Value>) -> Result<String, String> {
    let html = bounded(args, "html", MAX_HTML)?;
    match viewport_height_unit(&html) {
        Some(found) => Err(format!(
            "the page uses a viewport height unit ({found}). The frame fits the page's content, so let content set \
             the height, or use fixed pixel heights"
        )),
        None => Ok(html),
    }
}

/// The first `<number>vh` (or `svh`, `lvh`, `dvh`) length in `html`, anywhere in it. Not after a letter, digit or
/// `_.+/`, so a base64 run (an inlined image) that happens to hold "1vh" passes.
fn viewport_height_unit(html: &str) -> Option<&str> {
    let b = html.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    for i in 0..b.len().saturating_sub(1) {
        if !b[i..i + 2].eq_ignore_ascii_case(b"vh")
            || b.get(i + 2).is_some_and(|&c| word(c) || c == b'-')
        {
            continue;
        }
        let unit = if i > 0 && matches!(b[i - 1].to_ascii_lowercase(), b's' | b'l' | b'd') {
            i - 1
        } else {
            i
        };
        let mut start = unit;
        while start > 0 && (b[start - 1].is_ascii_digit() || b[start - 1] == b'.') {
            start -= 1;
        }
        let number = &b[start..unit];
        let lead_ok =
            start == 0 || !(word(b[start - 1]) || matches!(b[start - 1], b'.' | b'+' | b'/'));
        if number.iter().any(u8::is_ascii_digit) && lead_ok {
            return Some(&html[start..i + 2]);
        }
    }
    None
}

fn int(args: &Map<String, Value>, field: &str, min: u64, max: u64) -> Result<u32, String> {
    args.get(field)
        .and_then(Value::as_u64)
        .filter(|n| (min..=max).contains(n))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("{field} must be an integer from {min} to {max}"))
}

/// A stored `html_render` call that came back without an error: the timeline draws it as the page, outside the
/// turn's fold. The name is the agent's (`mcp__skyelab__html_render`, Claude's and Codex's form).
pub fn is_rendered_page(tool: &crate::timeline::ToolItem) -> bool {
    tool.name.strip_prefix("mcp__skyelab__") == Some(RENDER_TOOL)
        && tool.result.is_some()
        && !tool.is_error
}

#[cfg(test)]
mod tests;
