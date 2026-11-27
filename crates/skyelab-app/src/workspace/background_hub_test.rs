//! Acceptance 2 and 7: with no window shown, the hidden hub serves a delegating agent's `start_thread` and raises
//! the child's notice, and showing that thread opens a visible window on it, which takes the hub back over.

use super::background;
use super::background_keep_test::hidden_hub;
use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use super::windows::{self, window_handles};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn the_hidden_hub_serves_calls_and_opens_the_thread(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (a, vcx) = open(&rig, accounts, config, cx);
    let parent = start(&a, rig.work_a, "plain work", vcx);
    let parent_id = thread_id(&parent, vcx);
    // The app's opener for a window when none is shown, as main installs it.
    let (backend, path, folder) = a.read_with(vcx, |ws, _| {
        (ws.backend.clone(), ws.accounts_path.clone(), ws.folder.clone())
    });
    let opener: std::rc::Rc<dyn Fn(&mut gpui::App)> = std::rc::Rc::new(move |cx| {
        let (backend, path, folder) = (backend.clone(), path.clone(), folder.clone());
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            let config = AccountsConfig::default();
            let ws = cx.new(|cx| Workspace::new(folder, path, config, None, backend, window, cx));
            cx.new(|cx| crate::ui::window_root(ws.into(), window, cx))
        })
        .expect("a window opens");
    });
    TestAppContext::update(vcx, |cx| cx.set_global(crate::FreshWindow(opener)));
    TestAppContext::update(vcx, background::enter);
    vcx.run_until_parked();
    assert!(TestAppContext::update(vcx, |cx| window_handles(cx).is_empty()));
    let hub = hidden_hub(vcx).expect("the hidden hub");
    let mut hcx = VisualTestContext::from_window(hub, cx);
    let hub_ws = hcx
        .update(|_, cx| windows::hub_workspace(cx))
        .map(|(w, ws)| (w == hub).then_some(ws))
        .expect("a hub")
        .expect("the hidden window serves the calls");

    let child = host_call(
        &mut hcx,
        Caller {
            thread_id: parent_id,
            account_id: "work".into(),
        },
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "plain work".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("the hub started the child");
    let child = child["thread_id"].as_i64().expect("the child's id");
    let note: SharedString = format!("thread {child}").into();
    wait_until(&mut hcx, "the child's notice from the hidden hub", |cx| {
        hub_ws.read_with(cx, |ws, _| ws.notices.contains(&note))
    });

    // The notice's click: a visible window opens on the thread and the hidden hub closes.
    TestAppContext::update(&hcx, |cx| background::show_thread(child, cx));
    let shown = TestAppContext::update(&hcx, |cx| window_handles(cx));
    assert_eq!(shown.len(), 1, "a window shows");
    let mut scx = VisualTestContext::from_window(shown[0], cx);
    scx.run_until_parked();
    let ws = scx
        .update(|_, cx| windows::hub_workspace(cx))
        .expect("a hub")
        .1;
    assert_eq!(ws.read_with(&scx, |ws, _| ws.window_handle), shown[0], "it serves the calls now");
    assert!(
        ws.read_with(&scx, |ws, cx| ws.shows_thread(child, cx)),
        "it shows the thread"
    );
    assert!(hidden_hub(&mut scx).is_none(), "the hidden hub closed");
}
