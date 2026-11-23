//! Two workspace windows on one store: one thread view (one agent session) per thread, per-window docks and pages,
//! accounts reloads reaching both, one owner for a view's side effects, and the call queues handed on at close.

use super::center::Page;
use super::thread::ThreadViewEvent;
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext};
use gpui_component::dock::DockPlacement;

/// Runs `f` on `ws` inside its own window (`cx` may belong to another window).
fn in_window<R>(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) -> R,
) -> R {
    if cx.update(|window, _| window.window_handle()) == handle {
        return ws.update_in(cx, f);
    }
    cx.update(|_, app| {
        handle
            .update(app, |_, window, app| {
                ws.update(app, |ws, cx| f(ws, window, cx))
            })
            .expect("window open")
    })
}

/// A stored thread with a session to resume (so it isn't read-only), in a stored project.
fn seed(store: &SharedStore, dir: &std::path::Path) -> i64 {
    let s = lock(store).expect("store");
    let project = s.create_project("demo", dir).expect("project");
    let thread = s
        .create_thread(&skyelab_store::NewThread {
            project_id: project.id,
            title: "shared",
            cwd: dir,
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    s.set_agent_session_id(thread.id, Some("fake-sess-1"))
        .expect("session id");
    thread.id
}

#[gpui::test]
fn two_windows_share_one_thread_view(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let id = seed(&f.store, &f.project_dir);
    let (orch_tx, orch_rx) = flume::unbounded();
    cx.update(|cx| set_hub(None, Some(orch_rx), cx));
    let (a, cx) = open_workspace(&f, cx);
    let a_window = a.read_with(cx, |ws, _| ws.window_handle);
    in_window(a_window, &a, cx, |ws, window, cx| ws.claim_hub(window, cx));

    // 1, 12: New Window opens a second workspace on the same store and backend.
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();
    let handles = cx.update(|_, cx| window_handles(cx));
    assert_eq!(handles.len(), 2, "New Window opened a window");
    let b_window = handles[1];
    let b = cx.update(|_, cx| windows::workspaces(cx)[1].clone());
    let same_store =
        cx.update(|_, cx| Arc::ptr_eq(&a.read(cx).backend.store, &b.read(cx).backend.store));
    assert!(same_store, "both windows use one store");
    // 15: a window opens no orchestration server of its own.
    assert!(cx.update(|_, cx| !cx.has_global::<orchestrate::Orchestration>()));

    // 2: the thread opened in both windows is one view.
    cx.dispatch_action(OpenThread { id });
    cx.run_until_parked();
    in_window(b_window, &b, cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    let (va, vb) = cx.update(|_, cx| (a.read(cx).open_view(id, cx), b.read(cx).open_view(id, cx)));
    let (va, vb) = (va.expect("open in A"), vb.expect("open in B"));
    assert_eq!(
        va.entity_id(),
        vb.entity_id(),
        "one view, one agent session"
    );
    assert_eq!(
        cx.update(|_, cx| va.read(cx).window),
        b_window,
        "the window that showed it last owns it"
    );

    // 3: docks are per window. (B took the shared prompt, and with it A's focus.)
    in_window(a_window, &a, cx, |ws, window, cx| {
        ws.focus.focus(window, cx)
    });
    cx.dispatch_action(ToggleSidebar);
    cx.run_until_parked();
    let (left_a, left_b) = cx.update(|_, cx| {
        (
            layout::docks_of(a_window, cx).left,
            layout::docks_of(b_window, cx).left,
        )
    });
    assert!(
        !left_a && left_b,
        "A hid its sidebar, B kept its own: {left_a} {left_b}"
    );
    let b_left = cx.update(|_, cx| b.read(cx).dock.read(cx).is_dock_open(DockPlacement::Left));
    assert!(b_left);

    // 5: only the owner (B) tells the user a turn ended; neither window shows the thread now.
    cx.dispatch_action(OpenSettings);
    in_window(b_window, &b, cx, |ws, window, cx| {
        ws.show(Page::Welcome, window, cx)
    });
    cx.run_until_parked();
    va.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    cx.run_until_parked();
    let (na, nb) = cx.update(|_, cx| (a.read(cx).notices.len(), b.read(cx).notices.len()));
    assert_eq!((na, nb), (0, 1), "one notification, from the owner");

    // 12: pages are per window.
    let pages = cx.update(|_, cx| (a.read(cx).page.clone(), b.read(cx).page.clone()));
    assert!(matches!(pages.0, Page::Settings(_)) && pages.1 == Page::Welcome);

    // 4: an accounts change saved in A reaches B.
    let unbound = AccountsConfig::load(&f.accounts_path).expect("load");
    let mut unbound = unbound;
    unbound.bindings.clear();
    unbound.save(&f.accounts_path).expect("save");
    let settings = cx
        .update(|_, cx| a.read(cx).settings.clone())
        .expect("settings open");
    settings.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let b_bound = cx.update(|_, cx| b.read(cx).config.resolve(&f.project_dir).is_some());
    assert!(!b_bound, "B reloaded the accounts A saved");

    // 2: closing (archiving) it in one window removes it from both.
    in_window(a_window, &a, cx, |ws, window, cx| {
        ws.close_thread(&va, window, cx)
    });
    cx.run_until_parked();
    let left = cx.update(|_, cx| (a.read(cx).open_view(id, cx), b.read(cx).open_view(id, cx)));
    assert!(
        left.0.is_none() && left.1.is_none(),
        "closed in both windows"
    );

    // 6: A serves the call queues; when it closes, B takes them over.
    assert!(cx.update(|_, cx| a.read(cx).is_hub(cx) && !b.read(cx).is_hub(cx)));
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let app: &mut TestAppContext = cx;
    assert!(app.update(|cx| b.read(cx).is_hub(cx)), "B took the queues");
    assert!(app.update(|cx| window_handles(cx) == vec![b_window]));
    drop(orch_tx);
}
