//! The user's keybindings file: rules layered over SkyeLab's built-in bindings.
//!
//! ```toml
//! [[binding]]
//! key = "cmd-shift-t"          # GPUI keystrokes; `secondary-` is cmd on macOS, ctrl elsewhere
//! action = "NewTerminal"       # or "skyelab::NewTerminal"; "none" frees the key
//! context = "Workspace"        # optional, the default
//! ```
//!
//! This parses the file's shape only. The app checks that the action, keys and context exist (GPUI knows them).

use serde::Deserialize;
use std::path::PathBuf;

/// The context a rule without one binds in: the whole window.
pub const DEFAULT_CONTEXT: &str = "Workspace";
/// The action name that unbinds a key.
pub const UNBIND: &str = "none";

/// One `[[binding]]` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// 1-based line of the rule's `[[binding]]` header.
    pub line: usize,
    pub key: String,
    /// Full name (`skyelab::NewThread`), or [`UNBIND`].
    pub action: String,
    pub context: String,
}

/// Why a rule (or the whole file) was skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub reason: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("skyelab").join("keybindings.toml"))
}

/// What "Open keybindings file" writes when there is no file yet.
pub const TEMPLATE: &str = r#"# SkyeLab keybindings. Rules here are added after the built-in ones, so they win.
# Settings > Keyboard shortcuts lists every action and its current keys.
#
# [[binding]]
# key = "secondary-shift-t"   # secondary is cmd on macOS, ctrl elsewhere
# action = "NewTerminal"      # an action name; "none" frees the key
# context = "Workspace"       # optional: Workspace (default), Thread, Terminal, Settings, ...
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    binding: Vec<toml::Spanned<toml::Table>>,
}

fn line_at(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

/// The rules in `text`, in file order, and every problem found. A broken file gives no rules and one problem;
/// a broken rule is skipped and the rest still load.
pub fn parse(text: &str) -> (Vec<Rule>, Vec<Problem>) {
    let file: File = match toml::from_str(text) {
        Ok(file) => file,
        Err(e) => {
            let line = e.span().map_or(1, |s| line_at(text, s.start));
            let reason = e.message().trim().to_owned();
            return (Vec::new(), vec![Problem { line, reason }]);
        }
    };
    let (mut rules, mut problems) = (Vec::new(), Vec::new());
    for entry in file.binding {
        let line = line_at(text, entry.span().start);
        match rule(entry.get_ref(), line) {
            Ok(rule) => rules.push(rule),
            Err(reason) => problems.push(Problem { line, reason }),
        }
    }
    (rules, problems)
}

fn rule(table: &toml::Table, line: usize) -> Result<Rule, String> {
    if let Some(extra) = table
        .keys()
        .find(|k| !["key", "action", "context"].contains(&k.as_str()))
    {
        return Err(format!(
            "unknown field `{extra}` (use key, action, context)"
        ));
    }
    let field = |name: &str| match table.get(name) {
        None => Ok(None),
        Some(toml::Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_owned())),
        Some(_) => Err(format!("`{name}` must be a non-empty string")),
    };
    let key = field("key")?.ok_or("missing `key`")?;
    let action = field("action")?.ok_or("missing `action`")?;
    let action = if action == UNBIND || action.contains("::") {
        action
    } else {
        format!("skyelab::{action}")
    };
    let context = field("context")?.unwrap_or_else(|| DEFAULT_CONTEXT.to_owned());
    Ok(Rule {
        line,
        key,
        action,
        context,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules_with_lines_and_defaults() {
        let text = "# mine\n[[binding]]\nkey = \"cmd-shift-t\"\naction = \"NewTerminal\"\n\n[[binding]]\nkey = \"f6\"\naction = \"skyelab::OpenSettings\"\ncontext = \"Terminal\"\n";
        let (rules, problems) = parse(text);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            rules,
            vec![
                Rule {
                    line: 2,
                    key: "cmd-shift-t".into(),
                    action: "skyelab::NewTerminal".into(),
                    context: DEFAULT_CONTEXT.into(),
                },
                Rule {
                    line: 6,
                    key: "f6".into(),
                    action: "skyelab::OpenSettings".into(),
                    context: "Terminal".into(),
                },
            ]
        );
    }

    #[test]
    fn a_bad_rule_is_skipped_and_reported_with_its_line() {
        let text = "[[binding]]\nkey = \"a\"\naction = \"none\"\n\n[[binding]]\nkey = \"b\"\n\n[[binding]]\nkey = \"c\"\naction = \"X\"\nwhen = \"y\"\n";
        let (rules, problems) = parse(text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].action, UNBIND);
        assert_eq!(
            problems,
            vec![
                Problem {
                    line: 5,
                    reason: "missing `action`".into()
                },
                Problem {
                    line: 8,
                    reason: "unknown field `when` (use key, action, context)".into()
                },
            ]
        );
    }

    #[test]
    fn a_broken_file_loads_nothing_and_says_where() {
        let (rules, problems) = parse("[[binding]]\nkey = \"a\"\naction = \n");
        assert!(rules.is_empty());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, 3);
        let (_, problems) = parse("[[bindings]]\nkey = \"a\"\n");
        assert_eq!(
            problems.len(),
            1,
            "a misspelled table is reported, not ignored"
        );
    }

    #[test]
    fn non_string_fields_are_reported() {
        let (_, problems) = parse("[[binding]]\nkey = 6\naction = \"X\"\n");
        assert_eq!(problems[0].reason, "`key` must be a non-empty string");
    }

    #[test]
    fn the_template_has_no_rules_and_no_problems() {
        assert_eq!(parse(TEMPLATE), (Vec::new(), Vec::new()));
        assert_eq!(parse(""), (Vec::new(), Vec::new()));
    }
}
