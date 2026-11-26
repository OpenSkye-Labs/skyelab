//! Debug builds: the palette lists Open UI Gallery from a real workspace, and choosing it opens the gallery window.

use super::palette_test::{open, setup};
use super::threads_test::test_backend;
use crate::palette::Kind;
use gpui::TestAppContext;

#[gpui::test]
fn the_palette_lists_and_runs_open_ui_gallery(cx: &mut TestAppContext) {
    setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    let entries = ws.update_in(cx, |ws, window, cx| {
        let prev = ws.focus.clone();
        ws.palette_entries(&prev, window, cx)
    });
    let gallery = entries
        .iter()
        .find(|e| matches!(e.kind, Kind::Action("skyelab::OpenUiGallery")))
        .expect("GALLERY_NOT_IN_PALETTE");
    let run = gallery.run.clone();
    let before = cx.update(|_, cx| cx.windows().len());
    cx.update(|window, cx| run(window, cx));
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.windows().len()), before + 1, "the gallery window opened");
}
