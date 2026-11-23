//! An SVG can pull in files from anywhere (`<image href>`), so it is never drawn as an image.

use skyelab_core::files::{Preview, read_preview};
use std::path::Path;

#[test]
fn svg_with_external_href_is_not_drawn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image xlink:href="/etc/outside.png" width="10" height="10"/></svg>"#;
    std::fs::write(dir.path().join("x.svg"), svg).expect("write svg");
    let p = read_preview(dir.path(), Path::new("x.svg")).expect("preview");
    assert!(
        !matches!(p, Preview::Image(_)),
        "svg with external href rendered as image: {p:?}"
    );
}
