//! The image dialog opens over the window, zooms in steps over the fit, and closes on Escape.

use super::image_viewer::{self, ZOOMS, zoom_step};
use super::palette_test::{open, setup};
use super::threads_test::test_backend;
use gpui::{Image, ImageFormat, TestAppContext};
use std::sync::Arc;

#[test]
fn zoom_steps_hold_at_the_ends() {
    assert_eq!(zoom_step(0, -1), 0);
    assert_eq!(zoom_step(0, 1), 1);
    assert_eq!(zoom_step(ZOOMS.len() - 1, 1), ZOOMS.len() - 1);
    assert_eq!(ZOOMS[0], 1.0, "step 0 is Fit");
}

#[test]
fn only_image_attachments_are_kept() {
    let png = [0x89, b'P', b'N', b'G', 0, 0, 0, 0].to_vec();
    let files = [
        skyelab_agent::Attachment::new("a.png", png).expect("png"),
        skyelab_agent::Attachment::new("notes.txt", b"hi".to_vec()).expect("text"),
    ];
    let kept = image_viewer::images_of(&files);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].0, "a.png");
    assert_eq!(kept[0].1.format, ImageFormat::Png);
}

#[gpui::test]
fn image_dialog_opens_and_escape_closes_it(cx: &mut TestAppContext) {
    setup(cx);
    let (_ws, cx) = open(test_backend(), cx);
    let image = Arc::new(Image::from_bytes(ImageFormat::Png, Vec::new()));
    cx.update(|window, cx| image_viewer::open(image, "a.png".into(), window, cx));
    cx.run_until_parked();
    assert!(
        cx.update(crate::ui::has_active_dialog),
        "the viewer is open"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        !cx.update(crate::ui::has_active_dialog),
        "escape closes it"
    );
}
