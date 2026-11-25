//! Acceptance: with every window closed (macOS keeps running, agents included), Quit opens a window and asks there
//! instead of quitting while a thread still waits.

use super::manage_harness::rig;
use super::quit_guard::{Blocker, TestQuits, blockers, request_quit};
use super::threads_test::wait_until;
use super::*;
use gpui::TestAppContext;
use gpui_component::{Root, WindowExt as _};

#[gpui::test]
fn quit_with_no_window_open_still_asks(cx: &mut TestAppContext) {
    let (r, vcx) = rig(cx);
    vcx.update(|_, cx| cx.set_global(TestQuits(0)));
    vcx.dispatch_action(NewThread);
    vcx.run_until_parked();
    let view = r
        .ws
        .read_with(vcx, |ws, _| ws.open_threads.last().cloned())
        .expect("a thread tab");
    view.update_in(vcx, |v, window, cx| {
        v.mode = skyelab_agent::Mode::Default; // a new thread's Full access would skip the ask
        v.submit_remote("permit", window, cx)
    })
        .expect("sent");
    wait_until(vcx, "an approval", |cx| {
        cx.update(|_, cx| {
            blockers(cx)
                .iter()
                .any(|(_, b)| matches!(b, Blocker::Waiting(_)))
        })
    });
    // The app's opener for a window when none is left, as main installs it.
    let (backend, accounts) = r.ws.read_with(vcx, |ws, _| {
        (ws.backend.clone(), ws.accounts_path.clone())
    });
    let folder = r.f.project_dir.clone();
    let opener: std::rc::Rc<dyn Fn(&mut gpui::App)> = std::rc::Rc::new(move |cx| {
        let (backend, accounts, folder) = (backend.clone(), accounts.clone(), folder.clone());
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            let config = AccountsConfig::default();
            let ws = cx.new(|cx| {
                Workspace::new(folder, accounts, config, None, backend, window, cx)
            });
            cx.new(|cx| Root::new(ws, window, cx))
        })
        .expect("a window opens");
    });
    vcx.update(|_, cx| cx.set_global(crate::FreshWindow(opener)));
    vcx.update(|window, _| window.remove_window());
    vcx.run_until_parked();

    let (left, quits) = TestAppContext::update(vcx, |cx| {
        let left = blockers(cx).len();
        request_quit(cx);
        (left, cx.try_global::<TestQuits>().map_or(0, |q| q.0))
    });
    assert_eq!(left, 1, "the thread still waits with no window");
    assert_eq!(quits, 0, "Quit asks instead of quitting");
    vcx.run_until_parked();
    let asked = TestAppContext::update(vcx, |cx| {
        let windows = super::window_handles(cx);
        windows.iter().any(|w| {
            w.update(cx, |_, window, cx| window.has_active_dialog(cx))
                .unwrap_or(false)
        })
    });
    assert!(asked, "a new window shows the question");
}
