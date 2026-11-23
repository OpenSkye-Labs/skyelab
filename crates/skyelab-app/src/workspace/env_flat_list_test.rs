//! Another machine's view at parity with this one: its sidebar is the same flat list (Pinned, Working, then by day,
//! with each row's project, reply preview, status and account), mod+F finds in a remote thread, and an image the
//! other machine's store dropped reads "not kept".

use super::env_harness::*;
use super::remote_thread::RemoteThreadView;
use super::thread_find::FindHost as _;
use super::thread_model::{Item, Record};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_store::NewThread;

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

/// Opens the remote's one thread "fix the login bug" in a tab.
fn open_thread(
    ws: &Entity<Workspace>,
    remote: &RemoteSide,
    cx: &mut VisualTestContext,
) -> Entity<RemoteThreadView> {
    connect(ws, remote, cx);
    wait_until(cx, "remote threads", |cx| {
        sidebar(ws, cx)[1].1 == vec!["fix the login bug".to_owned()]
    });
    click_row(ws, "fix the login bug", cx);
    wait_until(cx, "the thread tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });
    ws.read_with(cx, |ws, _| ws.remote_tabs()[0].clone())
}

#[gpui::test]
fn remote_sidebar_lists_every_projects_threads_flat(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    {
        let mut store = remote.store.lock().expect("store");
        let web = store
            .create_project("web", std::path::Path::new("/remote/web"))
            .expect("project");
        let thread = |store: &skyelab_store::Store, title: &str, status: &str| {
            store
                .create_thread(&NewThread {
                    project_id: web.id,
                    title,
                    cwd: &web.path,
                    account_id: "work",
                    status,
                })
                .expect("thread")
                .id
        };
        let pinned = thread(&store, "ship the release", "exited");
        thread(&store, "migrate the db", "running");
        store.set_thread_pinned(pinned, true).expect("pin");
        let reply = Record::Agent(skyelab_agent::Event::Assistant {
            content: vec![skyelab_agent::Block::Text {
                text: "## Shipped\n\nTagged **v2.1** and pushed.".into(),
            }],
        });
        let (kind, payload) = reply.to_row();
        store.append_event(pinned, kind, &payload).expect("reply");
    }
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    wait_until(cx, "every project's threads", |cx| {
        raw_sidebar(&ws, cx)
            == [
                ("Pinned".to_owned(), vec!["ship the release".to_owned()]),
                ("Working".to_owned(), vec!["migrate the db".to_owned()]),
                ("Today".to_owned(), vec!["fix the login bug".to_owned()]),
                (
                    "Projects".to_owned(),
                    vec!["app".to_owned(), "web".to_owned()],
                ),
            ]
    });
    ws.read_with(cx, |ws, cx| {
        let sidebar = ws.sidebar.read(cx);
        let row = |title: &str| {
            sidebar
                .sections
                .iter()
                .flat_map(|s| &s.items)
                .find(|i| i.label == title)
                .expect("row")
        };
        let pinned = row("ship the release");
        assert_eq!(pinned.project.as_deref(), Some("web"), "its project");
        assert!(
            matches!(&pinned.account, Some(panels::RowAccount::Bound(a)) if a.id == "work" && a.label == "work"),
            "the account (no label from a machine without accounts.toml)"
        );
        let marks = sidebar.marks.get(&pinned.id).expect("marks");
        assert!(marks.pinned);
        assert_eq!(
            marks.preview.as_deref(),
            Some("Shipped Tagged v2.1 and pushed."),
            "the reply as one plain line"
        );
        assert_eq!(
            row("migrate the db").status,
            Some(crate::ui::Status::Running)
        );
        assert_eq!(row("app").project, None, "project rows stay plain");
    });
}

#[gpui::test]
fn find_in_a_remote_thread_counts_matches(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    remote.store_texts(
        remote.thread_id,
        &["a needle here", "nothing", "needle and needle"],
        false,
    );
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let view = open_thread(&ws, &remote, cx);
    wait_until(cx, "stored events", |cx| {
        view.read_with(cx, |v, _| v.items.len()) >= 3
    });
    view.update_in(cx, |v, window, cx| v.focus_prompt(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(cx.debug_bounds("find-bar").is_some(), "mod+F opens the bar");
    cx.simulate_keystrokes("n e e d l e");
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        assert_eq!(v.find().count("needle"), "1/3");
        assert_eq!(v.find().matches.len(), 3);
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    view.read_with(cx, |v, _| assert_eq!(v.find().count("needle"), "2/3"));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("find-bar").is_none(), "Esc closes it");
}

#[gpui::test]
fn a_dropped_image_reads_not_kept_remotely(cx: &mut TestAppContext) {
    use base64::Engine as _;
    let remote = remote_side(FakeTailnet::default());
    let png = base64::engine::general_purpose::STANDARD.encode(vec![7u8; 210_039]);
    let call = Record::Agent(skyelab_agent::Event::Assistant {
        content: vec![skyelab_agent::Block::ToolUse {
            id: "toolu_1".into(),
            name: "mcp__skyelab__browser_screenshot".into(),
            input: serde_json::json!({}),
        }],
    });
    let result = Record::Agent(skyelab_agent::Event::User {
        content: vec![skyelab_agent::Block::ToolResult {
            tool_use_id: "toolu_1".into(),
            content: serde_json::json!([
                {"type": "text", "text": "Screenshot of the tab"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}}
            ]),
            is_error: false,
        }],
    });
    for record in [call, result] {
        let (kind, payload) = record.to_row();
        remote.append(remote.thread_id, kind, &payload);
    }
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let view = open_thread(&ws, &remote, cx);
    let tool_result = |cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| {
            v.items.iter().find_map(|i| match i {
                Item::Tool(t) => t.result.clone(),
                _ => None,
            })
        })
    };
    wait_until(cx, "the tool row", |cx| tool_result(cx).is_some());
    assert_eq!(
        tool_result(cx).as_deref(),
        Some("Screenshot of the tab\nImage (205 KB) not kept")
    );
}
