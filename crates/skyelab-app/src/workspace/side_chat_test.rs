//! Acceptance: `/side <question>` opens a side chat in the right dock that runs in the parent's folder under the
//! parent's account, as its own agent session (the parent's session, timeline and events are untouched), hidden from
//! the sidebar and listed in the parent's details; "Bring back" puts its answer in the parent's composer as a chip;
//! a folder rebound to another account (or unbound) refuses to make one.

use super::*;
#[cfg(unix)]
use crate::workspace::thread::composer::Submit;
#[cfg(unix)]
use crate::workspace::thread::composer::composer_context::ChipKind;
#[cfg(unix)]
use crate::workspace::thread_model::Record;
use crate::workspace::worktree_thread_test::fixture;
#[cfg(unix)]
use crate::workspace::worktree_thread_test::{open_thread, send, wait_until};
use skyelab_core::Binding;

#[cfg(unix)]
fn stored(f: &crate::workspace::worktree_thread_test::Fixture, id: i64) -> Thread {
    let s = f.store.lock().expect("store");
    s.thread(id).expect("read").expect("row")
}

#[cfg(unix)]
fn event_count(f: &crate::workspace::worktree_thread_test::Fixture, id: i64) -> usize {
    let s = f.store.lock().expect("store");
    s.events_since(id, 0).expect("events").len()
}

/// The fake agent's `fake_env` report (its config dir, folder and pid), once stored.
#[cfg(unix)]
fn fake_env(
    f: &crate::workspace::worktree_thread_test::Fixture,
    id: i64,
) -> Option<serde_json::Value> {
    let s = f.store.lock().expect("store");
    s.events_since(id, 0).expect("events").iter().find_map(|e| {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(skyelab_agent::Event::Unknown(v))) if v["type"] == "fake_env" => {
                Some(v)
            }
            _ => None,
        }
    })
}

#[test]
fn side_question_reads_side_and_btw_only() {
    assert_eq!(side_question("/side why?"), Some(Some("why?".into())));
    assert_eq!(
        side_question("/btw  is it slow? "),
        Some(Some("is it slow?".into()))
    );
    assert_eq!(side_question("/side"), Some(None));
    assert_eq!(side_question("/sidebar x"), None);
    assert_eq!(side_question("fix /side x"), None);
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn a_side_chat_runs_beside_its_thread_and_brings_its_answer_back(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx); // "fix it"
    let parent = thread
        .read_with(cx, |t, _| t.thread.clone())
        .expect("parent row");
    wait_until(cx, "the parent's turn", |_| {
        fake_env(&f, parent.id).is_some()
    });
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| !t.turn_running())
    });
    let session = thread
        .read_with(cx, |t, _| t.session.clone())
        .expect("session");
    let (events, items) = (
        event_count(&f, parent.id),
        thread.read_with(cx, |t, _| t.items.len()),
    );

    thread.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("/side why is it slow?", window, cx));
        t.submit(Submit::Send, window, cx);
    });
    assert_eq!(
        thread.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string()),
        "",
        "the command left the composer"
    );
    wait_until(cx, "the side chat pane", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let panel = ws.read_with(cx, |ws, _| ws.side_chats[0].clone());
    let side_view = panel.read_with(cx, |p, _| p.view.clone());
    let side = side_view
        .read_with(cx, |v, _| v.thread.clone())
        .expect("side row");

    // Same folder, same account, its own row marked as the parent's side chat.
    assert_eq!(side.side_of, Some(parent.id));
    assert_eq!(
        (side.project_id, &side.cwd, &side.account_id),
        (parent.project_id, &parent.cwd, &parent.account_id)
    );
    assert_eq!(side.title, "Side: why is it slow?");
    assert!(ws.read_with(cx, |ws, _| ws.threads.iter().all(|t| t.id != side.id)));
    assert_eq!(
        thread.read_with(cx, |t, _| t.details.side_chats.clone()),
        [(side.id, "Side: why is it slow?".into())]
    );

    // The question went to its own agent, under the parent's account dir, in the parent's folder.
    wait_until(cx, "the side chat's agent", |_| {
        fake_env(&f, side.id).is_some()
    });
    let env = fake_env(&f, side.id).expect("env");
    assert_eq!(env["config_dir"], f.account_dir.to_str().expect("utf-8"));
    let cwd = std::fs::canonicalize(&parent.cwd).expect("cwd");
    assert_eq!(env["cwd"], cwd.to_str().expect("utf-8"));
    let parent_pid = fake_env(&f, parent.id).expect("env")["pid"].clone();
    assert_ne!(env["pid"], parent_pid, "a separate agent process");
    wait_until(cx, "the side chat's answer", |cx| {
        side_view.read_with(cx, |v, _| v.last_answer().is_some() && !v.turn_running())
    });
    assert!(
        stored(&f, side.id).agent_session_id.is_some(),
        "its own session, which consumed the context prompt"
    );
    // The fake echoes what it was sent: the parent's summary went ahead of the question.
    let sent = f
        .store
        .lock()
        .expect("store")
        .events_since(side.id, 0)
        .expect("events");
    assert!(
        sent.iter()
            .any(|e| e.payload_json.contains("side chat next to the thread")
                && e.payload_json.contains("why is it slow?")),
        "{sent:#?}"
    );

    // The main thread's session, timeline and stored events are as they were.
    thread.read_with(cx, |t, _| {
        let now = t.session.clone().expect("still running");
        assert!(std::sync::Arc::ptr_eq(&now, &session), "same session");
        assert_eq!(t.items.len(), items, "nothing new in its timeline");
    });
    assert_eq!(event_count(&f, parent.id), events, "no events stored on it");

    // Bring back: the answer becomes a chip in the parent's composer.
    let answer = side_view.read_with(cx, |v, _| v.last_answer().map(str::to_owned));
    panel.update(cx, |p, cx| p.bring_back(cx));
    thread.read_with(cx, |t, _| {
        let [chip] = &t.composer.contexts[..] else {
            panic!("one chip: {:?}", t.composer.contexts)
        };
        assert_eq!(chip.kind, ChipKind::SideChat);
        assert_eq!(chip.label.as_ref(), "Side: why is it slow?");
        assert_eq!(Some(chip.text.trim()), answer.as_deref().map(str::trim));
    });

    // Reopened from the parent: the kept pane, not a second one.
    thread.update(cx, |_, cx| {
        cx.emit(ThreadViewEvent::Side(SideAsk::Open(side.id)))
    });
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.side_chats.len()), 1);
}

#[test]
fn a_rebound_or_unbound_folder_makes_no_side_chat() {
    let f = fixture();
    let mut s = f.store.lock().expect("store");
    let p = s.create_project("demo", &f.project).expect("project");
    let parent = s
        .create_thread(&skyelab_store::NewThread {
            project_id: p.id,
            title: "fix it",
            cwd: &f.project,
            account_id: "acme",
            status: "idle",
        })
        .expect("parent");
    s.append_event(parent.id, "prompt", "\"fix the login\"")
        .expect("prompt");

    let mut rebound = f.config.clone();
    rebound.bindings.clear();
    rebound
        .add_binding(Binding::new(f.project.clone(), "other".into()))
        .expect("bind other");
    let mut unbound = f.config.clone();
    unbound.bindings.clear();
    for config in [&rebound, &unbound] {
        let e = create(&mut s, config, &parent, "Side").expect_err("refused");
        assert!(e.to_string().contains("isn't bound to acme"), "{e}");
    }
    assert!(
        s.side_chats(parent.id).expect("list").is_empty(),
        "nothing made"
    );

    // Still bound: made, with the parent's conversation as its first session's context.
    let side = create(&mut s, &f.config, &parent, "Side").expect("made");
    let context = s.fork_context(side.id).expect("ctx").expect("set");
    assert!(
        context.contains("side chat next to the thread \"fix it\""),
        "{context}"
    );
    assert!(
        context.contains(r#"{"role":"user","text":"fix the login"}"#),
        "{context}"
    );
}
