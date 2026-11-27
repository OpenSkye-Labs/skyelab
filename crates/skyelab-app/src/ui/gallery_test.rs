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
        .update(cx, |root, _, cx| {
            root.view()
                .clone()
                .downcast::<super::window_root::WindowRoot>()
                .is_ok_and(|ours| {
                    ours.read(cx)
                        .content()
                        .clone()
                        .downcast::<super::gallery::UiGallery>()
                        .is_ok()
                })
        })
        .expect("window alive");
    assert!(gallery, "the window's root view is the gallery");
}

/// The loading button's spinner turns only under the pointer: an open, idle gallery asks for no animation frames.
#[gpui::test]
fn the_loading_spinner_turns_only_while_hovered(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let handle = cx
        .update(super::gallery::open)
        .expect("the gallery window opens");
    cx.run_until_parked();
    let frames = |cx: &mut TestAppContext| {
        handle
            .update(cx, |_, window, _| window.refresh())
            .expect("window alive");
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("window alive")
    };
    assert_eq!(frames(cx), 0, "an idle gallery animates nothing");

    let mut vcx = gpui::VisualTestContext::from_window(handle.into(), cx);
    let at = vcx
        .debug_bounds("g-loading")
        .expect("the loading button is drawn")
        .center();
    vcx.simulate_mouse_move(at, None, gpui::Modifiers::none());
    vcx.run_until_parked();
    assert!(frames(cx) > 0, "the hovered spinner turns");
}
