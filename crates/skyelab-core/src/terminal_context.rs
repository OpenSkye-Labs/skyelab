//! Context added to a prompt (T3's context chips): terminal output, another thread's gist or a pull request,
//! capped, then sent as fenced blocks after the typed text.

/// The most characters one excerpt keeps; the rest is cut and marked.
pub const MAX_CHARS: usize = 20_000;

/// `text` with CRLFs made plain and blank lines trimmed from both ends, cut to [`MAX_CHARS`] (on a char boundary,
/// with a note saying so).
pub fn clip(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let text = text.trim_matches('\n');
    match text.char_indices().nth(MAX_CHARS) {
        Some((at, _)) => format!("{}\n[truncated]", &text[..at]),
        None => text.to_owned(),
    }
}

/// The most characters of another thread's last reply a thread chip carries.
pub const MAX_REPLY_CHARS: usize = 2_000;
/// The most characters of a thread or pull request title a chip carries.
pub const MAX_TITLE_CHARS: usize = 200;

/// `text`'s first `max` characters, with an ellipsis when cut.
fn cap(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// A thread chip's text: the thread's title, then its last reply (capped), or a note that it has none yet.
pub fn thread_summary(title: &str, reply: Option<&str>) -> String {
    let title = cap(title.trim(), MAX_TITLE_CHARS);
    match reply.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => format!("Title: {title}\nLast reply:\n{}", cap(r, MAX_REPLY_CHARS)),
        None => format!("Title: {title}\n(no reply yet)"),
    }
}

/// A pull request chip's text: its URL and the title of the thread it is linked to (SkyeLab stores no PR title,
/// and reading one would ask GitHub).
pub fn pr_summary(url: &str, thread_title: &str) -> String {
    format!(
        "URL: {url}\nThread: {}",
        cap(thread_title.trim(), MAX_TITLE_CHARS)
    )
}

/// The most characters of an issue's body an issue chip carries.
pub const MAX_ISSUE_BODY_CHARS: usize = 4_000;

/// An issue chip's text: title, link, labels, then the body (capped), or a note that it has none.
pub fn issue_summary(url: &str, title: &str, labels: &[String], body: &str) -> String {
    let mut out = format!("Title: {}\nURL: {url}", cap(title.trim(), MAX_TITLE_CHARS));
    if !labels.is_empty() {
        out.push_str(&format!("\nLabels: {}", labels.join(", ")));
    }
    match body.trim() {
        "" => out.push_str("\n(no description)"),
        b => out.push_str(&format!("\n\n{}", cap(b, MAX_ISSUE_BODY_CHARS))),
    }
    out
}

/// One block as it is sent: its heading (`Terminal 1 output`), then the text in a code fence longer than any run of
/// backticks inside it, so the text can't close the fence early.
pub fn fenced(heading: &str, text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{heading}:\n{fence}text\n{text}\n{fence}")
}

/// The prompt as sent: the typed text, then each `(heading, text)` block, a blank line apart.
pub fn with_contexts<'a>(
    prompt: &str,
    contexts: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !prompt.is_empty() {
        parts.push(prompt.to_owned());
    }
    parts.extend(
        contexts
            .into_iter()
            .map(|(heading, text)| fenced(heading, text)),
    );
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_block_outlasts_backticks_inside() {
        let block = fenced("Terminal 1 output", "a ```` b");
        assert!(
            block.starts_with("Terminal 1 output:\n`````text\n"),
            "{block}"
        );
        assert!(block.ends_with("\n`````"), "{block}");
        assert!(fenced("T output", "plain").contains("\n```text\nplain\n```"));
    }

    #[test]
    fn fenced_block_text_is_capped_on_a_char_boundary() {
        let long = "é".repeat(MAX_CHARS + 10);
        let clipped = clip(&long);
        assert!(clipped.ends_with("\n[truncated]"));
        assert_eq!(clipped.chars().filter(|c| *c == 'é').count(), MAX_CHARS);
        assert_eq!(clip("\r\nx\r\ny\n\n"), "x\ny");
    }

    #[test]
    fn thread_and_pr_summaries_are_capped() {
        let long = "é".repeat(MAX_REPLY_CHARS + 50);
        let s = thread_summary("  Fix login  ", Some(&long));
        assert!(s.starts_with("Title: Fix login\nLast reply:\n"), "{s}");
        assert!(s.ends_with('…'));
        assert_eq!(s.chars().filter(|c| *c == 'é').count(), MAX_REPLY_CHARS);
        assert_eq!(thread_summary("t", Some(" ")), "Title: t\n(no reply yet)");
        let title = "x".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(
            pr_summary("https://github.com/o/r/pull/7", &title),
            format!(
                "URL: https://github.com/o/r/pull/7\nThread: {}…",
                &title[..MAX_TITLE_CHARS]
            )
        );
    }

    #[test]
    fn issue_summary_is_capped() {
        let body = "z".repeat(MAX_ISSUE_BODY_CHARS + 10);
        let s = issue_summary(
            "https://github.com/o/r/issues/3",
            " Crash ",
            &["bug".into()],
            &body,
        );
        assert!(
            s.starts_with("Title: Crash\nURL: https://github.com/o/r/issues/3\nLabels: bug\n\n"),
            "{s}"
        );
        assert_eq!(
            s.chars().filter(|c| *c == 'z').count(),
            MAX_ISSUE_BODY_CHARS
        );
        assert!(issue_summary("u", "t", &[], " ").ends_with("(no description)"));
    }

    #[test]
    fn fenced_block_follows_the_prompt() {
        let sent = with_contexts("why?", [("Terminal 2 output", "err")]);
        assert_eq!(sent, "why?\n\nTerminal 2 output:\n```text\nerr\n```");
        assert_eq!(
            with_contexts("", [("T output", "x")]),
            "T output:\n```text\nx\n```"
        );
    }
}
