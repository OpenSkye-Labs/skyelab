//! Own UI kit, stage 2a (.agent/plans/2026-11-27-own-ui-kit-theme.md), criterion 5: the monospace family is the
//! platform default when installed, else the first installed alternate, else the system UI font.

use super::theme_load::first_installed;

fn installed(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

#[test]
fn keeps_the_default_when_installed() {
    let got = first_installed(
        "Menlo",
        &["Monaco", "Courier New"],
        &installed(&["Monaco", "Menlo"]),
    );
    assert_eq!(got.as_ref(), "Menlo");
}

#[test]
fn takes_the_first_installed_alternate_in_order() {
    let got = first_installed(
        "Menlo",
        &["Monaco", "Courier New"],
        &installed(&["Courier New", "Monaco"]),
    );
    assert_eq!(got.as_ref(), "Monaco");
}

#[test]
fn falls_back_to_the_system_ui_font() {
    let got = first_installed("Menlo", &["Monaco"], &installed(&["Helvetica"]));
    assert_eq!(got.as_ref(), ".SystemUIFont");
}
