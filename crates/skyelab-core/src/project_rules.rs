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

/// Words as the shell splits them (ASCII space and tab only: a no-break space is part of a word to bash).
fn words(command: &str) -> impl Iterator<Item = &str> {
    command.split([' ', '\t']).filter(|w| !w.is_empty())
}

/// Programs that run whatever their arguments say (shells, interpreters, wrappers): a prefix starting with one
/// would allow any code.
const RUNS_ANYTHING: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "pwsh",
    "powershell",
    "cmd",
    "env",
    "xargs",
    "sudo",
    "doas",
    "su",
    "eval",
    "exec",
    "command",
    "builtin",
    "nohup",
    "time",
    "timeout",
    "nice",
    "ionice",
    "watch",
    "stdbuf",
    "setsid",
    "chroot",
    "python",
    "python2",
    "python3",
    "node",
    "deno",
    "bun",
    "ruby",
    "perl",
    "php",
    "lua",
    "osascript",
    "awk",
    "gawk",
    "sed",
    "find",
];

/// Why a prefix of plain words still allows too much, if it does.
fn wide_prefix(prefix: &str) -> Option<String> {
    let first = words(prefix).next()?;
    let name = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .trim_end_matches(".exe");
    if RUNS_ANYTHING.contains(&name.to_ascii_lowercase().as_str()) {
        return Some(format!(
            "{first} runs whatever its arguments say, so it can only be allowed exactly."
        ));
    }
    words(prefix)
        .last()
        .filter(|w| w.starts_with('-'))
        .map(|w| format!("A prefix can't end in a flag ({w}): the next word could be code. Leave the flag out."))
}

/// Folders and files an agent may never change by a rule: git internals (a hook runs on SkyeLab's own git calls)
/// and agent and editor configuration. Compared case-insensitively (macOS and Windows folders are).
const PROTECTED: &[&str] = &[
    ".git",
    ".claude",
    ".codex",
    ".mcp.json",
    ".envrc",
    ".vscode",
    ".idea",
    ".husky",
];

fn protected(rel: &str) -> bool {
    rel.split('/')
        .any(|part| PROTECTED.iter().any(|p| p.eq_ignore_ascii_case(part)))
}

/// The input fields a shell ask may carry and still be matched: anything else (Claude's
/// `dangerouslyDisableSandbox`, Codex's `reason`, which is how it asks to retry a command outside its sandbox, a field
/// from a newer agent) may change what running it means.
const COMMAND_FIELDS: &[&str] = &[
    "command",
    "description",
    "timeout",
    "run_in_background",
    "cwd",
];

fn plain_command_ask(input: &Value) -> bool {
    input
        .as_object()
        .is_some_and(|o| o.keys().all(|k| COMMAND_FIELDS.contains(&k.as_str())))
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
        // A dangling symlink doesn't canonicalize either, but writing through it lands wherever it points.
        if existing.symlink_metadata().is_ok() {
            return None;
        }
        tail.push(existing.file_name()?.to_owned());
        existing = existing.parent()?.to_path_buf();
    };
    let real: PathBuf = resolved.join(tail.iter().rev().collect::<PathBuf>());
    let rel = real.strip_prefix(&root_real).ok()?;
    let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
    let parts = parts?;
    // `..` can only be here from a verbatim Windows path, which keeps it as a name.
    (!parts.is_empty() && !parts.contains(&"..")).then(|| parts.join("/"))
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
        return (!command.is_empty() && cwd_inside(input, root) && plain_command_ask(input))
            .then(|| rule(Kind::Exact, command.to_owned()));
    }
    if let Some(key) = file_key(tool) {
        let rel = relative_inside(root, Path::new(input[key].as_str()?))?;
        // A `*` in the name would be read as a glob, covering other files.
        return (!protected(&rel) && !rel.contains('*')).then(|| rule(Kind::Path, rel));
    }
    mcp_tool(tool).then(|| rule(Kind::Tool, String::new()))
}

/// The prefix the dialog suggests for a command: its first two words, or one when the second is a flag, when the
/// command may take a prefix rule at all.
pub fn suggested_prefix(command: &str) -> Option<String> {
    if !simple(command) {
        return None;
    }
    let mut picked: Vec<&str> = words(command).take(2).collect();
    while picked.last().is_some_and(|w| w.starts_with('-')) {
        picked.pop();
    }
    let prefix = picked.join(" ");
    (!prefix.is_empty() && wide_prefix(&prefix).is_none()).then_some(prefix)
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
            if let Some(why) = wide_prefix(p) {
                return Err(why);
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
            if protected(p) {
                return Err(
                    "Rules never cover .git or agent and editor settings (.claude, .mcp.json, …)."
                        .into(),
                );
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
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    let one = |p: &&str, s: &&str| wild(p.as_bytes(), s.as_bytes(), |c| *c == b'*', |a, b| a == b);
    wild(&p, &s, |seg| *seg == "**", one)
}

/// Wildcard match in O(len p × len s): `star` items match any run (also empty), the rest one item each by `same`.
/// Greedy with one backtrack point, so many stars never go exponential.
fn wild<T>(p: &[T], s: &[T], star: impl Fn(&T) -> bool, same: impl Fn(&T, &T) -> bool) -> bool {
    let (mut i, mut j) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while j < s.len() {
        if i < p.len() && star(&p[i]) {
            back = Some((i, j));
            i += 1;
        } else if i < p.len() && same(&p[i], &s[j]) {
            i += 1;
            j += 1;
        } else if let Some((bi, bj)) = back {
            (i, j) = (bi + 1, bj + 1);
            back = Some((bi, bj + 1));
        } else {
            return false;
        }
    }
    p[i..].iter().all(star)
}

/// Whether `rule` allows this ask from an agent working in `root`.
pub fn matches(rule: &Rule, tool: &str, input: &Value, root: &Path) -> bool {
    if rule.tool != tool || validate(rule).is_err() {
        return false;
    }
    let pattern = rule.pattern.trim();
    match rule.kind {
        Kind::Exact | Kind::Prefix => {
            let Some(raw) = input["command"].as_str() else {
                return false;
            };
            if !cwd_inside(input, root) || !plain_command_ask(input) {
                return false;
            }
            if rule.kind == Kind::Exact {
                return raw.trim() == pattern;
            }
            let want: Vec<&str> = words(pattern).collect();
            // The untrimmed command: a trailing newline is a second line to the shell.
            simple(raw) && words(raw).take(want.len()).eq(want.iter().copied())
        }
        Kind::Path => {
            let Some(key) = file_key(tool) else {
                return false;
            };
            input[key]
                .as_str()
                .and_then(|f| relative_inside(root, Path::new(f)))
                .is_some_and(|rel| !protected(&rel) && glob(pattern, &rel))
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
