//! Restart agent session: an idle agent ends and the next send resumes its session in a new process; a working one
//! is asked about first. And the details card's MCP list follows what the latest process reported.

use super::resume::env_reports;
use super::*;
use crate::workspace::thread::RestartAgentSession;
use crate::workspace::thread_details::fold_mcp;
use gpui_component::WindowExt as _;
use skyelab_agent::McpServer;

fn server(name: &str, status: &str) -> McpServer {
    McpServer {
        name: name.into(),
        status: status.into(),
        error: None,
    }
}

fn report(servers: Vec<McpServer>, launch: bool) -> Record {
    Record::Agent(skyelab_agent::Event::Mcp {
        servers,
        launch,
        warning: None,
    })
}

#[gpui::test]
fn restart_idle_resumes_busy_asks(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    std::fs::create_dir_all(&f.account_dir).expect("account dir");
    std::fs::write(
        f.account_dir.join("fake-mcp-status.json"),
        r#"[{"name":"docs","status":"failed"},{"name":"linear","status":"connected"}]"#,
    )
    .expect("status file");
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    // What the agent's init reported is on the card.
    thread.read_with(cx, |t, _| {
        assert_eq!(
            t.details.mcp,
            [server("docs", "failed"), server("linear", "connected")]
        );
    });
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);

    // Idle: no question, the process ends.
    thread.update_in(cx, |t, window, cx| {
        t.restart_session(&RestartAgentSession, window, cx)
    });
    wait_until(cx, "the agent to end", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));

    // The next send resumes the same session in a new process, one that never ends its turn.
    std::fs::write(f.project_dir.join(".fake-agent-no-result"), "").expect("marker");
    type_and_send(&thread, cx);
    wait_until(cx, "the resumed agent's env report", |_| {
        env_reports(&f.store, id).len() == 2
    });
    let args = env_reports(&f.store, id)[1]["args"]
        .as_str()
        .expect("args")
        .to_owned();
    assert!(args.contains("--resume fake-sess-1"), "{args}");
    wait_until(cx, "the turn to run", |cx| {
        thread.read_with(cx, |t, _| t.turn_running())
    });

    // Working: it asks, and Cancel leaves the agent running.
    thread.update_in(cx, |t, window, cx| {
        t.restart_session(&RestartAgentSession, window, cx)
    });
    cx.run_until_parked();
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.update(|window, cx| window.close_dialog(cx));
    cx.run_until_parked();
    thread.read_with(cx, |t, _| {
        assert!(t.session.is_some(), "Cancel keeps the agent")
    });
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
}

#[test]
fn mcp_servers_show_latest_process() {
    let mut list = Vec::new();
    assert!(fold_mcp(
        &mut list,
        &report(
            vec![server("skyelab", "starting"), server("docs", "starting")],
            true
        )
    ));
    // A report updates each server it names and adds new ones (Codex reports one at a time).
    fold_mcp(&mut list, &report(vec![server("docs", "failed")], false));
    fold_mcp(&mut list, &report(vec![server("linear", "ready")], false));
    assert_eq!(
        list,
        [
            server("skyelab", "starting"),
            server("docs", "failed"),
            server("linear", "ready")
        ]
    );
    // A new process starts the list over: servers it wasn't given are gone.
    fold_mcp(
        &mut list,
        &report(vec![server("skyelab", "starting")], true),
    );
    assert_eq!(list, [server("skyelab", "starting")]);
    assert!(!fold_mcp(&mut list, &Record::Prompt("hi".into())));
}
