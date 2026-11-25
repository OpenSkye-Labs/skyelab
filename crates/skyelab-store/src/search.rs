//! Thread search over titles and message text: every typed word must match, quoted text is one phrase.

use anyhow::Result;
use rusqlite::types::Value;

use crate::{Store, THREAD_COLS, Thread, thread_from_row};

/// A thread that matched, with the matched line of its newest matching message (none for a title-only match).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub thread: Thread,
    pub snippet: Option<String>,
}

/// The most terms a query keeps, and the most characters a term keeps: each term is one SQL condition, and SQLite
/// refuses an expression over 1,000 deep or a LIKE pattern over 50,000 bytes (a pasted log would hit both).
const MAX_TERMS: usize = 32;
const MAX_TERM_CHARS: usize = 256;

/// The query's terms: text inside double quotes is one term (an unclosed quote runs to the end), the rest is split
/// on whitespace. Repeats are dropped, and only the first [`MAX_TERMS`], each cut to [`MAX_TERM_CHARS`], are kept.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    let parts = query
        .split('"')
        .enumerate()
        .flat_map(|(i, part)| match i % 2 {
            1 => vec![part.trim()],
            _ => part.split_whitespace().collect(),
        });
    for part in parts.filter(|t| !t.is_empty()) {
        let term: String = part.chars().take(MAX_TERM_CHARS).collect();
        if !terms
            .iter()
            .any(|t| t.to_lowercase() == term.to_lowercase())
        {
            terms.push(term);
        }
        if terms.len() == MAX_TERMS {
            break;
        }
    }
    terms
}

/// The trigram index needs three characters; shorter terms are checked as substrings instead.
fn indexed(term: &str) -> bool {
    term.chars().count() >= 3
}

fn like(term: &str) -> Value {
    let escaped = term
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Value::Text(format!("%{escaped}%"))
}

impl Store {
    /// Threads whose title holds every term of `query`, or one of whose messages (prompts, assistant text) does,
    /// most recently updated first, in `project` or in every project. Deleted threads, side chats and removed
    /// projects are left out. When every term is under three characters only titles are searched.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[tracing::instrument(name = "store.search_threads", skip_all)]
    pub fn search_threads(
        &self,
        query: &str,
        project: Option<i64>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let terms = query_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec![
            project.map_or(Value::Null, Value::Integer),
            Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)),
        ];
        let mut arg = |v: Value| {
            args.push(v);
            format!("?{}", args.len())
        };
        let title = terms
            .iter()
            .map(|t| format!("t.title LIKE {} ESCAPE '\\'", arg(like(t))))
            .collect::<Vec<_>>()
            .join(" AND ");
        // Each indexed term is a quoted FTS5 string, so its text is never read as syntax (NEAR, *, -, :, ^).
        let mut phrases: Vec<String> = terms
            .iter()
            .filter(|t| indexed(t))
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        // Only short words (`go to`, `PR #4`): the index can still find them written together.
        let whole = terms.join(" ");
        if phrases.is_empty() && indexed(&whole) {
            phrases.push(format!("\"{}\"", whole.replace('"', "\"\"")));
        }
        let (hits, hit_row) = if phrases.is_empty() {
            (String::new(), "NULL")
        } else {
            let mut cond = format!("event_text MATCH {}", arg(Value::Text(phrases.join(" "))));
            for t in terms.iter().filter(|t| !indexed(t)) {
                cond += &format!(" AND body LIKE {} ESCAPE '\\'", arg(like(t)));
            }
            (
                format!(
                    "LEFT JOIN (SELECT thread_id, max(rowid) AS row FROM event_text WHERE {cond} GROUP BY thread_id) h
                     ON h.thread_id = t.id"
                ),
                "h.row",
            )
        };
        let cols = THREAD_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {cols}, {hit_row} FROM threads t JOIN projects p ON p.id = t.project_id {hits}
             WHERE t.deleted_at IS NULL AND t.side_of IS NULL AND p.removed_at IS NULL
               AND (?1 IS NULL OR t.project_id = ?1)
               AND (({title}) OR {hit_row} IS NOT NULL)
             ORDER BY t.updated_at DESC, t.id DESC LIMIT ?2"
        );
        let found: Vec<(Thread, Option<i64>)> = self
            .conn
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((thread_from_row(r)?, r.get(25)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        // Bodies are read for the listed hits only, after the limit.
        let mut body = self
            .conn
            .prepare_cached("SELECT body FROM event_text WHERE rowid = ?1")?;
        found
            .into_iter()
            .map(|(thread, row)| {
                let snippet = match row {
                    Some(row) => Some(snippet(
                        &body.query_row([row], |r| r.get::<_, String>(0))?,
                        &terms,
                    )),
                    None => None,
                };
                Ok(SearchHit { thread, snippet })
            })
            .collect()
    }
}

/// How far before the match a snippet starts, so a one-line row still shows the match.
const SNIPPET_LEAD: usize = 24;
/// A snippet's most characters, ellipses included.
const SNIPPET_LEN: usize = 120;

/// Where `needle` first appears in `hay` ignoring case, as a byte index.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let lower = |s: &str| s.chars().flat_map(char::to_lowercase).collect::<Vec<_>>();
    let needle = lower(needle);
    hay.char_indices().map(|(i, _)| i).find(|&i| {
        let mut rest = hay[i..].chars().flat_map(char::to_lowercase);
        needle.iter().all(|c| rest.next() == Some(*c))
    })
}

/// One line of `body` around the first term it holds: from a word start a little before it, whitespace collapsed,
/// emphasis and code marks dropped, `…` where it was cut.
fn snippet(body: &str, terms: &[String]) -> String {
    let hit = terms
        .iter()
        .filter_map(|t| find_ci(body, t))
        .min()
        .unwrap_or(0);
    let mut start = body[..hit]
        .char_indices()
        .rev()
        .nth(SNIPPET_LEAD - 1)
        .map_or(0, |(i, _)| i);
    if start > 0
        && let Some((i, c)) = body[start..hit]
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
    {
        start += i + c.len_utf8();
    }
    let text = body[start..].replace("**", "").replace('`', "");
    let mut words = text.split_whitespace();
    let budget = SNIPPET_LEN - usize::from(start > 0) - 1;
    let mut out = String::new();
    let mut cut = false;
    for word in words.by_ref() {
        let sep = usize::from(!out.is_empty());
        if out.chars().count() + sep + word.chars().count() > budget {
            let room = budget.saturating_sub(out.chars().count() + sep);
            if room > 0 {
                out.push_str(&" ".repeat(sep));
                out.extend(word.chars().take(room));
            }
            cut = true;
            break;
        }
        out.push_str(&" ".repeat(sep));
        out.push_str(word);
    }
    format!(
        "{}{out}{}",
        if start > 0 { "…" } else { "" },
        if cut { "…" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::{find_ci, query_terms, snippet};

    #[test]
    fn terms() {
        assert_eq!(query_terms("  foo   bar "), ["foo", "bar"]);
        assert_eq!(query_terms("a \"b c\" d"), ["a", "b c", "d"]);
        assert_eq!(query_terms("x \"open phrase"), ["x", "open phrase"]);
        assert_eq!(query_terms("\"\" \" \""), Vec::<String>::new());
        assert_eq!(query_terms("say\"hi\"now"), ["say", "hi", "now"]);
        assert_eq!(query_terms("Foo foo FOO bar"), ["Foo", "bar"]);
        let many = (0..100)
            .map(|n| format!("w{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(query_terms(&many).len(), super::MAX_TERMS);
        assert_eq!(
            query_terms(&"é".repeat(1000))[0].chars().count(),
            super::MAX_TERM_CHARS
        );
    }

    #[test]
    fn finds_ignoring_case() {
        assert_eq!(find_ci("Hello ZEBRA", "zebra"), Some(6));
        assert_eq!(find_ci("Ünïcode ÉCOLE", "école"), Some(10));
        assert_eq!(find_ci("abc", "abcd"), None);
    }

    #[test]
    fn snippets() {
        let t = |s: &str| vec![s.to_owned()];
        assert_eq!(
            snippet("short **zebra** `text`", &t("zebra")),
            "short zebra text"
        );
        let long = format!("{}zebra{}", "word ".repeat(20), " tail".repeat(40));
        let s = snippet(&long, &t("ZEBRA"));
        assert!(s.starts_with("…word"), "{s}");
        assert!(s.ends_with('…'), "{s}");
        assert_eq!(s.chars().count(), 120, "{s}");
        // Multibyte text cuts between characters.
        let s = snippet(&format!("{}é zebra", "é".repeat(200)), &t("zebra"));
        assert!(s.contains("zebra"), "{s}");
    }
}
