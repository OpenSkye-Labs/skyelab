//! Acceptance: New thread never opens the system folder picker. With no projects it opens Add project; with several,
//! the New thread in page ends with Add project….

use super::add_project_rig::{dialog_open, keys, rig, shown};
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn new_thread_with_no_projects_opens_add_project(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    let focus = rig.ws.read_with(cx, |ws, _| ws.focus.clone());
    cx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(NewThread), cx);
    });
    cx.run_until_parked();
    assert!(dialog_open(cx));
    assert_eq!(shown(cx).page.as_deref(), Some("Add project"));
}

#[gpui::test]
fn new_thread_in_lists_add_project_last(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    for dir in ["code/app", "code/api"] {
        let dir = rig.home.join(dir);
        rig.ws.update(cx, |ws, cx| ws.add_project(dir.clone(), cx));
        rig.wait_project(&dir, cx);
    }
    rig.ws.update(cx, |ws, _| ws.scope.clear());
    let focus = rig.ws.read_with(cx, |ws, _| ws.focus.clone());
    cx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(NewThread), cx);
    });
    cx.run_until_parked();
    let s = shown(cx);
    assert_eq!(s.page.as_deref(), Some("New thread in"));
    assert_eq!(s.rows.last().map(|r| r.1.as_str()), Some("Add project…"));
    keys("up enter", cx);
    assert_eq!(shown(cx).page.as_deref(), Some("Add project"), "it opens the page in place");
}
