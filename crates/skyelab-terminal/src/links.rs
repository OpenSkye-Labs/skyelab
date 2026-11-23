//! Finding links in a line of terminal text: http(s) URLs and file paths. No other scheme is ever a link.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    Path,
}

/// A link in a line, as a char range (`start..end`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub kind: LinkKind,
}

/// Every link in `line`. A whitespace- or quote-separated token is a URL when it starts with `http://` or
/// `https://`, and a path when it contains no `://`, has a letter, and has a `/` or a `name.ext` shape.
pub fn find_links(line: &str) -> Vec<Found> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_break(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !is_break(chars[i]) {
            i += 1;
        }
        let token = &chars[start..i];
        // Leading brackets belong to the prose around the link.
        let lead = token
            .iter()
            .take_while(|c| matches!(c, '(' | '[' | '{'))
            .count();
        let token = &token[lead..];
        let text: String = token.iter().collect();
        let kind = if text.starts_with("http://") || text.starts_with("https://") {
            LinkKind::Url
        } else if is_path(&text) {
            LinkKind::Path
        } else {
            continue;
        };
        let len = trimmed_len(token);
        if kind == LinkKind::Url && len <= "https://".len() {
            continue;
        }
        let begin = start + lead;
        out.push(Found {
            start: begin,
            end: begin + len,
            kind,
        });
    }
    out
}

/// The file part of a path link: `src/main.rs:12:5` → `src/main.rs`.
pub fn path_target(text: &str) -> &str {
    let mut t = text;
    for _ in 0..2 {
        match t.rsplit_once(':') {
            Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => t = head,
            _ => break,
        }
    }
    t
}

fn is_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
}

fn is_path(text: &str) -> bool {
    let file = path_target(trim_str(text));
    if file.contains("://") || !file.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    if !file
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '+' | '@'))
    {
        return false;
    }
    if file.contains('/') {
        return true;
    }
    // `name.ext`: a dot with a letter-led name on both sides (not `3.14`, not `...`).
    match file.rsplit_once('.') {
        Some((name, ext)) => {
            !name.is_empty()
                && !ext.is_empty()
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
                && name.chars().any(|c| c.is_alphabetic())
        }
        None => false,
    }
}

fn trim_str(text: &str) -> &str {
    let chars: Vec<char> = text.chars().collect();
    let len = trimmed_len(&chars);
    let byte = text.char_indices().nth(len).map_or(text.len(), |(b, _)| b);
    &text[..byte]
}

/// The token's length without trailing punctuation and closing brackets it never opened.
fn trimmed_len(token: &[char]) -> usize {
    let mut len = token.len();
    while len > 0 {
        let c = token[len - 1];
        let unbalanced = |open: char| {
            let t = &token[..len];
            t.iter().filter(|&&x| x == open).count() < t.iter().filter(|&&x| x == c).count()
        };
        let drop = match c {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => unbalanced('('),
            ']' => unbalanced('['),
            '}' => unbalanced('{'),
            _ => false,
        };
        if !drop {
            break;
        }
        len -= 1;
    }
    len
}
