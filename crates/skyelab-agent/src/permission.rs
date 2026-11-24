//! Which tool calls an agent may make without asking, and how the user answers when it asks.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A thread's permission mode. Stored on the thread row as [`Mode::as_str`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The agent asks before anything its own rules don't already allow.
    #[default]
    Default,
    /// Read and plan only, no edits or commands.
    Plan,
    /// File edits in the folder go ahead; anything else asks.
    AcceptEdits,
    /// Nothing asks. Claude skips every check; Codex leaves its sandbox.
    Bypass,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Default, Mode::Plan, Mode::AcceptEdits, Mode::Bypass];

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Default => "default",
            Mode::Plan => "plan",
            Mode::AcceptEdits => "accept_edits",
            Mode::Bypass => "bypass",
        }
    }

    /// An unknown value (a newer build's) reads as Ask.
    pub fn parse(s: &str) -> Mode {
        Mode::ALL
            .into_iter()
            .find(|m| m.as_str() == s)
            .unwrap_or_default()
    }

    /// Claude's `--permission-mode` name.
    pub(crate) fn claude(self) -> &'static str {
        match self {
            Mode::Default => "default",
            Mode::Plan => "plan",
            Mode::AcceptEdits => "acceptEdits",
            Mode::Bypass => "bypassPermissions",
        }
    }
}

/// The user's answer to a permission request, or why it no longer needs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    AllowOnce,
    /// Allow this call and its like for as long as this agent process runs.
    AllowThread,
    Deny,
    /// The agent withdrew the request (an interrupt, or its turn ended) before anyone answered.
    Withdrawn,
}

const SUMMARY_MAX: usize = 200;

/// One line saying what a tool call does: its command, the file it touches, or the folder it wants to write in,
/// else its input as JSON. At most [`SUMMARY_MAX`] characters.
pub fn summary(input: &Value) -> String {
    let text = [
        "command",
        "file_path",
        "path",
        "url",
        "pattern",
        "query",
        "changes",
        "grantRoot",
        "reason",
    ]
    .iter()
    .find_map(|k| match &input[k] {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                // A list of strings (argv), or of Codex's file changes, each with a path.
                .filter_map(|p| p.as_str().or_else(|| p["path"].as_str()))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    })
    .unwrap_or_else(|| input.to_string());
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(SUMMARY_MAX) {
        Some(_) => {
            let cut = line
                .char_indices()
                .nth(SUMMARY_MAX - 1)
                .map_or(0, |(at, _)| at);
            format!("{}…", &line[..cut])
        }
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mode_round_trips_and_unknown_is_default() {
        for m in Mode::ALL {
            assert_eq!(Mode::parse(m.as_str()), m);
        }
        assert_eq!(Mode::parse("yolo"), Mode::Default);
    }

    #[test]
    fn summary_prefers_the_command_and_is_capped() {
        assert_eq!(summary(&json!({"command": "ls\n  -la"})), "ls -la");
        assert_eq!(
            summary(&json!({"file_path": "/a/b", "content": "x"})),
            "/a/b"
        );
        assert_eq!(summary(&json!({"x": 1})), r#"{"x":1}"#);
        let long = summary(&json!({ "command": "a".repeat(500) }));
        assert_eq!(long.chars().count(), 200);
        let exact = summary(&json!({ "command": "b".repeat(200) }));
        assert_eq!(exact, "b".repeat(200), "fits, so not cut");
        assert_eq!(
            summary(&json!({ "grantRoot": "/Users/x", "reason": "r" })),
            "/Users/x"
        );
    }
}
