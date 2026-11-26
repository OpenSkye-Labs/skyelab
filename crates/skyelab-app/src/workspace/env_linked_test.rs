//! Linked machines: another machine's threads in This Mac's list, each marked with its machine, opened in place.

use super::center::Page;
use super::env_harness::*;
use super::env_linked::ToggleLinkedEnvironment;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_daemon::testing::NODE_NAME;

const ROW: &str = "fix the login bug";

fn link(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    let action = ToggleLinkedEnvironment {
        host: NODE_NAME.into(),
    };
    ws.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(action), cx)
    });
    cx.run_until_parked();
}

fn rows(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    raw_sidebar(ws, cx)
        .into_iter()
        .flat_map(|(_, r)| r)
        .collect()
}

/// The marks of the first row labelled `label`.
fn marks_of(
    ws: &Entity<Workspace>,
    label: &str,
    cx: &mut VisualTestContext,
) -> Option<(Option<String>, bool)> {
    ws.read_with(cx, |ws, cx| {
        let sidebar = ws.sidebar.read(cx);
        let item = sidebar
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.label == label)?;
        let m = sidebar.marks.get(&item.id)?;
        Some((m.machine.as_ref().map(|s| s.to_string()), m.unread))
    })
}

/// Linked and listed: the remote's thread shows in This Mac's list.
fn linked_and_listed(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    link(ws, cx);
    wait_until(cx, "the linked machine's thread", |cx| {
        rows(ws, cx).contains(&ROW.to_owned())
    });
}

/// Waits until the linked machine's stream brought its thread's stored status here: no listing is asked for.
fn relist(ws: &Entity<Workspace>, remote: &RemoteSide, cx: &mut VisualTestContext) {
    let status = remote
        .store
        .lock()
        .expect("store")
        .thread(remote.thread_id)
        .expect("read")
        .expect("thread")
        .status;
    wait_until(cx, "the change streamed here", |cx| {
        ws.read_with(cx, |ws, cx| {
            ws.linked_envs().iter().all(|env| {
                env.read(cx)
                    .threads
                    .iter()
                    .any(|t| t.id == remote.thread_id && t.status == status)
            })
        })
    });
    cx.run_until_parked();
}

/// A thread started, renamed, settled and finished on the linked machine shows here with no listing at all.
#[gpui::test]
fn linked_rows_follow_the_machines_stream(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    let lists = |remote: &RemoteSide| remote.asked().iter().filter(|a| **a == "list_threads").count();
    let listed = lists(&remote);
    let started = {
        let store = remote.store.lock().expect("store");
        let id = store
            .create_thread(&skyelab_store::NewThread {
                project_id: remote.project_id,
                title: "started over there",
                cwd: Path::new("/remote/app"),
                account_id: "work",
                status: "running",
            })
            .expect("thread")
            .id;
        store.set_thread_title(id, "renamed over there").expect("rename");
        id
    };
    wait_until(cx, "the started, renamed thread", |cx| {
        rows(&ws, cx).contains(&"renamed over there".to_owned())
    });
    remote.store.lock().expect("store").set_thread_status(started, "idle").expect("finish");
    wait_until(cx, "finished, unseen: Done", |cx| {
        marks_of(&ws, "renamed over there", cx).map(|m| m.1) == Some(true)
    });
    remote.store.lock().expect("store").set_thread_settled(started, true).expect("settle");
    wait_until(cx, "settled there, off the list here", |cx| {
        !rows(&ws, cx).contains(&"renamed over there".to_owned())
    });
    assert_eq!(lists(&remote), listed, "nothing was listed again");
}

#[gpui::test]
fn linked_machine_threads_show_in_local_list(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    assert!(
        ws.read_with(cx, |ws, _| !ws.is_remote()),
        "still on This Mac"
    );
    let (machine, _) = marks_of(&ws, ROW, cx).expect("marks");
    assert_eq!(machine.as_deref(), Some("desktop"), "the machine's chip");
}

#[gpui::test]
fn linked_row_opens_in_place_and_sends(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    click_row(&ws, ROW, cx);
    wait_until(cx, "the thread page", |cx| {
        ws.read_with(cx, |ws, _| matches!(ws.page, Page::RemoteThread(_)))
    });
    assert!(ws.read_with(cx, |ws, _| !ws.is_remote()), "no switch");
    let view = ws.read_with(cx, |ws, _| ws.remote_tabs()[0].clone());
    assert_eq!(
        view.read_with(cx, |v, _| v.thread_id()),
        Some(remote.thread_id)
    );
    // Opening it again shows the same view.
    click_row(&ws, ROW, cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.remote_tabs().len()), 1);
    view.update_in(cx, |v, window, cx| v.submit("try again", window, cx));
    wait_until(cx, "sent there", |_| {
        remote
            .ran()
            .contains(&Ran::Send(remote.thread_id, "try again".into()))
    });
}

#[gpui::test]
fn linked_machine_waiting_shows_state_row(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    remote
        .daemon
        .as_ref()
        .expect("running")
        .set_approved(vec![]);
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    link(&ws, cx);
    wait_until(cx, "the state row", |cx| {
        raw_sidebar(&ws, cx).iter().any(|(title, rows)| {
            title == "Linked machines"
                && rows
                    .iter()
                    .any(|r| r.starts_with("desktop · Waiting for approval"))
        })
    });
    let label = rows(&ws, cx)
        .into_iter()
        .find(|r| r.starts_with("desktop · "))
        .expect("row");
    assert_eq!(
        label, "desktop · Waiting for approval",
        "short; the detail is in its card"
    );
    let detail = ws.read_with(cx, |ws, cx| {
        let sidebar = ws.sidebar.read(cx);
        let id: gpui::SharedString = format!("linked-state-{NODE_NAME}").into();
        let card = sidebar
            .marks
            .get(&id)
            .and_then(|m| m.card.clone())
            .expect("card");
        card.note.clone().expect("state").to_string()
    });
    assert!(detail.starts_with("Waiting for approval: "), "{detail}");
    click_row(&ws, &label, cx);
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| !ws.is_remote()),
        "the row doesn't switch machines"
    );
}

#[gpui::test]
fn linked_thread_finishing_unseen_reads_done(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    let set = |status: &str| {
        let store = remote.store.lock().expect("store");
        store
            .set_thread_status(remote.thread_id, status)
            .expect("status");
    };
    set("running");
    relist(&ws, &remote, cx);
    assert!(
        raw_sidebar(&ws, cx)
            .iter()
            .any(|(t, r)| t == "Working" && r.contains(&ROW.to_owned()))
    );
    assert_eq!(marks_of(&ws, ROW, cx).map(|m| m.1), Some(false));
    set("idle");
    relist(&ws, &remote, cx);
    assert_eq!(marks_of(&ws, ROW, cx).map(|m| m.1), Some(true), "Done");
    click_row(&ws, ROW, cx);
    cx.run_until_parked();
    assert_eq!(marks_of(&ws, ROW, cx).map(|m| m.1), Some(false), "seen");
}

#[gpui::test]
fn linked_listing_without_change_keeps_sidebar(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    relist(&ws, &remote, cx);
    let before = ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).rebuilds);
    relist(&ws, &remote, cx);
    relist(&ws, &remote, cx);
    let after = ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).rebuilds);
    assert_eq!(after, before, "an unchanged listing rebuilds nothing");
    let set = |status: &str| {
        let store = remote.store.lock().expect("store");
        store
            .set_thread_status(remote.thread_id, status)
            .expect("status");
    };
    set("running");
    relist(&ws, &remote, cx);
    let changed = ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).rebuilds);
    assert!(changed > after, "a changed one does");
}

#[gpui::test]
fn unlinking_drops_rows_and_tabs(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    click_row(&ws, ROW, cx);
    wait_until(cx, "the thread page", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len() == 1)
    });
    link(&ws, cx); // again: unlinks
    assert!(!rows(&ws, cx).contains(&ROW.to_owned()));
    ws.read_with(cx, |ws, _| {
        assert!(ws.remote_tabs().is_empty());
        assert!(ws.linked_envs().is_empty());
        assert!(matches!(ws.page, Page::Welcome));
    });
}

#[gpui::test]
fn linking_is_saved_and_reconnects_at_startup(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let (ws, _local, cx) =
        local_workspace(Arc::default(), Some(path.clone()), remote.options(), cx);
    cx.run_until_parked();
    linked_and_listed(&ws, cx);
    wait_until(cx, "saved", |_| {
        skyelab_core::AppSettings::load(&path).is_ok_and(|s| s.linked_environments == [NODE_NAME])
    });
    // A window opened on that file links it again by itself.
    let (ws2, cx2) = cx.add_window_view(|window, cx| {
        let backend = ws.read(cx).backend.clone();
        let mut ws = Workspace::new(
            std::env::temp_dir(),
            None,
            AccountsConfig::default(),
            None,
            backend,
            window,
            cx,
        );
        ws.set_environments(
            Arc::new(FakeTailnet::default()),
            Some(path.clone()),
            remote.options(),
            window,
            cx,
        );
        ws
    });
    wait_until(cx2, "listed at startup", |cx| {
        rows(&ws2, cx).contains(&ROW.to_owned())
    });
}

#[gpui::test]
fn the_palette_links_machines_and_opens_their_threads(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    picker.update(cx, |p, _| p.recents = vec![NODE_NAME.into()]);
    let toggle = format!("Show threads from {NODE_NAME} in this list");
    let find = |ws: &Entity<Workspace>, label: &str, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.linked_palette_entries(cx)
                .into_iter()
                .find(|e| e.label == label)
        })
    };
    let entry = find(&ws, &toggle, cx).expect("a recent machine's toggle");
    assert_eq!(entry.checked, Some(false));
    ws.update_in(cx, |_, window, cx| (entry.run)(window, cx));
    wait_until(cx, "listed", |cx| rows(&ws, cx).contains(&ROW.to_owned()));
    assert_eq!(find(&ws, &toggle, cx).expect("toggle").checked, Some(true));
    let open = find(&ws, &format!("Open thread: {ROW} · desktop"), cx).expect("its thread");
    ws.update_in(cx, |_, window, cx| (open.run)(window, cx));
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.remote_tabs().len()), 1);
    assert!(ws.read_with(cx, |ws, _| !ws.is_remote()));
}

#[gpui::test]
fn a_linked_thread_shown_again_by_forward_is_not_done(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    let set = |status: &str| {
        let store = remote.store.lock().expect("store");
        store
            .set_thread_status(remote.thread_id, status)
            .expect("status");
    };
    set("running");
    relist(&ws, &remote, cx);
    click_row(&ws, ROW, cx);
    cx.run_until_parked();
    cx.dispatch_action(super::GoBack);
    set("idle");
    relist(&ws, &remote, cx);
    assert_eq!(
        marks_of(&ws, ROW, cx).map(|m| m.1),
        Some(true),
        "Done while away"
    );
    cx.dispatch_action(super::GoForward);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| matches!(ws.page, Page::RemoteThread(_))));
    assert_eq!(
        marks_of(&ws, ROW, cx).map(|m| m.1),
        Some(false),
        "seen again"
    );
}

#[gpui::test]
fn leaving_machines_closes_every_connection(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    connect(&ws, &remote, cx);
    let daemon = remote.daemon.as_ref().expect("served");
    wait_until(cx, "both connections", |_| daemon.peers().len() == 2);
    // What a closing window does, even if the workspace outlives it a while.
    ws.update(cx, |ws, cx| ws.leave_machines(cx));
    wait_until(cx, "every connection to close", |_| {
        daemon.peers().is_empty()
    });
}

/// A linked machine's thread settled from This Mac's list (protocol 1.6): settled there, listed here under Settled.
#[gpui::test]
fn linked_thread_settles_on_its_machine(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    linked_and_listed(&ws, cx);
    let a = super::thread_list::ShelveThread {
        id: remote.thread_id,
        to: super::thread_list::Shelf::Settle,
        host: Some(NODE_NAME.into()),
    };
    ws.update_in(cx, |ws, window, cx| ws.shelve_thread(&a, window, cx));
    wait_until(cx, "the Settled shelf", |cx| {
        ws.read_with(cx, |ws, cx| {
            let shelves = &ws.sidebar.read(cx).shelves;
            shelves.len() == 1 && shelves[0].title == "Settled" && shelves[0].items[0].label == ROW
        })
    });
    assert!(!raw_sidebar(&ws, cx).iter().any(|(_, r)| r.contains(&ROW.to_owned())));
    let store = remote.store.lock().expect("store");
    let t = store.thread(remote.thread_id).expect("read").expect("thread");
    assert!(t.settled_at.is_some(), "settled on its machine");
}

/// The device key (on macOS a keychain read that may prompt) loads on the first connection, never at startup.
#[gpui::test]
fn the_device_key_loads_on_first_remote_use_only(cx: &mut TestAppContext) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let remote = remote_side(FakeTailnet::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut options = remote.options();
    options.device_key = Some(skyelab_client::LazyDeviceKey::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(skyelab_client::testing::DEVICE_KEY.into())
    }));
    let (ws, _local, cx) = local_workspace(Arc::default(), None, options, cx);
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0, "started without remote use");
    linked_and_listed(&ws, cx);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "linked: loaded once");
}
