//! Own UI kit, stage 2a (.agent/plans/2026-11-27-own-ui-kit-theme.md), criterion 4: views read our theme. Only the
//! bridge names gpui-component's theme (to write it for that crate's remaining widgets), and it never reads it back.

use std::{fs, path::Path};

/// Names gpui-component's theme goes by.
const THEIRS: &[&str] = &[
    "ActiveTheme",
    "Theme",
    "ThemeMode",
    "ThemeColor",
    "ThemeConfig",
    "ThemeRegistry",
];
/// The bridge, and the tests that check what it writes (the colors; our motion in gpui-component's widgets).
const ALLOWED: &[&str] = &[
    "theme_bridge.rs",
    "theme_match_test.rs",
    "motion_test.rs",
    "theme_boundary_test.rs",
];

/// Each `use gpui_component…;` statement in `src`, joined onto one line, with its line number.
fn uses(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (n, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if open.is_none()
            && (t.starts_with("use gpui_component") || t.starts_with("pub use gpui_component"))
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

/// Whether `text` names `word` as a whole identifier.
fn names(text: &str, word: &str) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|w| w == word)
}

fn scan(dir: &Path, root: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, root, hits);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let src = fs::read_to_string(&path).expect("read source");
        if file == "theme_bridge.rs" {
            for (n, line) in src.lines().enumerate() {
                if line.contains("Theme::global(") || line.contains(".theme()") {
                    hits.push(format!(
                        "{rel}:{}: the bridge reads gpui-component's theme",
                        n + 1
                    ));
                }
            }
        }
        if ALLOWED.contains(&file) {
            continue;
        }
        for (n, text) in uses(&src) {
            for word in THEIRS {
                if names(&text, word) {
                    hits.push(format!("{rel}:{n}: imports gpui-component's {word}"));
                }
            }
        }
        for (n, line) in src.lines().enumerate() {
            for path in [
                "gpui_component::Theme",
                "gpui_component::ActiveTheme",
                "gpui_component::theme::",
            ] {
                if line.contains(path) {
                    hits.push(format!("{rel}:{}: {path}", n + 1));
                }
            }
        }
    }
}

#[test]
fn views_read_our_theme_and_only_the_bridge_writes_theirs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&root, &root, &mut hits);
    assert!(
        root.join("ui/theme_bridge.rs").exists(),
        "the bridge is ui/theme_bridge.rs"
    );
    assert!(
        hits.is_empty(),
        "{} places still use gpui-component's theme:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[test]
fn the_scan_sees_a_multi_line_import() {
    let src = "use gpui_component::{\n    ActiveTheme as _,\n    h_flex,\n};\nuse gpui_component::Icon;\n";
    let found: Vec<_> = uses(src)
        .into_iter()
        .filter(|(_, t)| names(t, "ActiveTheme"))
        .map(|(n, _)| n)
        .collect();
    assert_eq!(found, [1]);
    assert!(!names("use gpui_component::ThemeColorX;", "ThemeColor"));
}
