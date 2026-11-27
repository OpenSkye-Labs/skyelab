//! Settings → Sharing & access, open, redraws when a delegation request arrives (bug: it showed only after the
//! page was left and opened again, while the status bar already said so).

use super::remote_delegation_harness::*;
use crate::debug_renders::take_test_count;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_protocol::DelegateOp;

#[gpui::test]
fn the_open_sharing_page_redraws_when_a_request_arrives(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            real_window: true,
            ..Setup::default()
        },
        cx,
    );
    b.ws.update_in(cx, |ws, window, cx| {
        ws.open_settings_at(crate::settings::Section::Sharing, window, cx)
    });
    cx.run_until_parked();
    assert!(take_test_count("RemotePanel") > 0, "the page is on screen");
    assert_eq!(denies(cx), 0);
    call(&b, &caller(), DelegateOp::ListProjects, cx).expect_err("no link yet");
    wait_until(cx, "the request listed", |cx| {
        b.remote.update(cx, |r, cx| r.refresh_peers(cx));
        b.remote.read_with(cx, |r, _| !r.delegation_requests.is_empty())
    });
    cx.run_until_parked();
    assert!(take_test_count("RemotePanel") > 0, "the open page drew the request");
    assert_eq!(denies(cx), 1, "the request's Deny is on screen");
}

fn denies(cx: &mut gpui::VisualTestContext) -> usize {
    cx.update(|window, _| gpui_base::test_support::snapshots(window))
        .iter()
        .filter(|s| s.label() == Some("Deny"))
        .count()
}
