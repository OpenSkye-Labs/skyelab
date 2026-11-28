//! Own UI kit, stage 2b (.agent/plans/2026-11-28-own-ui-kit-layout-icon.md), criterion 2: skyelab-app lays out and
//! draws icons with crate::ui's `h_flex`, `v_flex` and `Icon`, never gpui-component's or gpui-base's.

use std::{fs, path::Path};

const THEIRS: &[&str] = &["h_flex", "v_flex", "Icon", "IconName"];
const CRATES: &[&str] = &["gpui_component", "gpui_base"];
/// The parity test compares against theirs; theme_boundary_test.rs holds an import of theirs as a string.
const ALLOWED: &[&str] = &[
    "icon_layout_test.rs",
    "layout_icon_boundary_test.rs",
    "theme_boundary_test.rs",
];

/// Each `use gpui_component…;` / `use gpui_base…;` statement in `src`, joined onto one line, with its line number.
fn uses(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (n, line) in src.lines().enumerate() {
        let t = line.trim_start();
        let t_use = t.strip_prefix("pub ").unwrap_or(t);
        if open.is_none()
            && CRATES
                .iter()
                .any(|c| t_use.starts_with(&format!("use {c}")))
        {
            open = Some((n + 1, String::new()));
        }
        if let Some((_, text)) = open.as_mut() {
            text.push_str(t);
            text.push(' ');
            if t.contains(';') {
                out.extend(open.take());
            }
        }
    }
    out
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
}

/// The names `line` reaches through a full path such as `gpui_component::Icon::new`.
fn paths(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for c in CRATES {
        let prefix = format!("{c}::");
        for (i, _) in line.match_indices(&prefix) {
            out.extend(words(&line[i + prefix.len()..]).next());
        }
    }
    out
}

fn hits_in(src: &str, rel: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for (n, text) in uses(src) {
        for word in THEIRS {
            if words(&text).any(|w| w == *word) {
                hits.push(format!("{rel}:{n}: imports {word}"));
            }
        }
    }
    for (n, line) in src.lines().enumerate() {
        for name in paths(line) {
            if THEIRS.contains(&name) {
                hits.push(format!("{rel}:{}: names {name}", n + 1));
            }
        }
    }
    hits
}

fn scan(dir: &Path, root: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, root, hits);
            continue;
        }
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if path.extension().is_none_or(|e| e != "rs") || ALLOWED.contains(&file) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        hits.extend(hits_in(
            &fs::read_to_string(&path).expect("read source"),
            &rel,
        ));
    }
}

#[test]
fn views_use_our_layout_helpers_and_icon() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&root, &root, &mut hits);
    assert!(
        hits.is_empty(),
        "{} places still use gpui-component's layout helpers or Icon:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[test]
fn the_scan_sees_imports_and_paths_but_not_icon_named() {
    let src = "use gpui_component::{\n    ActiveTheme as _,\n    h_flex,\n};\n\
               use gpui_component::IconNamed;\n\
               let i = gpui_component::Icon::new(x);\n\
               pub use gpui_base::v_flex;\n";
    let hits = hits_in(src, "f.rs");
    assert_eq!(
        hits,
        [
            "f.rs:1: imports h_flex",
            "f.rs:7: imports v_flex",
            "f.rs:6: names Icon",
            "f.rs:7: names v_flex"
        ]
    );
}
