//! Own UI kit (.agent/plans/2026-11-27-own-ui-kit.md), criterion 1: dialogs are SkyeLab's own. No source file, tests
//! included, reaches gpui-component's dialog or its `WindowExt` dialog calls. Later stages add
//! their names to `kit_boundaries_test.rs`.

use std::{fs, path::Path};

/// Text that only code using gpui-component's dialog contains.
const BANNED: &[&str] = &[
    "gpui_component::dialog",
    "WindowExt::open_dialog",
    "WindowExt::close_dialog",
    "WindowExt::has_active_dialog",
    ".open_dialog(cx",
    ".close_dialog(cx)",
    ".has_active_dialog(cx)",
];

fn scan(dir: &Path, root: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, root, hits);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") || path.ends_with("kit_boundary_test.rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read source");
        for (n, line) in src.lines().enumerate() {
            for banned in BANNED {
                if line.contains(banned) {
                    let rel = path.strip_prefix(root).unwrap_or(&path).display();
                    hits.push(format!("{rel}:{}: {banned}", n + 1));
                }
            }
        }
    }
}

#[test]
fn dialogs_never_reach_gpui_component() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&root, &root, &mut hits);
    assert!(
        hits.is_empty(),
        "gpui-component's dialog is still used:\n{}",
        hits.join("\n")
    );
}
