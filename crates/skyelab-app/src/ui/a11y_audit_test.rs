//! Static accessibility audit for `crates/skyelab-app/src/`: fails if a custom `.on_click(...)` or left
//! `.on_mouse_down(...)` outside SkyeLab's accessible primitives lacks `.role(...)` or an accessible label on the
//! element itself, or if a `ui::row(...)` / `row_surface(...)` caller omits `.aria_label(...)`. Heuristic: it reads
//! source text, so docs/gpui/accessibility.md §8 says how to teach it a new helper.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[path = "a11y_audit_scan_test.rs"]
mod scan;
use scan::*;

const PRIMITIVE_FILES: &[&str] = &[
    "ui/button.rs",
    "ui/choice.rs",
    "ui/toast.rs",
    "ui/toggle.rs",
    "ui/components.rs",
];

fn collect_non_test_rs_files(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let entries = fs::read_dir(dir).expect("read source dir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_non_test_rs_files(&path, root, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(".rs")
            || name.ends_with("_test.rs")
            || name.ends_with("_tests.rs")
            || name == "tests.rs"
        {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .expect("strip src prefix")
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, path));
    }
}

const BUILTINS: &[&str] = &[
    "Button::",
    "icon_button(",
    "Switch::",
    "Checkbox::",
    "SegmentedControl::",
    "Tabs::",
    "PopupMenuItem::",
    "dialog_cancel(",
    "Toast::",
    ": Toast",
    "ui::row(",
    "ui::row_surface(",
    "row_surface(",
    "tinted_row(",
    "plain_row(",
    "tool_row",
];

/// Names of crate `fn`s whose every definition builds on a primitive, so a helper imported from another file counts.
fn accessible_fns(files: &[(String, String)]) -> std::collections::HashSet<String> {
    let mut verdicts: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for (_, masked) in files {
        for (pos, _) in masked.match_indices("fn ") {
            let name: String = masked[pos + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            let body = fn_body(masked, pos);
            let ok = BUILTINS.iter().any(|k| body.contains(k)) || body.contains(".role(");
            *verdicts.entry(name).or_insert(true) &= ok;
        }
    }
    verdicts
        .into_iter()
        .filter(|(_, ok)| *ok)
        .map(|(name, _)| name)
        .collect()
}

/// Accessible helpers shared under a common name, as (`use` path, call): `thread_details_card.rs`'s `row`.
const IMPORTED_HELPERS: &[(&str, &str)] = &[("use super::card::{", "row(")];

fn has_accessible_primitive(
    expr: &str,
    full_file_masked: &str,
    crate_fns: &std::collections::HashSet<String>,
) -> bool {
    if BUILTINS.iter().any(|k| expr.contains(k))
        || IMPORTED_HELPERS
            .iter()
            .any(|(import, call)| full_file_masked.contains(import) && expr.contains(call))
    {
        return true;
    }
    // A file-local helper (`fn button(..)`, `let toggle = |..| ..`, a `let seg = SegmentedControl::new(..)` local or a
    // `note: Toast` parameter) counts when its definition builds on a primitive.
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_')
            || (i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
        {
            i += 1;
            continue;
        }
        let end = i + bytes[i..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
            .count();
        let ident = &expr[i..end];
        i = end;
        let defs = [
            format!("fn {ident}("),
            format!("let {ident} = "),
            format!("let mut {ident} = "),
            format!("{ident}: "),
        ];
        let mut local = defs
            .iter()
            .flat_map(|def| {
                full_file_masked
                    .match_indices(def.as_str())
                    .map(move |(pos, _)| (def, pos))
            })
            .peekable();
        // A definition in this file decides; only a name it doesn't define falls back to the crate's `fn`s.
        if local.peek().is_none() {
            if expr[end..].starts_with('(') && crate_fns.contains(ident) {
                return true;
            }
            continue;
        }
        let builds_primitive = local.any(|(def, pos)| {
            // A `fn` reads its body; a `let` or a parameter only its own statement.
            let body = if def.starts_with("fn ") {
                fn_body(full_file_masked, pos)
            } else {
                let mut at = pos + def.len();
                let rest = full_file_masked[at..].trim_start_matches("move ");
                // Step over a closure's `|a, b|` so its commas don't end the statement.
                if let Some(params) = rest.strip_prefix('|') {
                    at = full_file_masked.len() - params.len()
                        + params.find('|').map_or(0, |close| close + 1);
                }
                statement_forward(full_file_masked, at)
            };
            BUILTINS.iter().any(|k| body.contains(k)) || body.contains(".role(")
        });
        if builds_primitive {
            return true;
        }
    }
    false
}

#[test]
fn audit_helpers_keep_scope_to_the_clicked_element() {
    let probe = "use gpui::div;\n#[cfg(test)]\nmod t;\nfn p() { div().id(1).child(Button::new(2)).on_click(|_, _, _| {}); }";
    let masked = blank_test_items(&mask_comments_and_strings(probe));
    assert_eq!(masked.len(), probe.len());
    assert!(
        masked.contains("fn p()"),
        "code after a test module must stay audited"
    );
    let (dot, _) = next_click_handler(&masked, 0).expect("finds the click");
    let recv = without_child_args(&resolve_receiver_context(&masked, dot));
    assert!(
        !has_accessible_primitive(&recv, &masked, &Default::default()),
        "a Button child must not make its clickable parent count as accessible"
    );
}

#[test]
fn custom_clickables_and_ui_rows_declare_role_and_label() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_non_test_rs_files(&src_root, &src_root, &mut files);
    // The browser pane is UI too; its paths are reported from the crates folder (`skyelab-browser/src/..`).
    let crates = src_root.join("../..");
    let browser = crates.join("skyelab-browser/src");
    collect_non_test_rs_files(&browser, &crates, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut violations = Vec::new();
    let sources: Vec<(String, String)> = files
        .into_iter()
        .map(|(rel, path)| {
            let raw = fs::read_to_string(&path).expect("read source file");
            (rel, blank_test_items(&mask_comments_and_strings(&raw)))
        })
        .collect();
    let crate_fns = accessible_fns(&sources);

    for (rel, masked) in &sources {
        let (rel, masked) = (rel.as_str(), masked.as_str());
        if PRIMITIVE_FILES.contains(&rel) {
            continue;
        }

        let mut scan = 0usize;
        while let Some((dot_pos, call)) = next_click_handler(masked, scan) {
            scan = dot_pos + call.len();

            let arg_end = balanced_paren_arg_end(masked, scan);
            let body = masked[scan..arg_end.saturating_sub(1)].trim();
            // A propagation guard, or a press that only focuses a region (`this.focus.focus(..)`), is not a control.
            let dots = body.matches('.').count();
            if (body.contains("stop_propagation") && dots <= 2)
                || (body.contains(".focus(") && dots <= 3)
            {
                continue;
            }

            let recv = without_child_args(&resolve_receiver_context(masked, dot_pos));
            let explicit_a11y = recv.contains(".role(")
                && (recv.contains(".aria_label(") || recv.contains(".accessibility_label("));
            if !explicit_a11y && !has_accessible_primitive(&recv, masked, &crate_fns) {
                violations.push(format!(
                    "{rel}:{}: `{call}...)` without accessible primitive or `.role(...)` + `.aria_label(...)`",
                    line_number(masked, dot_pos)
                ));
            }
        }

        // A scroll container registers where it is on screen, so controls inside it can be scrolled into view.
        if !rel.starts_with("skyelab-browser/") {
            for call in [
                ".overflow_y_scroll()",
                ".overflow_x_scroll()",
                ".overflow_scroll()",
            ] {
                let mut from = 0;
                while let Some(at) = masked[from..].find(call) {
                    let pos = from + at;
                    from = pos + call.len();
                    if !statement_forward(masked, pos).contains(".a11y_scroll_area(") {
                        violations.push(format!(
                            "{rel}:{}: `{call}` without `.a11y_scroll_area()`",
                            line_number(masked, pos)
                        ));
                    }
                }
            }
        }

        for row_call in ["ui::row(", "row_surface("] {
            let mut rscan = 0usize;
            while let Some(offset) = masked[rscan..].find(row_call) {
                let call_pos = rscan + offset;
                rscan = call_pos + row_call.len();
                let stmt = statement_forward(masked, call_pos);
                let has_label =
                    stmt.contains(".aria_label(") || stmt.contains(".accessibility_label(");
                if !has_label {
                    violations.push(format!(
                        "{rel}:{}: `{row_call}...)` call missing `.aria_label(...)`",
                        line_number(masked, call_pos)
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "accessibility audit found missing roles/labels:\n{}",
        violations.join("\n")
    );
}
