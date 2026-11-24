//! The UI Gallery: OpenUiGallery opens a window drawing every section, and the palette lists the action.

use gpui::TestAppContext;

#[gpui::test]
fn open_ui_gallery_opens_a_window_with_every_section(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    cx.update(super::gallery::init);
    let before = cx.update(|cx| cx.windows().len());
    cx.update(|cx| cx.dispatch_action(&super::gallery::OpenUiGallery));
    cx.run_until_parked();
    let windows = cx.update(|cx| cx.windows());
    assert_eq!(windows.len(), before + 1, "a gallery window opened");
    let handle = windows
        .last()
        .copied()
        .expect("the gallery window")
        .downcast::<gpui_component::Root>()
        .expect("rooted");
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

#[gpui::test]
fn the_palette_lists_open_ui_gallery_in_debug_builds(cx: &mut TestAppContext) {
    cx.update(super::gallery::init);
    let names = cx.update(|cx| cx.all_action_names().to_vec());
    assert!(names.contains(&"skyelab::OpenUiGallery"));
    assert!(crate::palette::listed("skyelab::OpenUiGallery"));
}
