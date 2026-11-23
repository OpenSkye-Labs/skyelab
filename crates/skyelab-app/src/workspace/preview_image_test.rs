//! Acceptance: the Preview tab shows an image file's current content. GPUI caches an image loaded by path for the
//! whole session, so the preview reads the bytes and draws those: an image rewritten on disk shows anew.

use super::*;

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(2, 2, image::Rgb(rgb))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode");
    out.into_inner()
}

fn shown(
    panel: &Entity<PreviewPanel>,
    cx: &mut gpui::VisualTestContext,
) -> Option<Arc<gpui::Image>> {
    panel.read_with(cx, |p, _| match &p.body {
        Body::Image(image) => Some(image.clone()),
        _ => None,
    })
}

#[gpui::test]
fn an_edited_image_shows_its_new_content(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = std::fs::canonicalize(dir.path()).expect("root");
    std::fs::write(root.join("shot.png"), png([255, 0, 0])).expect("write");
    cx.update(gpui_component::init);
    let (panel, cx) = cx.add_window_view(PreviewPanel::new);
    let show = |cx: &mut gpui::VisualTestContext| {
        panel.update_in(cx, |p, window, cx| {
            p.show(root.clone(), "shot.png".into(), None, window, cx)
        });
        cx.run_until_parked();
        shown(&panel, cx).expect("an image")
    };
    let first = show(cx);
    assert_eq!(first.bytes(), png([255, 0, 0]).as_slice());

    std::fs::write(root.join("shot.png"), png([0, 0, 255])).expect("rewrite");
    let second = show(cx);
    assert_eq!(second.bytes(), png([0, 0, 255]).as_slice(), "the new bytes");
    assert_ne!(first.id(), second.id(), "a new image, not the cached one");
}

#[test]
fn svg_is_never_drawn_as_an_image() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("x.svg");
    std::fs::write(&file, "<svg/>").expect("write");
    assert!(image(&file).is_err());
}
