//! A thread row's text: its reply preview as one plain line, and a pull request's short label.

/// A reply as one plain line for the sidebar: markdown marks dropped, whitespace collapsed.
pub(in crate::workspace) fn preview_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut in_fence = false;
    for line in markdown.lines() {
        let line = line.trim();
        // Fenced code (a diagram's source, a snippet) and rules carry no words; headings, quotes and bullets keep theirs.
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence
            || line
                .chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '|' | ':' | ' '))
        {
            continue;
        }
        // A heading's marks ("## "), a quote's ("> "); "#24 merged" keeps its number.
        let line = line.trim_start_matches(['>', ' ']);
        let line = match line.trim_start_matches('#') {
            rest if rest.starts_with(' ') => rest.trim_start(),
            _ => line,
        };
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(
            link_text(line)
                .chars()
                .filter(|c| !matches!(c, '*' | '`' | '|')),
        );
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[text](url)` and `![alt](url)` as just their text; a `[` that isn't a link stays.
fn link_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let url = &after[close + 2..];
            url.find(')').map(|end| (&after[..close], &url[end + 1..]))
        });
        match link {
            Some((text, tail)) => {
                out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
                out.push_str(text);
                rest = tail;
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// "#24" for a pull request URL ending in its number, else "PR".
pub(in crate::workspace) fn pr_label(url: &str) -> String {
    match url.trim_end_matches('/').rsplit_once('/') {
        Some((_, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => format!("#{n}"),
        _ => "PR".into(),
    }
}
