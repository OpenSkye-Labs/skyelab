//! The browser tools: an agent drives browser tabs its own thread opened. Input is checked here for shape and
//! size; the app checks the URL scheme (http and https only) and which tab the thread owns.

use super::tools::bounded;
use serde_json::{Map, Value, json};

/// Longest URL an agent may pass.
pub(super) const MAX_URL: usize = 2048;
/// Longest JS expression an agent may evaluate.
pub(super) const MAX_EXPRESSION: usize = 32_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserTool {
    /// Opens this thread's tab on `url`, or navigates it if the thread already has one.
    Open {
        url: String,
    },
    Navigate {
        url: String,
    },
    Url,
    Eval {
        expression: String,
    },
}

pub(super) fn list() -> [Value; 4] {
    let url = json!({"type": "string", "minLength": 1, "maxLength": MAX_URL, "description": "An http or https URL."});
    [
        json!({
            "name": "browser_open",
            "description": "Open a SkyeLab browser tab on an http or https URL (localhost is fine). A thread has one \
                tab: if yours is open, this navigates it. The user sees that you control it and can take it over.",
            "inputSchema": {"type": "object", "properties": {"url": url}, "required": ["url"]},
        }),
        json!({
            "name": "browser_navigate",
            "description": "Load an http or https URL in your browser tab.",
            "inputSchema": {"type": "object", "properties": {"url": url}, "required": ["url"]},
        }),
        json!({
            "name": "browser_url",
            "description": "The URL your browser tab is on.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "browser_eval",
            "description": "Evaluate a JavaScript expression in your browser tab's page and return its JSON value. \
                Promises are not awaited. Large results are refused; return only what you need.",
            "inputSchema": {
                "type": "object",
                "properties": {"expression": {"type": "string", "minLength": 1, "maxLength": MAX_EXPRESSION}},
                "required": ["expression"],
            },
        }),
    ]
}

pub(super) fn parse(name: &str, args: &Map<String, Value>) -> Option<Result<BrowserTool, String>> {
    Some(match name {
        "browser_open" => bounded(args, "url", MAX_URL).map(|url| BrowserTool::Open { url }),
        "browser_navigate" => {
            bounded(args, "url", MAX_URL).map(|url| BrowserTool::Navigate { url })
        }
        "browser_url" => Ok(BrowserTool::Url),
        "browser_eval" => bounded(args, "expression", MAX_EXPRESSION)
            .map(|expression| BrowserTool::Eval { expression }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
