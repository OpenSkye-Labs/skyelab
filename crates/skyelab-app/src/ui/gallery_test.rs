//! The UI Gallery opens in a window of its own whose root view is the gallery (the palette entry that opens it is
//! `workspace::palette_gallery_test`).

use gpui::TestAppContext;

#[gpui::test]
fn the_gallery_opens_in_its_own_window(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let before = cx.update(|cx| cx.windows().len());
    let handle = cx
        .update(super::gallery::open)
        .expect("the gallery window opens");
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.windows().len()), before + 1);
    let gallery = handle
        .update(cx, |root, _, _| {
            root.view()
                .clone()
                .downcast::<super::gallery::UiGallery>()
                .is_ok()
        })
        .expect("window alive");
    assert!(gallery, "the window's root view is the gallery");
}
