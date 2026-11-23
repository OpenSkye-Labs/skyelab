//! Review fixes on the Preview image: its format comes from its bytes (a JPEG named `.png` still draws, as GPUI's
//! path loader drew it), and a replaced image leaves GPUI's asset cache instead of piling up one decode per revision.

use super::*;

fn encoded(format: image::ImageFormat) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(2, 2, image::Rgb([9, 9, 9]))
        .write_to(&mut out, format)
        .expect("encode");
    out.into_inner()
}

#[test]
fn the_format_comes_from_the_bytes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("shot.png");
    std::fs::write(&file, encoded(image::ImageFormat::Jpeg)).expect("write");
    assert_eq!(image(&file).expect("drawn").format, gpui::ImageFormat::Jpeg);
    std::fs::write(&file, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").expect("write");
    assert!(image(&file).is_err(), "SVG bytes are never drawn");
}

#[gpui::test]
fn a_replaced_image_leaves_the_cache(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = std::fs::canonicalize(dir.path()).expect("root");
    std::fs::write(root.join("a.png"), encoded(image::ImageFormat::Png)).expect("write");
    cx.update(gpui_component::init);
    let (panel, cx) = cx.add_window_view(PreviewPanel::new);
    let show = |cx: &mut gpui::VisualTestContext| {
        panel.update_in(cx, |p, window, cx| {
            p.show(root.clone(), "a.png".into(), None, window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |p, _| match &p.body {
            Body::Image(image) => image.clone(),
            _ => panic!("an image"),
        })
    };
    let first = show(cx);
    cx.update(|window, _| window.refresh()); // drawn: the panel's img() decodes it into the cache
    cx.run_until_parked();
    assert!(cx.update(|_, cx| first.is_asset_cached(cx)), "drawn once");
    std::fs::write(root.join("a.png"), encoded(image::ImageFormat::Bmp)).expect("rewrite");
    let second = show(cx);
    assert_ne!(first.id(), second.id());
    assert!(
        !cx.update(|_, cx| first.is_asset_cached(cx)),
        "the old decode is dropped"
    );
}
