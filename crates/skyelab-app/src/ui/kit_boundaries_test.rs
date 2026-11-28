use std::{fs, path::Path};

/// One stage of the own UI kit: the names it took over from these crates, and the files that still name them on
/// purpose (bridges, parity tests).
struct Row {
    stage: &'static str,
    crates: &'static [&'static str],
    names: &'static [&'static str],
    allowed: &'static [&'static str],
}

const ROWS: &[Row] = &[
    Row {
        stage: "2a theme",
        crates: &["gpui_component"],
        names: &[
            "ActiveTheme",
            "Theme",
            "ThemeMode",
            "ThemeColor",
            "ThemeConfig",
            "ThemeRegistry",
            "theme",
        ],
        allowed: &[
            "theme_bridge.rs",
            "theme_match_test.rs",
            "theme_switch_test.rs",
            "motion_test.rs",
            "small_pieces_parity_test.rs",
        ],
    },
    Row {
        stage: "2b layout and icon",
        crates: &["gpui_component", "gpui_base"],
        names: &["h_flex", "v_flex", "Icon", "IconName"],
        allowed: &["icon_layout_test.rs"],
    },
    Row {
        stage: "3 menus and popovers",
        crates: &["gpui_component", "gpui_base"],
        names: &[
            "menu",
            "PopupMenu",
            "PopupMenuItem",
            "DropdownMenu",
            "ContextMenuExt",
            "popover",
            "Popover",
            "BasePopover",
            "hover_card",
            "HoverCard",
            "BaseHoverCard",
        ],
        allowed: &["menu_parity_test.rs"],
    },
    Row {
        stage: "5a dock skin",
        crates: &["gpui_component"],
        names: &["dock"],
        allowed: &["dock_engine_parity_test.rs"],
    },
    Row {
        stage: "5b dock engine",
        crates: &["gpui_base"],
        names: &["dock", "PANEL_MIN_SIZE"],
        allowed: &["dock_engine_parity_test.rs"],
    },
    Row {
        stage: "4a small pieces",
        crates: &["gpui_component", "gpui_base"],
        names: &[
            "spinner",
            "Spinner",
            "clipboard",
            "Clipboard",
            "status_bar",
            "StatusBar",
            "notification",
            "Notification",
        ],
        allowed: &["small_pieces_parity_test.rs"],
    },
    Row {
        stage: "7a diff viewer",
        crates: &["gpui_component"],
        names: &["diff", "Diff", "DiffFile", "DiffState"],
        allowed: &[],
    },
];

fn uses(src: &str, crates: &[&str]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (n, line) in src.lines().enumerate() {
        let t = line.trim_start();
        let t_use = t.strip_prefix("pub ").unwrap_or(t);
        if open.is_none()
            && crates
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

fn names_after_crate_path<'a>(line: &'a str, crates: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for c in crates {
        let prefix = format!("{c}::");
        for (i, _) in line.match_indices(&prefix) {
            out.extend(words(&line[i + prefix.len()..]).next());
        }
    }
    out
}

fn hits_in(src: &str, rel: &str, row: &Row) -> Vec<String> {
    let mut hits = Vec::new();
    for (n, text) in uses(src, row.crates) {
        for word in row.names {
            if words(&text).any(|w| w == *word) {
                hits.push(format!("{}: {rel}:{n}: imports {word}", row.stage));
            }
        }
    }
    for (n, line) in src.lines().enumerate() {
        for name in names_after_crate_path(line, row.crates) {
            if row.names.contains(&name) {
                hits.push(format!("{}: {rel}:{}: names {name}", row.stage, n + 1));
            }
        }
    }
    hits
}

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn src_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn each_stage_s_names_come_from_our_kit() {
    let root = src_root();
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut hits = Vec::new();
    for path in &files {
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if ["kit_boundaries_test.rs", "kit_boundary_test.rs"].contains(&file) {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        let src = fs::read_to_string(path).expect("read source");
        for row in ROWS.iter().filter(|r| !r.allowed.contains(&file)) {
            hits.extend(hits_in(&src, &rel, row));
        }
    }
    assert!(
        hits.is_empty(),
        "{} places still use what the kit took over:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[test]
fn one_scanner_holds_every_stage() {
    for old in [
        "ui/theme_boundary_test.rs",
        "ui/layout_icon_boundary_test.rs",
    ] {
        assert!(
            !src_root().join(old).exists(),
            "{old} moved into kit_boundaries_test.rs"
        );
    }
}

#[test]
fn the_theme_bridge_only_writes_gpui_component_s_theme() {
    let src = fs::read_to_string(src_root().join("ui/theme_bridge.rs"))
        .expect("the bridge is ui/theme_bridge.rs");
    let reads: Vec<_> = src
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("Theme::global(") || l.contains(".theme()"))
        .map(|(n, _)| n + 1)
        .collect();
    assert!(
        reads.is_empty(),
        "theme_bridge.rs reads gpui-component's theme at lines {reads:?}"
    );
}

#[test]
fn the_scan_sees_imports_and_paths_but_not_longer_names() {
    let row = &ROWS[2];
    let src = "use gpui_component::{\n    Sizable as _,\n    menu::{DropdownMenu as _, PopupMenuItem},\n};\n\
               use gpui_component::native_menu;\n\
               let m = gpui_component::menu::PopupMenu::build(w, cx, f);\n\
               pub use gpui_base::HoverCard;\n";
    assert_eq!(
        hits_in(src, "f.rs", row),
        [
            "3 menus and popovers: f.rs:1: imports menu",
            "3 menus and popovers: f.rs:1: imports PopupMenuItem",
            "3 menus and popovers: f.rs:1: imports DropdownMenu",
            "3 menus and popovers: f.rs:7: imports HoverCard",
            "3 menus and popovers: f.rs:6: names menu",
            "3 menus and popovers: f.rs:7: names HoverCard",
        ]
    );
    let longer = ["use gpui_component::", "ThemeColorX;"].concat();
    assert!(hits_in(&longer, "f.rs", &ROWS[0]).is_empty());
}
