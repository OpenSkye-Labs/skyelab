//! Acceptance: "Fork from here" makes a new thread in the same project and account with the history up to that reply,
//! marked as a fork; its first send starts a new session with the copied history as context, stored as typed. An
//! unbound folder forks nothing.

use crate::workspace::thread_extras::fork::ForkThread;
use crate::workspace::thread_items::ItemsView as _;
use crate::workspace::thread_model::{Item, Record};
use crate::workspace::threads_test::{
    fixture, open_workspace, project_thread, setup, stored_records, type_and_send, wait_done,
    wait_until,
};
use gpui::TestAppContext;

#[gpui::test]
fn fork_copies_history_and_starts_with_context(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let src = project_thread(&f, &ws, cx);
    type_and_send(&src, cx);
    wait_done(&src, cx);
    let (src_id, action) = src.read_with(cx, |t, _| {
        let ix = t
            .items
            .iter()
            .position(|i| matches!(i, Item::Text(_)))
            .expect("a reply");
        let ix = (ix..t.items.len())
            .rev()
            .find(|&i| t.fork(i).is_some())
            .expect("a forkable reply");
        (
            t.thread.as_ref().expect("row").id,
            t.fork(ix).expect("fork"),
        )
    });
    assert_eq!(
        action,
        ForkThread {
            id: src_id,
            turns: 1
        }
    );
    cx.dispatch_action(action.clone());
    wait_until(cx, "the fork opened", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.threads.iter().any(|t| t.forked_from == Some(src_id))
        })
    });
    let fork_row = ws
        .read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .find(|t| t.forked_from == Some(src_id))
                .cloned()
        })
        .expect("row");
    assert_eq!(fork_row.account_id, "acme");
    assert_eq!(fork_row.project_id, src.read_with(cx, |t, _| t.project_id));
    let copied = stored_records(&f.store, fork_row.id);
    assert!(matches!(copied.first(), Some(Record::Prompt(p)) if p == "hi\nthere"));
    assert!(
        matches!(
            copied.last(),
            Some(Record::Agent(skyelab_agent::Event::Result { .. }))
        ),
        "up to the reply's turn"
    );

    let view = ws
        .read_with(cx, |ws, cx| ws.open_view(fork_row.id, cx))
        .expect("fork tab");
    wait_until(cx, "the fork's header", |cx| {
        view.read_with(cx, |t, _| {
            t.composer.extras.forked_from.as_ref().map(|(id, _)| *id) == Some(src_id)
        })
    });
    view.read_with(cx, |t, _| assert!(!t.read_only, "a fork can be sent to"));

    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("next step");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "the fork's session", |_| {
        f.store
            .lock()
            .expect("lock")
            .thread(fork_row.id)
            .expect("read")
            .expect("row")
            .agent_session_id
            .is_some()
    });
    wait_until(cx, "the context cleared", |_| {
        f.store
            .lock()
            .expect("lock")
            .fork_context(fork_row.id)
            .expect("read")
            .is_none()
    });
    let records = stored_records(&f.store, fork_row.id);
    let prompts: Vec<&String> = records
        .iter()
        .filter_map(|r| match r {
            Record::Prompt(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(
        prompts.last().map(|p| p.as_str()),
        Some("next step"),
        "stored as typed"
    );
    assert!(!prompts.iter().any(|p| p.contains("<conversation>")));
    // The fake echoes what it was sent: the context block went to the agent.
    wait_until(cx, "the agent got the context", |_| {
        let s = f.store.lock().expect("lock");
        s.events_since(fork_row.id, 0)
            .expect("events")
            .iter()
            .any(|e| e.kind == "agent" && e.payload_json.contains("<conversation>"))
    });

    // Unbound: refused, no new row.
    let mut config = skyelab_core::AccountsConfig::load(&f.accounts_path).expect("load");
    config.bindings.clear();
    config.save(&f.accounts_path).expect("save");
    let file = f.accounts_path.clone();
    cx.update(|_, cx| cx.set_global(crate::workspace::AccountsFile(file)));
    let before = f
        .store
        .lock()
        .expect("lock")
        .list_threads(fork_row.project_id)
        .expect("list")
        .len();
    cx.dispatch_action(action);
    cx.run_until_parked();
    std::thread::sleep(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    let after = f
        .store
        .lock()
        .expect("lock")
        .list_threads(fork_row.project_id)
        .expect("list")
        .len();
    assert_eq!(before, after, "nothing forked under no binding");
}
