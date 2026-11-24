//! "Allow for this project": narrow allow rules the user saves from a permission ask, kept in SkyeLab's own store per
//! project and per account (never in the repo or the account's agent settings). SkyeLab matches each later ask
//! itself and answers it, so the rule grammar is ours: a command rule is the exact command or a word prefix of a
//! simple command (no `;`, `&&`, pipes, substitution or redirects), a file rule is a glob inside the thread's folder,
//! and an MCP rule names one tool.

use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// How a rule's pattern is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The command, byte for byte (ends trimmed).
    Exact,
    /// A simple command whose first words are the pattern's words.
    Prefix,
    /// A `/`-separated glob relative to the thread's folder: `*` within a segment, `**` across segments.
    Path,
    /// Any call to the tool (MCP tools only).
    Tool,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Exact => "exact",
            Kind::Prefix => "prefix",
            Kind::Path => "path",
            Kind::Tool => "tool",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        [Kind::Exact, Kind::Prefix, Kind::Path, Kind::Tool]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub tool: String,
    pub kind: Kind,
    pub pattern: String,
}

/// Claude's `Bash` and Codex's `shell`.
fn command_tool(tool: &str) -> bool {
    matches!(tool, "Bash" | "shell")
}

/// The input key holding the file a file tool writes.
fn file_key(tool: &str) -> Option<&'static str> {
    match tool {
        "Edit" | "Write" | "MultiEdit" => Some("file_path"),
        "NotebookEdit" => Some("notebook_path"),
        _ => None,
    }
}

fn mcp_tool(tool: &str) -> bool {
    tool.len() > "mcp__".len() && tool.starts_with("mcp__")
}

/// Whether "Allow for this project" can cover a call to `tool` at all: a shell command, a file write, an MCP tool.
pub fn offers(tool: &str) -> bool {
    command_tool(tool) || file_key(tool).is_some() || mcp_tool(tool)
}

/// A command a prefix may cover: one plain command, nothing that chains, substitutes, redirects or escapes.
fn simple(command: &str) -> bool {
    !command.trim().is_empty()
        && !command
            .chars()
            .any(|c| c.is_control() || ";&|`$<>()\\".contains(c))
}

/// `path` relative to `root`, `/`-separated, when it is inside `root` by its text and after resolving symlinks on
/// the part that exists (a file being written may not exist yet). `None` when it escapes or isn't UTF-8.
pub fn relative_inside(root: &Path, path: &Path) -> Option<String> {
    // By text first, so an escaping path is never touched (a network share would be reached).
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let full = if path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_)))
    {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let root_real = root.canonicalize().ok()?;
    let rest = full
        .strip_prefix(root)
        .or_else(|_| full.strip_prefix(&root_real))
        .ok()?
        .to_path_buf();
    // The deepest part that exists, resolved, must still be inside: a symlinked folder can point out.
    let mut existing = root_real.join(&rest);
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let resolved = loop {
        if let Ok(real) = existing.canonicalize() {
            break real;
        }
        tail.push(existing.file_name()?.to_owned());
        existing = existing.parent()?.to_path_buf();
    };
    let real: PathBuf = resolved.join(tail.iter().rev().collect::<PathBuf>());
    let rel = real.strip_prefix(&root_real).ok()?;
    let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
    let parts = parts?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Whether the agent's working folder in this ask (Codex's `cwd`) is the folder or under it; no `cwd` is the folder.
fn cwd_inside(input: &Value, root: &Path) -> bool {
    match &input["cwd"] {
        Value::Null => true,
        Value::String(cwd) => {
            cwd.trim_end_matches('/') == root.to_string_lossy().trim_end_matches('/')
                || relative_inside(root, Path::new(cwd)).is_some()
        }
        _ => false,
    }
}

/// The rule the dialog starts from for this ask, or `None` when no project rule can cover it (another tool, a file
/// outside the folder, a command with no text).
pub fn propose(tool: &str, input: &Value, root: &Path) -> Option<Rule> {
    let rule = |kind, pattern: String| Rule {
        tool: tool.to_owned(),
        kind,
        pattern,
    };
    if command_tool(tool) {
        let command = input["command"].as_str()?.trim();
        return (!command.is_empty() && cwd_inside(input, root))
            .then(|| rule(Kind::Exact, command.to_owned()));
    }
    if let Some(key) = file_key(tool) {
        let rel = relative_inside(root, Path::new(input[key].as_str()?))?;
        return Some(rule(Kind::Path, rel));
    }
    mcp_tool(tool).then(|| rule(Kind::Tool, String::new()))
}

/// The prefix the dialog suggests for a command: its first two words, when it is simple enough to take one.
pub fn suggested_prefix(command: &str) -> Option<String> {
    simple(command).then(|| {
        command
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// Why `rule` can't be saved, in words for the user.
pub fn validate(rule: &Rule) -> Result<(), String> {
    let p = rule.pattern.trim();
    match rule.kind {
        Kind::Exact if command_tool(&rule.tool) => {
            if p.is_empty() {
                return Err("Enter the command.".into());
            }
        }
        Kind::Prefix if command_tool(&rule.tool) => {
            if p.is_empty() {
                return Err("Enter the start of the command.".into());
            }
            if !simple(p) {
                return Err(
                    "A prefix must be one plain command: no ; & | $ ` < > ( ) or backslashes. Allow a compound \
                     command exactly instead."
                        .into(),
                );
            }
        }
        Kind::Path if file_key(&rule.tool).is_some() => {
            if p.is_empty() {
                return Err("Enter a path inside the project.".into());
            }
            let bad_start = p.starts_with('/')
                || p.starts_with('~')
                || p.contains('\\')
                || p.as_bytes().get(1) == Some(&b':');
            if bad_start || p.split('/').any(|s| s == ".." || s == "." || s.is_empty()) {
                return Err(
                    "Use a path relative to the project, like src/** or docs/*.md (no .., no leading /)."
                        .into(),
                );
            }
            if p.chars().any(char::is_control) {
                return Err("The path has control characters.".into());
            }
        }
        Kind::Tool if mcp_tool(&rule.tool) => {
            if !p.is_empty() {
                return Err("An MCP rule takes no pattern.".into());
            }
        }
        _ => return Err(format!("{} can't take this kind of rule.", rule.tool)),
    }
    Ok(())
}

/// `*` matches within one segment, `**` any number of whole segments; anything else matches itself.
fn glob(pattern: &str, path: &str) -> bool {
    fn segs(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => segs(&p[1..], s) || (!s.is_empty() && segs(p, &s[1..])),
            (Some(a), Some(b)) => seg(a.as_bytes(), b.as_bytes()) && segs(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn seg(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg(&p[1..], s) || (!s.is_empty() && seg(p, &s[1..])),
            (Some(a), Some(b)) => a == b && seg(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    segs(&p, &s)
}

/// Whether `rule` allows this ask from an agent working in `root`.
pub fn matches(rule: &Rule, tool: &str, input: &Value, root: &Path) -> bool {
    if rule.tool != tool || validate(rule).is_err() {
        return false;
    }
    let pattern = rule.pattern.trim();
    match rule.kind {
        Kind::Exact | Kind::Prefix => {
            let Some(command) = input["command"].as_str().map(str::trim) else {
                return false;
            };
            if !cwd_inside(input, root) {
                return false;
            }
            if rule.kind == Kind::Exact {
                return command == pattern;
            }
            let want: Vec<&str> = pattern.split_whitespace().collect();
            simple(command)
                && command
                    .split_whitespace()
                    .take(want.len())
                    .eq(want.iter().copied())
        }
        Kind::Path => {
            let Some(key) = file_key(tool) else {
                return false;
            };
            input[key]
                .as_str()
                .and_then(|f| relative_inside(root, Path::new(f)))
                .is_some_and(|rel| glob(pattern, &rel))
        }
        Kind::Tool => true,
    }
}

/// The first of `rules` that allows this ask.
pub fn first_match<'a>(
    rules: &'a [Rule],
    tool: &str,
    input: &Value,
    root: &Path,
) -> Option<&'a Rule> {
    rules.iter().find(|r| matches(r, tool, input, root))
}

/// The rule in a few words: "Bash · starts with npm test".
pub fn label(rule: &Rule) -> String {
    let what = match rule.kind {
        Kind::Exact => format!("exactly {}", rule.pattern),
        Kind::Prefix => format!("starts with {}", rule.pattern),
        Kind::Path => rule.pattern.clone(),
        Kind::Tool => "any call".to_owned(),
    };
    format!("{} · {what}", rule.tool)
}

#[cfg(test)]
#[path = "project_rules_test.rs"]
mod tests;
