//! Switcher follow-ups: MagicDNS-only typed names, the remote account's label, reopening a tab quickly, and a
//! remembered name that now answers from a different machine.

use super::env_harness::*;
use super::env_mode::{OpenRemoteThread, TrustRemoteNode};
use super::remote_thread::RemoteThreadView;
use super::thread_model::Item;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::{Account, AppSettings};
use skyelab_daemon::testing::NODE_NAME;

fn texts(view: &Entity<RemoteThreadView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.items
            .iter()
            .filter_map(|i| match i {
                Item::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    })
}

/// Connected, "app" selected, and its one thread open in a tab.
fn open_thread(
    ws: &Entity<Workspace>,
    remote: &RemoteSide,
    cx: &mut VisualTestContext,
) -> Entity<RemoteThreadView> {
    connect(ws, remote, cx);
    click_row(ws, "app", cx);
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
fn typed_names_must_be_magicdns_names(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    cx.dispatch_action(env_picker::ToggleEnvironments);
    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    for bad in ["100.64.0.1", "localhost", "laptop", "fd7a:115c:a1e0::1"] {
        picker.update_in(cx, |p, window, cx| p.set_typed(bad, window, cx));
        cx.dispatch_action(env_picker::ConnectTyped);
        cx.run_until_parked();
        assert!(
            ws.read_with(cx, |ws, _| ws.remote_env().is_none()),
            "{bad:?} must not start a connection"
        );
        picker.read_with(cx, |p, _| {
            let error = p.error.clone().expect("an error is shown");
            assert!(error.contains("ts.net"), "{error}");
        });
    }
}

#[gpui::test]
fn remote_threads_show_the_account_label(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let accounts = remote._dir.path().join("accounts.toml");
    AccountsConfig {
        accounts: vec![Account {
            id: "work".into(),
            label: "Work (Team)".into(),
            config_dir: remote._dir.path().join("claude-work"),
            gh_config_dir: None,
        }],
        ..AccountsConfig::default()
    }
    .save(&accounts)
    .expect("save accounts");
    remote.accounts = Some(accounts);
    remote.stop();
    remote.serve();
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let view = open_thread(&ws, &remote, cx);
    view.read_with(cx, |v, cx| {
        assert_eq!(v.account(cx).as_deref(), Some("Work (Team) · work"))
    });
}

#[gpui::test]
fn closing_and_reopening_a_tab_at_once_keeps_its_events(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    remote.store_texts(remote.thread_id, &["first"], false);
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let old = open_thread(&ws, &remote, cx);
    wait_until(cx, "stored events", |cx| texts(&old, cx) == ["first"]);

    // Close and reopen in one go: the old view is dropped (and unwatches) after the new one has watched.
    let id = remote.thread_id;
    ws.update_in(cx, move |ws, window, cx| {
        ws.dock
            .update(cx, |dock, cx| dock.remove_panel(old, window, cx));
        ws.open_remote_thread(&OpenRemoteThread { id }, window, cx);
    });
    let tabs = ws.read_with(cx, |ws, _| ws.remote_tabs());
    assert_eq!(tabs.len(), 1, "one tab after reopening");
    let new = tabs[0].clone();
    wait_until(cx, "the reopened tab's events", |cx| {
        texts(&new, cx) == ["first"]
    });
    remote.store_texts(remote.thread_id, &["second"], true);
    wait_until(cx, "a live event in the reopened tab", |cx| {
        texts(&new, cx) == ["first", "second"]
    });
}

fn recorded(path: &std::path::Path) -> Option<String> {
    AppSettings::load(path)
        .expect("load")
        .environment_nodes
        .get(NODE_NAME)
        .cloned()
}

/// The remote's daemon comes back as another tailnet node under the same name.
fn become_node(remote: &mut RemoteSide, id: &str) {
    *remote.tailnet.self_id.lock().expect("self id") = Some(id.into());
    remote.stop();
    remote.serve();
}

#[gpui::test]
fn a_name_that_moved_to_another_machine_is_refused_until_trusted(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let (ws, _local, cx) = local_workspace(
        remote.tailnet.clone(),
        Some(path.clone()),
        remote.options(),
        cx,
    );

    // First connect: the node is recorded.
    connect(&ws, &remote, cx);
    wait_until(cx, "the node recorded", |_| {
        recorded(&path).as_deref() == Some("nSelf")
    });

    // Later, the same name answers from another node.
    switch(&ws, None, cx);
    become_node(&mut remote, "nImpostor");
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the warning", |cx| {
        state(&ws, cx).contains("different machine")
    });
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "no projects from it");
    assert_eq!(recorded(&path).as_deref(), Some("nSelf"), "not overwritten");

    // Trusting a node other than the one shown is refused.
    ws.update_in(cx, |ws, window, cx| {
        ws.trust_remote_node(
            &TrustRemoteNode {
                node_id: "nSomethingElse".into(),
            },
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(state(&ws, cx).contains("different machine"));
    assert_eq!(recorded(&path).as_deref(), Some("nSelf"));

    // Trusting the shown one saves it and connects.
    ws.update_in(cx, |ws, window, cx| {
        ws.trust_remote_node(
            &TrustRemoteNode {
                node_id: "nImpostor".into(),
            },
            window,
            cx,
        )
    });
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
    wait_until(cx, "projects", |cx| sidebar(&ws, cx)[0].1 == ["app"]);
    wait_until(cx, "the new node saved", |_| {
        recorded(&path).as_deref() == Some("nImpostor")
    });

    // A reconnect within the session is checked too, and the other machine's tabs close.
    click_row(&ws, "app", cx);
    wait_until(cx, "remote threads", |cx| !sidebar(&ws, cx)[1].1.is_empty());
    click_row(&ws, "fix the login bug", cx);
    wait_until(cx, "the thread tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });
    become_node(&mut remote, "nThird");
    wait_until(cx, "the warning again", |cx| {
        state(&ws, cx).contains("different machine")
    });
    assert!(ws.read_with(cx, |ws, _| ws.remote_tabs().is_empty()));
    assert!(sidebar(&ws, cx)[0].1.is_empty());
}
