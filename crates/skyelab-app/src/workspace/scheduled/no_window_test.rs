//! Acceptance: in background mode (and on macOS with every window closed), the hidden hub window takes the app-wide
//! work. A task that comes due then runs there without opening a window (the opener stays a fallback).

use super::super::threads_test::{fake_bin, wait_until};
use super::super::windows::{self, window_handles};
use super::super::{Backend, Workspace};
use super::test_util::{HOUR, NOW, open, rig, runs, task, tick};
use gpui::{Bounds, TestAppContext, point, px, size};
use skyelab_core::AccountsConfig;
use std::cell::Cell;
use std::rc::Rc;

#[gpui::test]
fn a_due_task_runs_in_the_hidden_hub(cx: &mut TestAppContext) {
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
    TestAppContext::update(cx, super::super::background::enter);
    cx.run_until_parked();
    let handles =
        |cx: &mut gpui::VisualTestContext| TestAppContext::update(cx, |cx| window_handles(cx));
    assert!(handles(cx).is_empty(), "no window left");

    // Nothing due: the tick opens nothing.
    TestAppContext::update(cx, super::tick);
    assert_eq!(opened.get(), 0, "a window opened with nothing due");

    // Due now: the run starts in the hidden hub, and no window opens.
    let hub = TestAppContext::update(cx, |cx| windows::hub_workspace(cx).map(|h| h.0));
    let hub = hub.expect("the hidden hub serves the work");
    assert!(TestAppContext::update(cx, |cx| windows::is_hidden(hub, cx)));
    TestAppContext::update(cx, |cx| super::set_now(NOW + HOUR, cx));
    TestAppContext::update(cx, super::tick);
    let mut wcx = gpui::VisualTestContext::from_window(hub, cx);
    wait_until(&mut wcx, "the run", |_| runs(&rig, later.id).len() == 1);
    tick(&mut wcx);
    assert_eq!(opened.get(), 0, "no window opened");
    assert!(handles(&mut wcx).is_empty(), "none shown");
}
