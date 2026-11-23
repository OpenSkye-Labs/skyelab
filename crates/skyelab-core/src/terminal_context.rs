//! Terminal output added to a prompt (T3's terminal context chip): capped, then sent as a fenced block.

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

/// One excerpt as it is sent: a heading naming the terminal, then the text in a code fence longer than any run of
/// backticks inside it, so the excerpt can't close the fence early.
pub fn fenced(label: &str, text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{label} output:\n{fence}text\n{text}\n{fence}")
}

/// The prompt as sent: the typed text, then each excerpt, a blank line apart.
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
            .map(|(label, text)| fenced(label, text)),
    );
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_block_outlasts_backticks_inside() {
        let block = fenced("Terminal 1", "a ```` b");
        assert!(
            block.starts_with("Terminal 1 output:\n`````text\n"),
            "{block}"
        );
        assert!(block.ends_with("\n`````"), "{block}");
        assert!(fenced("T", "plain").contains("\n```text\nplain\n```"));
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
    fn fenced_block_follows_the_prompt() {
        let sent = with_contexts("why?", [("Terminal 2", "err")]);
        assert_eq!(sent, "why?\n\nTerminal 2 output:\n```text\nerr\n```");
        assert_eq!(
            with_contexts("", [("T", "x")]),
            "T output:\n```text\nx\n```"
        );
    }
}
