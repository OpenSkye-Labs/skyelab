//! Acceptance: macOS keeps SkyeLab running with every window closed. A task that comes due then opens a window (the
//! one the dock would) and runs in it; with nothing due, no window opens.

use super::super::threads_test::{fake_bin, wait_until};
use super::super::windows::{self, window_handles};
use super::super::{Backend, Workspace};
use super::test_util::{HOUR, NOW, open, rig, runs, task, tick};
use gpui::{Bounds, TestAppContext, point, px, size};
use skyelab_core::AccountsConfig;
use std::cell::Cell;
use std::rc::Rc;

#[gpui::test]
fn a_due_task_opens_a_window_and_runs(cx: &mut TestAppContext) {
    let rig = rig(true);
    let later = task(&rig, "every 1h", NOW + HOUR);
    let (ws, cx) = open(&rig, cx);
    let window = cx.update(|window, _| window.window_handle());
    super::super::multi_window_test::in_window(window, &ws, cx, |ws, window, cx| {
        ws.claim_hub(window, cx)
    });
    let opened = Rc::new(Cell::new(0));
    let (count, store, accounts) = (
        opened.clone(),
        rig.f.store.clone(),
        rig.f.accounts_path.clone(),
    );
    let folder = rig.f.project_dir.parent().expect("parent").to_path_buf();
    cx.update(|_, cx| {
        super::set_opener(
            Rc::new(move |cx: &mut gpui::App| {
                count.set(count.get() + 1);
                let (store, accounts, folder) = (store.clone(), accounts.clone(), folder.clone());
                let at = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(700.)));
                windows::open(
                    at,
                    move |window, cx| {
                        let config = AccountsConfig::load(&accounts).expect("load");
                        let backend = Backend {
                            store,
                            claude_bin: fake_bin(),
                        };
                        Workspace::new(folder, Some(accounts), config, None, backend, window, cx)
                    },
                    cx,
                )
                .expect("open");
            }),
            cx,
        )
    });
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let handles =
        |cx: &mut gpui::VisualTestContext| TestAppContext::update(cx, |cx| window_handles(cx));
    assert!(handles(cx).is_empty(), "no window left");

    // Nothing due: the tick opens nothing.
    TestAppContext::update(cx, super::tick);
    assert_eq!(opened.get(), 0, "a window opened with nothing due");

    // Due now: one window opens and the run starts in it.
    TestAppContext::update(cx, |cx| super::set_now(NOW + HOUR, cx));
    TestAppContext::update(cx, super::tick);
    assert_eq!(opened.get(), 1, "the due task opened a window");
    let handles = handles(cx);
    assert_eq!(handles.len(), 1);
    let mut wcx = gpui::VisualTestContext::from_window(handles[0], cx);
    wait_until(&mut wcx, "the run", |_| runs(&rig, later.id).len() == 1);
    tick(&mut wcx);
    assert_eq!(opened.get(), 1, "the open window serves later ticks");
}
