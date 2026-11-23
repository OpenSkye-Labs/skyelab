//! File links in agent replies: `path:line` text and code spans naming a file in the thread's folder become links
//! to the Preview tab, and markdown links to files outside it lose their link. Only the text is looked at (no disk
//! reads while drawing); the preview checks the real path again when one is clicked.

use super::super::timeline_text::file_link;
use std::path::{Component, Path};

/// `text` with file paths inside `root` linked and outside-folder file links unwrapped. Fenced code is left alone.
pub(in crate::workspace) fn linkify(text: &str, root: &Path) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            out.push_str(line);
        } else if fenced {
            out.push_str(line);
        } else {
            scan(line, root, &mut out);
        }
    }
    out
}

/// The line a file link points at (`a.rs:12` or `a.rs:12:3` → 12).
pub(in crate::workspace) fn link_line(url: &str) -> Option<u32> {
    let path = file_link(url)?;
    let rest = url.trim().strip_prefix(path.as_str())?.strip_prefix(':')?;
    rest.split(':').next()?.parse().ok().filter(|&n| n > 0)
}

fn scan(line: &str, root: &Path, out: &mut String) {
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        let used = if c == '`' {
            code_span(rest, root, out)
        } else if c == '[' {
            md_link(rest, root, out)
        } else if c.is_whitespace() {
            out.push(c);
            c.len_utf8()
        } else {
            word(rest, root, out)
        };
        rest = &rest[used..];
    }
}

/// A code span: linked when it is just a path in the folder. Returns the bytes it used.
fn code_span(s: &str, root: &Path, out: &mut String) -> usize {
    let ticks = s.len() - s.trim_start_matches('`').len();
    let fence = &s[..ticks];
    let Some(end) = s[ticks..].find(fence).map(|i| i + ticks) else {
        out.push_str(fence);
        return ticks;
    };
    let inner = &s[ticks..end];
    let whole = &s[..end + ticks];
    if ticks == 1 && is_file_ref(inner, root, false) {
        out.push_str(&format!("[`{inner}`]({inner})"));
    } else {
        out.push_str(whole);
    }
    whole.len()
}

/// A `[label](target)` link: kept, unless it names a file outside the folder, which leaves only its label.
fn md_link(s: &str, root: &Path, out: &mut String) -> usize {
    let parsed = s.find("](").and_then(|mid| {
        let end = s[mid + 2..].find(')')? + mid + 2;
        Some((&s[1..mid], &s[mid + 2..end], end + 1))
    });
    let Some((label, target, len)) = parsed.filter(|(label, ..)| !label.contains('[')) else {
        out.push('[');
        return 1;
    };
    let target = match target.trim().strip_prefix('<') {
        Some(t) => t.split('>').next().unwrap_or_default(),
        None => target.split_whitespace().next().unwrap_or_default(),
    };
    match file_link(target) {
        Some(path) if !lexically_inside(&path, root) => scan(label, root, out),
        _ => out.push_str(&s[..len]),
    }
    len
}

/// A bare word: linked when it is `path:line` in the folder; punctuation around it stays outside the link.
fn word(s: &str, root: &Path, out: &mut String) -> usize {
    let end = s
        .find(|c: char| c.is_whitespace() || c == '`' || c == '[')
        .unwrap_or(s.len());
    let token = &s[..end];
    let core = token.trim_start_matches(['(', '"', '\'', '<']);
    let lead = &token[..token.len() - core.len()];
    let core = core.trim_end_matches(['.', ',', ';', ':', ')', '"', '\'', '!', '?', '>']);
    if is_file_ref(core, root, true) {
        let tail = &token[lead.len() + core.len()..];
        out.push_str(&format!("{lead}[{core}]({core}){tail}"));
    } else {
        out.push_str(token);
    }
    end.max(1)
}

/// Whether `s` reads as a file path in the folder (with `:line` when `need_line`).
fn is_file_ref(s: &str, root: &Path, need_line: bool) -> bool {
    let Some(path) = file_link(s) else {
        return false;
    };
    let drive =
        path.len() > 2 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic();
    let plain = path
        .char_indices()
        .all(|(i, c)| c.is_alphanumeric() || "/._-~@+".contains(c) || (drive && i == 1));
    let p = Path::new(&path);
    let named = path.contains('/') || p.extension().is_some_and(|e| !e.is_empty());
    !path.is_empty()
        && plain
        && named
        && (!need_line || link_line(s).is_some())
        && lexically_inside(&path, root)
}

/// `path` is in `root` by its text alone: no `..`, no `~`, and an absolute one starts with `root`.
pub(in crate::workspace) fn lexically_inside(path: &str, root: &Path) -> bool {
    let p = Path::new(path);
    let climbs = p.components().any(|c| matches!(c, Component::ParentDir));
    !climbs && !path.starts_with('~') && (!p.has_root() || p.starts_with(root))
}
