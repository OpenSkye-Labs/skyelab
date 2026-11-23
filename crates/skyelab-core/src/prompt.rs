//! Composer text rules: when a paste is long enough to become a file, what that file is called, and the most a
//! prompt can carry (T3's limits: `packages/client-runtime/src/textPaste.ts`, `contracts/src/chatAttachment.ts`).

/// A paste this many bytes or more becomes an attachment instead of prompt text.
pub const LONG_PASTE_BYTES: usize = 8 * 1024;
/// A paste of this many lines or more becomes an attachment too, however short its lines.
pub const LONG_PASTE_LINES: usize = 200;
/// The most characters one prompt can send.
pub const MAX_PROMPT_CHARS: usize = 120_000;

/// Whether pasted `text` should be attached as a file rather than typed into the prompt.
pub fn is_long_paste(text: &str) -> bool {
    text.len() >= LONG_PASTE_BYTES || text.lines().count() >= LONG_PASTE_LINES
}

/// `pasted-text.txt`, else the first free `pasted-text-N.txt` among `taken` (compared case-insensitively).
pub fn pasted_file_name<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: Vec<String> = taken.into_iter().map(str::to_lowercase).collect();
    let free = |n: &String| !taken.contains(n);
    std::iter::once("pasted-text.txt".to_owned())
        .chain((2..).map(|n| format!("pasted-text-{n}.txt")))
        .find(free)
        .expect("an unbounded sequence always has a free name")
}

/// Why `prompt` can't be sent as is, when it is over [`MAX_PROMPT_CHARS`].
pub fn over_limit(prompt: &str) -> Option<String> {
    let over = prompt
        .trim()
        .chars()
        .count()
        .checked_sub(MAX_PROMPT_CHARS)?;
    (over > 0).then(|| {
        let s = if over == 1 { "" } else { "s" };
        format!(
            "Prompt is {} character{s} over the 120,000-character limit. Shorten it, or paste it as a file.",
            group(over)
        )
    })
}

/// `12345` as `12,345`.
fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paste_by_bytes_or_lines() {
        assert!(!is_long_paste("short"));
        assert!(!is_long_paste(&"x".repeat(LONG_PASTE_BYTES - 1)));
        assert!(is_long_paste(&"x".repeat(LONG_PASTE_BYTES)));
        assert!(!is_long_paste(&"a\n".repeat(LONG_PASTE_LINES - 1)));
        assert!(is_long_paste(&"a\n".repeat(LONG_PASTE_LINES)));
    }

    #[test]
    fn pasted_names_skip_taken_ones() {
        assert_eq!(pasted_file_name([]), "pasted-text.txt");
        assert_eq!(
            pasted_file_name(["a.png", "Pasted-Text.txt"]),
            "pasted-text-2.txt"
        );
        assert_eq!(
            pasted_file_name(["pasted-text.txt", "pasted-text-2.txt"]),
            "pasted-text-3.txt"
        );
    }

    #[test]
    fn limit_counts_characters_past_the_cap() {
        assert_eq!(over_limit(&"é".repeat(MAX_PROMPT_CHARS)), None);
        assert_eq!(
            over_limit(&"x".repeat(MAX_PROMPT_CHARS + 1)).as_deref(),
            Some(
                "Prompt is 1 character over the 120,000-character limit. Shorten it, or paste it as a file."
            )
        );
        let msg = over_limit(&"x".repeat(MAX_PROMPT_CHARS + 12_345)).expect("over");
        assert!(msg.starts_with("Prompt is 12,345 characters over"), "{msg}");
    }
}
