//! Acceptance: a side chat is a read-only helper by construction. Its agent starts in Plan mode (Claude's
//! `--permission-mode plan`; Codex's read-only sandbox) with no SkyeLab MCP config, even when orchestration is on and
//! even for a side chat stored before this held; its mode can't be raised from the composer, nor by approving a plan;
//! and a file link in its replies opens the Preview tab.

use crate::workspace::thread::composer::Submit;
use crate::workspace::thread_items::ItemsView as _;
use crate::workspace::thread_model::Record;
use crate::workspace::worktree_thread_test::{Fixture, fixture, open_thread, send, wait_until};
use skyelab_agent::Mode;

/// The fake agent's command line, once it has reported it.
fn args(f: &Fixture, id: i64) -> Option<String> {
    let s = f.store.lock().expect("store");
    s.events_since(id, 0).expect("events").iter().find_map(|e| {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(skyelab_agent::Event::Unknown(v))) if v["type"] == "fake_env" => {
                v["args"].as_str().map(str::to_owned)
            }
            _ => None,
        }
    })
}

#[gpui::test]
fn a_side_chat_starts_in_plan_without_tools(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let run_dir = f.project.parent().expect("parent").join("run");
    let (ws, thread, cx) = open_thread(&f, cx);
    // Orchestration on, as in the app: the main thread's agent gets SkyeLab's tools.
    let proxy = skyelab_agent::mcp::Proxy {
        program: std::env::current_exe().expect("test binary"),
        args: Vec::new(),
    };
    let (store, config) = (f.store.clone(), f.config.clone());
    let _calls = cx.update(|_, cx| {
        crate::workspace::orchestrate::install(cx, &run_dir, store, config, proxy)
            .expect("orchestration")
    });
    send(cx);
    let parent = thread.read_with(cx, |t, _| t.thread.clone()).expect("row");
    wait_until(cx, "the parent's agent", |_| args(&f, parent.id).is_some());
    let parent_args = args(&f, parent.id).expect("args");
    assert!(parent_args.contains("--mcp-config"), "{parent_args}");
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| !t.turn_running())
    });

    thread.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("/side", window, cx));
        t.submit(Submit::Send, window, cx);
    });
    wait_until(cx, "the side chat pane", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let side_view = ws.read_with(cx, |ws, cx| ws.side_chats[0].read(cx).view.clone());
    wait_until(cx, "the side chat loads", |cx| {
        side_view.read_with(cx, |v, _| !v.read_only)
    });
    let side = side_view
        .read_with(cx, |v, _| v.thread.clone())
        .expect("row");
    assert_eq!(side.permission_mode, "plan", "stored in Plan");
    assert_eq!(side_view.read_with(cx, |v, _| v.mode), Mode::Plan);

    // Raising it is refused: from the chip, through Full access's confirm, and by approving a plan.
    side_view.update(cx, |v, cx| {
        v.set_mode(Mode::AcceptEdits, cx);
        v.set_mode(Mode::Bypass, cx);
        v.confirm_mode(true, cx);
        assert_eq!(v.mode, Mode::Plan);
        let refused = v
            .answer_plan_then("plan-1", true, |_| {}, cx)
            .expect_err("refused");
        assert!(refused.contains("side chat"), "{refused}");
    });

    // A side chat stored before this change loads in Ask: its agent still starts in Plan.
    side_view.update(cx, |v, _| v.mode = Mode::Default);
    side_view
        .update_in(cx, |v, window, cx| {
            v.submit_remote("where is it slow?", window, cx)
        })
        .expect("sent");
    wait_until(cx, "the side chat's agent", |_| args(&f, side.id).is_some());
    let side_args = args(&f, side.id).expect("args");
    assert!(side_args.contains("--permission-mode plan"), "{side_args}");
    assert!(
        !side_args.contains("--mcp-config"),
        "no SkyeLab tools: {side_args}"
    );
    // Started in Plan while the view held Ask: the agent is not then switched to the view's mode.
    wait_until(cx, "the side chat's answer", |cx| {
        side_view.read_with(cx, |v, _| v.last_answer().is_some() && !v.turn_running())
    });
    let sent = f
        .store
        .lock()
        .expect("store")
        .events_since(side.id, 0)
        .expect("events");
    assert!(
        !sent
            .iter()
            .any(|e| e.payload_json.contains("set_permission_mode")
                && !e.payload_json.contains(r#""mode":"plan""#)),
        "{sent:#?}"
    );
    assert_eq!(side_view.read_with(cx, |v, _| v.mode), Mode::Plan);

    // A file link in its reply opens the Preview tab at that file.
    side_view.update(cx, |v, cx| v.preview_path("README.md", None, cx));
    cx.run_until_parked();
    let tab = ws
        .read_with(cx, |ws, _| ws.preview_tab())
        .expect("the Preview tab");
    assert_eq!(
        tab.read_with(cx, |p, _| p.path.clone()),
        std::path::PathBuf::from("README.md")
    );
}
