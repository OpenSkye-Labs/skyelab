//! The render's caps: an oversized source, a diagram too big to draw at half scale and a bad source each come back
//! as a reason for the card to show, never a panic or an unbounded image.

use super::mermaid::{MAX_SIDE, MAX_SOURCE, Palette, rasterize, render_png};
use gpui::{black, white};

fn palette() -> Palette {
    Palette::new(white(), white(), white(), black(), black(), black())
}

#[test]
fn small_flowchart_draws_at_twice_its_size() {
    let png = render_png("flowchart LR\n  A[Start] --> B[End]", palette()).expect("draws");
    let image = image::load_from_memory(&png.bytes).expect("a PNG");
    assert!(png.width > 10.0 && png.height > 10.0);
    assert_eq!(image.width(), (png.width * 2.0).ceil() as u32);
}

#[test]
fn oversized_source_cap_refuses_without_rendering() {
    let source = format!(
        "flowchart LR\n{}",
        "  A --> B\n".repeat(MAX_SOURCE / 10 + 1)
    );
    assert!(source.len() > MAX_SOURCE);
    let err = render_png(&source, palette()).expect_err("too big");
    assert!(err.contains("too large"), "{err}");
}

#[test]
fn oversized_raster_cap_scales_then_refuses() {
    let svg = |w: f32| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="40"><rect width="{w}" height="40" fill="red"/></svg>"#
        )
    };
    // Over MAX_SIDE at 2x: drawn smaller, but never wider than MAX_SIDE pixels.
    let png = rasterize(&svg(3000.0)).expect("drawn at a lower scale");
    let image = image::load_from_memory(&png.bytes).expect("a PNG");
    assert!(image.width() as f32 <= MAX_SIDE, "{}", image.width());
    assert_eq!(png.width, 3000.0);
    // Readable only below half scale: refused.
    let err = rasterize(&svg(MAX_SIDE * 2.0 + 10.0)).expect_err("too wide");
    assert!(err.contains("too large"), "{err}");
}

#[test]
fn bad_source_falls_back_with_a_reason() {
    let err = render_png("this is not a diagram ->->", palette()).expect_err("no diagram");
    assert!(!err.is_empty());
}
