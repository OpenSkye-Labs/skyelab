//! A diagram never loads an image: an `<image href>` to a file on disk or a data URI draws nothing. The target is an
//! SVG, which usvg would load and draw with no raster feature at all.

use super::mermaid::rasterize;

const RED: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#;

fn red_pixels(png: &[u8]) -> usize {
    let image = image::load_from_memory(png).expect("a PNG").to_rgba8();
    image
        .pixels()
        .filter(|p| p[0] > 200 && p[1] < 50 && p[3] > 200)
        .count()
}

#[test]
fn image_href_never_loads() {
    let dir = tempfile::tempdir().expect("temp dir");
    let red = dir.path().join("red.svg");
    std::fs::write(&red, RED).expect("write red.svg");
    let data = format!(
        "data:image/svg+xml;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, RED)
    );
    for href in [
        red.display().to_string(),
        format!("file://{}", red.display()),
        data,
    ] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="40"><image width="40" height="40" href="{href}" xlink:href="{href}"/></svg>"#
        );
        let png = rasterize(&svg).expect("draws");
        assert_eq!(red_pixels(&png.bytes), 0, "{href} loaded");
    }
}
