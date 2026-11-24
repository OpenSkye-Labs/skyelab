use super::{IDLE_RELEASE, holds_work};
use crate::workspace::thread_model::Item;
use skyelab_agent::timeline::ToolItem;
use skyelab_agent::timeline::subagent::{SubagentItem, SubagentStatus};

fn tool(raw: serde_json::Value) -> Item {
    Item::Tool(ToolItem {
        id: "t".into(),
        name: "Bash".into(),
        input: String::new(),
        raw,
        result: Some("ok".into()),
        is_error: false,
    })
}

#[test]
fn background_work_pins_the_agent_and_plain_turns_do_not() {
    let plain = [
        Item::Prompt("hi".into()),
        tool(serde_json::json!({"command": "ls"})),
        Item::Text("done".into()),
    ];
    assert!(!holds_work(&plain));
    assert!(holds_work(&[tool(
        serde_json::json!({"command": "npm run dev", "run_in_background": true})
    )]));
    let mut sub = SubagentItem::new("a", "Agent", &serde_json::json!({"description": "look"}));
    sub.status = SubagentStatus::Running;
    assert!(holds_work(&[Item::Subagent(sub.clone())]));
    sub.status = SubagentStatus::Done;
    assert!(!holds_work(&[Item::Subagent(sub)]));
}

#[cfg(unix)]
mod live {
    use super::IDLE_RELEASE;
    use crate::test_wait::wait_ticking;
    use crate::workspace::threads_test::resume::env_reports;
    use crate::workspace::threads_test::{
        fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
    };
    use gpui::TestAppContext;
    use std::time::Duration;

    fn alive(pid: i64) -> bool {
        std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .status()
            .expect("kill runs")
            .success()
    }

    #[gpui::test]
    fn an_idle_agent_is_ended_and_the_next_send_resumes_it(cx: &mut TestAppContext) {
        setup(cx);
        let f = fixture(true);
        let (ws, cx) = open_workspace(&f, cx);
        let thread = project_thread(&f, &ws, cx);
        type_and_send(&thread, cx);
        wait_done(&thread, cx);
        let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
        wait_until(cx, "the fake's env report", |_| {
            !env_reports(&f.store, id).is_empty()
        });
        let pid = env_reports(&f.store, id)[0]["pid"]
            .as_i64()
            .expect("the fake reports its pid");

        cx.executor()
            .advance_clock(IDLE_RELEASE - Duration::from_secs(5));
        cx.run_until_parked();
        assert!(alive(pid), "kept while idle for less than the release time");

        wait_ticking(cx, "the idle agent ended, the thread still idle", |cx| {
            let status = f
                .store
                .lock()
                .expect("lock")
                .thread(id)
                .expect("read")
                .expect("row")
                .status;
            !alive(pid) && status == "idle" && thread.read_with(cx, |t, _| t.session.is_none())
        });
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None);
            assert!(!t.read_only, "the next send resumes it");
        });

        type_and_send(&thread, cx);
        wait_until(cx, "a second agent, resumed", |_| {
            env_reports(&f.store, id).len() == 2
        });
        let args = env_reports(&f.store, id)[1]["args"]
            .as_str()
            .expect("args")
            .to_owned();
        assert!(args.contains("--resume"), "{args}");
    }

    #[gpui::test]
    fn a_delegated_thread_keeps_its_agent(cx: &mut TestAppContext) {
        setup(cx);
        let f = fixture(true);
        let (ws, cx) = open_workspace(&f, cx);
        let thread = project_thread(&f, &ws, cx);
        type_and_send(&thread, cx);
        wait_done(&thread, cx);
        let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
        wait_until(cx, "the fake's env report", |_| {
            !env_reports(&f.store, id).is_empty()
        });
        let pid = env_reports(&f.store, id)[0]["pid"].as_i64().expect("pid");
        // Its parent sends follow-ups to the running agent, so it must not be released.
        thread.update(cx, |t, _| {
            t.thread.as_mut().expect("row").parent_thread_id = Some(id + 1000);
        });
        cx.executor()
            .advance_clock(IDLE_RELEASE + Duration::from_secs(60));
        cx.run_until_parked();
        assert!(alive(pid), "a delegated thread's agent stays");
        assert!(thread.read_with(cx, |t, _| t.session.is_some()));
    }
}
