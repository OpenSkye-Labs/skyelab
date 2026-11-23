//! The palette's sharing toggle says whether sharing is on, as T3's toggle rows do.

use super::calls::sharing_workspace;
use super::*;

fn sharing_entry(f: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<bool> {
    f.update_in(cx, |ws, window, cx| {
        let focus = gpui::Focusable::focus_handle(ws, cx);
        ws.palette_entries(&focus, window, cx)
            .into_iter()
            .find(|e| e.label == "Toggle Sharing on Tailnet")
            .expect("the sharing entry is listed")
            .checked
    })
}

#[gpui::test]
fn sharing_entry_shows_state(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx); // sharing on
    assert_eq!(sharing_entry(&f.ws, cx), Some(true));
    f.remote.update(cx, |r, cx| r.toggle(cx));
    assert_eq!(sharing_entry(&f.ws, cx), Some(false));
}
