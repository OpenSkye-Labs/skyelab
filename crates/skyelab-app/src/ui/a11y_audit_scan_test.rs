//! Source-text helpers for the accessibility audit: masking comments and strings, finding the receiver of a click
//! handler, and the bounds of calls and statements.

/// Replaces comments and string literals with spaces while preserving line count (`\n`), so balanced-delimiter scanning
/// never trips on strings or comments and byte offsets map accurately back to 1-based line numbers.
pub(super) fn mask_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    let mut line_comment = false;
    let mut block_depth: usize = 0;
    let mut in_str = false;
    let mut raw_hashes: Option<usize> = None;

    while i < n {
        let ch = src[i..].chars().next().expect("valid utf8 boundary");
        let ch_len = ch.len_utf8();
        if line_comment {
            if ch == '\n' {
                line_comment = false;
                out.push('\n');
            } else {
                out.push(' ');
            }
            i += ch_len;
        } else if block_depth > 0 {
            if bytes[i..].starts_with(b"/*") {
                block_depth += 1;
                out.push_str("  ");
                i += 2;
            } else if bytes[i..].starts_with(b"*/") {
                block_depth -= 1;
                out.push_str("  ");
                i += 2;
            } else {
                out.push(if ch == '\n' { '\n' } else { ' ' });
                i += ch_len;
            }
        } else if let Some(hashes) = raw_hashes {
            if ch == '"'
                && i + 1 + hashes <= n
                && bytes[i + 1..i + 1 + hashes].iter().all(|&b| b == b'#')
            {
                out.push(' ');
                for _ in 0..hashes {
                    out.push(' ');
                }
                raw_hashes = None;
                i += 1 + hashes;
            } else {
                out.push(if ch == '\n' { '\n' } else { ' ' });
                i += ch_len;
            }
        } else if in_str {
            if ch == '\\' && i + 1 < n {
                out.push(' ');
                let next_ch = src[i + 1..].chars().next().expect("next char");
                out.push(if next_ch == '\n' { '\n' } else { ' ' });
                i += 1 + next_ch.len_utf8();
            } else if ch == '"' {
                in_str = false;
                out.push(' ');
                i += 1;
            } else {
                out.push(if ch == '\n' { '\n' } else { ' ' });
                i += ch_len;
            }
        } else if bytes[i..].starts_with(b"//") {
            line_comment = true;
            out.push_str("  ");
            i += 2;
        } else if bytes[i..].starts_with(b"/*") {
            block_depth = 1;
            out.push_str("  ");
            i += 2;
        } else if ch == 'r' && i + 1 < n && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') {
            let mut j = i + 1;
            let mut hashes = 0;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                raw_hashes = Some(hashes);
                for _ in 0..=(j - i) {
                    out.push(' ');
                }
                i = j + 1;
            } else {
                out.push('r');
                i += 1;
            }
        } else if ch == '"' {
            in_str = true;
            out.push(' ');
            i += 1;
        } else {
            out.push(ch);
            i += ch_len;
        }
    }
    out
}

pub(super) fn line_number(masked: &str, byte_offset: usize) -> usize {
    1 + masked[..byte_offset]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

pub(super) fn balanced_paren_arg_end(masked: &str, open_paren_after: usize) -> usize {
    let bytes = masked.as_bytes();
    let mut depth = 1usize;
    let mut i = open_paren_after;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    i
}

/// Walks backward from `dot_pos` across chained method calls and steps out of `.map(|r| r...)` / `.when(..., |r| r...)`
/// closures back into their outer receiver chain.
pub(super) fn resolve_receiver_context(masked: &str, dot_pos: usize) -> String {
    let bytes = masked.as_bytes();
    let mut out = String::new();
    let mut cursor = dot_pos;

    for _step in 0..4 {
        let mut p = 0i32;
        let mut b = 0i32;
        let mut k = 0i32;
        let mut i = cursor as isize - 1;
        while i >= 0 {
            match bytes[i as usize] {
                b')' => p += 1,
                b'(' => {
                    if p == 0 {
                        break;
                    }
                    p -= 1;
                }
                b']' => b += 1,
                b'[' => {
                    if b == 0 {
                        break;
                    }
                    b -= 1;
                }
                b'}' => k += 1,
                b'{' => {
                    if k == 0 {
                        break;
                    }
                    k -= 1;
                }
                b',' | b';' | b'=' | b'|' if p == 0 && b == 0 && k == 0 => break,
                _ => {}
            }
            i -= 1;
        }
        let start = (i + 1) as usize;
        let chunk = &masked[start..cursor];
        out.insert_str(0, chunk);

        // If stopped at `|row|` inside `.map(|row| ...)` or `.when(..., |row| ...)`, step out to the enclosing `.`!
        let stop_char = if i >= 0 { bytes[i as usize] } else { 0 };
        if stop_char == b'|' {
            // Find the matching `|` opening the closure params, then enclosing `(`, then step to its preceding `.`!
            let mut pipe_open = i - 1;
            while pipe_open >= 0 && bytes[pipe_open as usize] != b'|' {
                pipe_open -= 1;
            }
            if pipe_open >= 0 {
                // Find enclosing unmatched `(` (argument opener of .map( / .when( / .when_some().
                let mut par = 0i32;
                let mut cur = pipe_open - 1;
                while cur >= 0 {
                    match bytes[cur as usize] {
                        b')' => par += 1,
                        b'(' => {
                            if par == 0 {
                                break;
                            }
                            par -= 1;
                        }
                        b';' | b'{' if par == 0 => {
                            cur = -1;
                            break;
                        }
                        _ => {}
                    }
                    cur -= 1;
                }
                if cur > 0 {
                    // Find `.method(` before `cur`.
                    let before = masked[..cur as usize].trim_end();
                    if let Some(dot) = before.rfind('.') {
                        let method = &before[dot + 1..];
                        if matches!(
                            method,
                            "map" | "when" | "when_some" | "when_none" | "when_else"
                        ) {
                            cursor = dot;
                            out.insert(0, ' ');
                            continue;
                        }
                    }
                }
            }
        }

        // If the root expression is a local identifier `var` (e.g. `seg`, `note`, `row`), look back in the current
        // function body for `let (mut )?<ident>` or parameter `<ident>:`.
        let trimmed = chunk.trim_start();
        if let Some(ident) = trimmed
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .filter(|s| !s.is_empty() && trimmed.starts_with(s))
        {
            let rest_after_ident = trimmed[ident.len()..].trim_start();
            if rest_after_ident.is_empty() || rest_after_ident.starts_with('.') {
                let fn_start = masked[..start]
                    .rfind("\nfn ")
                    .or_else(|| masked[..start].rfind(" fn "))
                    .unwrap_or(0);
                let scope = &masked[fn_start..start];
                let let_pat = format!("let mut {ident}");
                let let_imm = format!("let {ident}");
                if let Some(pos) = scope.rfind(&let_pat).or_else(|| scope.rfind(&let_imm)) {
                    let decl_tail = &scope[pos..];
                    out.insert(0, ' ');
                    out.insert_str(0, decl_tail);
                } else {
                    let param_pat = format!("{ident}:");
                    if let Some(pos) = scope.rfind(&param_pat) {
                        let sig_end = (pos + param_pat.len() + 32).min(scope.len());
                        out.insert(0, ' ');
                        out.insert_str(0, &scope[pos..sig_end]);
                    }
                }
            }
        }
        break;
    }

    out
}

/// A `fn`'s text from its name to its matching `}` (or the end of a bodiless signature).
pub(super) fn fn_body(masked: &str, pos: usize) -> &str {
    let Some(open) = masked[pos..].find(['{', ';']).map(|o| pos + o) else {
        return &masked[pos..];
    };
    if masked.as_bytes()[open] == b';' {
        return &masked[pos..open];
    }
    let mut depth = 0usize;
    for (i, b) in masked.bytes().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &masked[pos..=i];
                }
            }
            _ => {}
        }
    }
    &masked[pos..]
}

pub(super) fn statement_forward(masked: &str, start_pos: usize) -> &str {
    let bytes = masked.as_bytes();
    let mut p = 0i32;
    let mut b = 0i32;
    let mut k = 0i32;
    let mut i = start_pos;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => p += 1,
            b')' => {
                if p == 0 {
                    break;
                }
                p -= 1;
            }
            b'[' => b += 1,
            b']' => {
                if b == 0 {
                    break;
                }
                b -= 1;
            }
            b'{' => k += 1,
            b'}' => {
                if k == 0 {
                    break;
                }
                k -= 1;
            }
            b';' | b',' if p == 0 && b == 0 && k == 0 => break,
            _ => {}
        }
        i += 1;
    }
    &masked[start_pos..i]
}

/// The next `.on_click(` or left `.on_mouse_down(` at or after `from`: both make a control clickable, and a mouse-down
/// alone gives it no accessible Click.
pub(super) fn next_click_handler(masked: &str, from: usize) -> Option<(usize, &'static str)> {
    let click = masked[from..]
        .find(".on_click(")
        .map(|at| (from + at, ".on_click("));
    let mut scan = from;
    let left_down = loop {
        let Some(at) = masked[scan..].find(".on_mouse_down(") else {
            break None;
        };
        let at = scan + at;
        scan = at + ".on_mouse_down(".len();
        if masked[scan..].trim_start().starts_with("MouseButton::Left") {
            break Some((at, ".on_mouse_down("));
        }
    };
    [click, left_down]
        .into_iter()
        .flatten()
        .min_by_key(|&(at, _)| at)
}

/// `recv` with the arguments of `.child(...)` / `.children(...)` blanked: a primitive or role on a child says nothing
/// about the element that takes the click.
pub(super) fn without_child_args(recv: &str) -> String {
    let mut out = recv.to_owned();
    for call in [".child(", ".children("] {
        let mut from = 0;
        while let Some(at) = out[from..].find(call) {
            let open = from + at + call.len();
            let close = balanced_paren_arg_end(&out, open)
                .saturating_sub(1)
                .max(open);
            out.replace_range(open..close, &" ".repeat(close - open));
            from = open;
        }
    }
    out
}

/// `masked` with every `#[cfg(test)]` item (a `mod x;`, a `mod x { .. }`, a test fn or use) turned into spaces,
/// newlines kept so line numbers still match. Production code after a test module is still audited.
pub(super) fn blank_test_items(masked: &str) -> String {
    let mut out = masked.to_owned();
    let mut from = 0;
    while let Some(at) = out[from..].find("#[cfg(test)]") {
        let start = from + at;
        let bytes = out.as_bytes();
        let mut end = start;
        let mut depth = 0i32;
        while end < bytes.len() {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end += 1;
                        break;
                    }
                }
                b';' if depth == 0 => {
                    end += 1;
                    break;
                }
                _ => {}
            }
            end += 1;
        }
        // Byte for byte, so a multi-byte char in the item keeps every later offset in place.
        let blank: String = out.as_bytes()[start..end]
            .iter()
            .map(|&c| if c == b'\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(start..end, &blank);
        from = end;
    }
    out
}
