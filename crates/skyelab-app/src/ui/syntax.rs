//! Syntax colors for code drawn line by line (diff rows): syntect, with the syntax set gpui-component's highlighter
//! uses, naming each span with the theme's highlight names (`keyword`, `string`) so colors follow the theme.

use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// Loaded on the first highlight (tens of ms), then shared.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// One line's spans: byte ranges into the line and the theme highlight name for each.
pub type Spans = Vec<(Range<usize>, &'static str)>;

/// The highlighter's language for a file: its extension, or its name for `Makefile`, `Dockerfile` and the like.
pub fn language(path: &Path) -> String {
    path.extension()
        .or_else(|| path.file_name())
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn syntax(language: &str) -> Option<&'static SyntaxReference> {
    SYNTAXES
        .find_syntax_by_token(language)
        .filter(|s| s.name != "Plain Text")
}

/// Spans for each of `lines`, parsed in order so a construct that spans lines (a block comment, a docstring) colors
/// through. Empty spans for a language syntect doesn't know.
pub fn highlight<'a>(language: &str, lines: impl IntoIterator<Item = &'a str>) -> Vec<Spans> {
    let Some(syntax) = syntax(language) else {
        return lines.into_iter().map(|_| Spans::new()).collect();
    };
    let mut parser = ParseState::new(syntax);
    let mut scopes = ScopeStack::new();
    let mut line_buf = String::new();
    lines
        .into_iter()
        .map(|line| {
            line_buf.clear();
            line_buf.push_str(line);
            line_buf.push('\n');
            let mut spans = Spans::new();
            // A line syntect can't parse stays plain; the next starts from the scopes it had.
            let Ok(ops) = parser.parse_line(&line_buf, &SYNTAXES) else {
                return spans;
            };
            let mut push = |r: Range<usize>, scopes: &ScopeStack| {
                let r = r.start.min(line.len())..r.end.min(line.len());
                if r.is_empty() {
                    return;
                }
                // The innermost scope that names something wins: a string's quotes color as the string.
                if let Some(name) = scopes.scopes.iter().rev().find_map(|s| name(*s)) {
                    match spans.last_mut() {
                        Some((last, n)) if last.end == r.start && *n == name => last.end = r.end,
                        _ => spans.push((r, name)),
                    }
                }
            };
            let mut at = 0;
            for (ix, op) in ops {
                push(at..ix, &scopes);
                let _ = scopes.apply(&op);
                at = ix;
            }
            push(at..line.len(), &scopes);
            spans
        })
        .collect()
}

/// A TextMate scope as one of the theme's highlight names, or `None` for containers with no color of their own.
fn name(scope: Scope) -> Option<&'static str> {
    let s = scope.build_string();
    let is = |p: &str| s == p || s.strip_prefix(p).is_some_and(|r| r.starts_with('.'));
    Some(match () {
        _ if is("punctuation.definition.string") || is("punctuation.definition.comment") => {
            return None;
        }
        _ if is("meta") || is("source") || is("text") || is("invalid") => return None,
        _ if is("comment") => "comment",
        _ if is("constant.character.escape") => "string.escape",
        _ if is("string") => "string",
        _ if is("constant.numeric") => "number",
        _ if is("constant.language.boolean") => "boolean",
        _ if is("constant") || is("support.constant") => "constant",
        _ if is("keyword.operator") => "operator",
        _ if is("keyword") || is("storage.modifier") => "keyword",
        // `fn`, `class`, `def`, `let` are declarations; `int` and `bool` are types.
        _ if is("storage.type") => {
            const DECL: [&str; 9] = [
                "class", "function", "impl", "struct", "trait", "type", "let", "const", "enum",
            ];
            if DECL.iter().any(|w| s.contains(w)) || s.ends_with(".def") {
                "keyword"
            } else {
                "type"
            }
        }
        _ if is("entity.name.function") || is("support.function") || is("variable.function") => {
            "function"
        }
        _ if is("entity.name") || is("entity.other.inherited-class") || is("support.type") => {
            "type"
        }
        _ if is("support.class") => "type",
        _ if is("entity.other.attribute-name") => "attribute",
        _ if is("variable.language") => "variable.special",
        _ if is("variable.other.member") || is("variable.other.property") => "property",
        _ if is("variable.parameter") => "variable",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_lines_get_keyword_string_and_comment_names() {
        let lines = ["def add(a, b):", "    return \"x\"  # sum"];
        let spans = highlight("py", lines);
        let named = |line: usize, text: &str| {
            let start = lines[line].find(text).expect("text in line");
            spans[line]
                .iter()
                .find(|(r, _)| r.contains(&start))
                .map(|s| s.1)
        };
        assert_eq!(named(0, "def"), Some("keyword"));
        assert_eq!(named(0, "add"), Some("function"));
        assert_eq!(named(1, "return"), Some("keyword"));
        assert_eq!(named(1, "\"x\""), Some("string"));
        assert_eq!(named(1, "# sum"), Some("comment"));
    }

    #[test]
    fn a_block_comment_colors_across_lines_and_unknown_languages_stay_plain() {
        let spans = highlight("rs", ["/* one", "two */ fn x() {}"]);
        assert_eq!(spans[1].first().map(|s| s.1), Some("comment"));
        assert!(
            highlight("no-such-language", ["a", "b"])
                .iter()
                .all(Vec::is_empty)
        );
    }
}
