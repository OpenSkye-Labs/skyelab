//! Sign in on a needs-auth MCP row: a terminal runs the agent CLI's per-server login in the thread's folder, under
//! the folder's bound account only, then the workspace offers to restart the agent so the server reconnects.

use super::github::mcp_login_args;
use super::thread::ThreadViewEvent;
use super::worktree_thread_test::{fixture, open_thread, open_thread_with, send, wait_until};
use crate::test_wait::wait_ticking;
use gpui::{Entity, VisualTestContext};
use gpui_component::WindowExt as _;

#[test]
fn mcp_login_args_name_the_server_after_dashdash() {
    // `--` keeps a server named like a flag from being read as one (both CLIs take it: `mcp get -- <name>`).
    assert_eq!(
        mcp_login_args("-x linear"),
        ["mcp", "login", "--", "-x linear"]
    );
}

/// A stored thread (sign-in needs its account), then Sign in on `server`.
fn stored_then_sign_in(
    thread: &Entity<super::ThreadView>,
    server: &str,
    cx: &mut VisualTestContext,
) {
    send(cx);
    wait_until(cx, "thread stored", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some())
    });
    let server = server.to_owned();
    thread.update(cx, |_, cx| cx.emit(ThreadViewEvent::McpSignIn(server)));
    cx.run_until_parked();
}

fn tab_names(ws: &Entity<super::Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    ws.read_with(cx, |ws, cx| {
        ws.terminals
            .iter()
            .filter_map(|p| p.read(cx).name.as_ref().map(|n| n.to_string()))
            .collect()
    })
}

#[gpui::test]
fn sign_in_opens_a_terminal_under_the_folder_account(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    stored_then_sign_in(&thread, "linear", cx);
    let record = f.account_dir.join("fake-mcp-login");
    wait_until(cx, "the fake ran mcp login", |_| record.exists());
    let text = std::fs::read_to_string(&record).expect("record");
    let project = std::fs::canonicalize(&f.project).expect("canonical project");
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        ["linear", project.to_str().expect("utf8")],
        "the login names the server and runs in the thread's folder, under acme's config dir"
    );
    assert!(tab_names(&ws, cx).contains(&"Sign in: linear".to_owned()));
}

#[gpui::test]
fn sign_in_refuses_a_folder_bound_elsewhere(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "thread stored", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some())
    });
    // The thread claims an account the folder isn't bound to: never sign in under either.
    thread.update(cx, |t, cx| {
        if let Some(row) = t.thread.as_mut() {
            row.account_id = "other".into();
        }
        cx.emit(ThreadViewEvent::McpSignIn("linear".into()));
    });
    cx.run_until_parked();
    wait_until(cx, "an error on the thread", |cx| {
        thread.read_with(cx, |t, _| t.error.is_some())
    });
    let before = tab_names(&ws, cx);
    assert!(!before.iter().any(|n| n.starts_with("Sign in")), "{before:?}");
    assert!(!f.account_dir.join("fake-mcp-login").exists());
    assert!(!f.project.parent().expect("parent").join(".claude-other/fake-mcp-login").exists());
}

#[gpui::test]
fn sign_in_offers_restart_after_the_terminal_exits(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    // In a Root, so the notice is really shown and its Restart can be clicked.
    let (ws, thread, cx) = open_thread_with(&f, true, cx);
    stored_then_sign_in(&thread, "linear", cx);
    // The exit watch polls on the executor's clock.
    wait_ticking(cx, "the restart offer", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.notices
                .iter()
                .any(|n| n.contains("linear") && n.contains("Restart"))
        })
    });
    let notes = |cx: &mut VisualTestContext| cx.update(|window, cx| window.notifications(cx));
    // The notice slides in: click once it has stopped moving (a redraw each poll, so the bounds are this frame's).
    let mut last = None;
    wait_ticking(cx, "the notice in place", |cx| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let now = cx.debug_bounds("mcp-restart");
        now.is_some() && std::mem::replace(&mut last, now) == now
    });
    let offer = notes(cx).first().cloned().expect("the restart offer");
    assert!(thread.read_with(cx, |t, _| t.session.is_some()));
    let at = last.expect("drawn").center();
    cx.simulate_click(at, gpui::Modifiers::none());
    wait_until(cx, "the agent to end", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    // The notice closes with a short animation on the executor's clock.
    wait_ticking(cx, "the offer gone", |cx| !notes(cx).contains(&offer));
}
