//! Trusting a linked machine's new node through the switcher brings its threads back to This Mac's list.

use super::env_harness::*;
use super::env_linked::ToggleLinkedEnvironment;
use super::env_mode::TrustRemoteNode;
use super::*;
use gpui::TestAppContext;
use skyelab_core::AppSettings;
use skyelab_daemon::testing::NODE_NAME;

const ROW: &str = "fix the login bug";

#[gpui::test]
fn trusting_a_linked_machine_through_the_switcher_restores_its_rows(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let mut s = AppSettings::default();
    s.trust_environment_node(NODE_NAME, "nOld");
    s.save(&path).expect("save");
    let (ws, _local, cx) =
        local_workspace(Arc::default(), Some(path.clone()), remote.options(), cx);
    cx.run_until_parked();

    let action = ToggleLinkedEnvironment {
        host: NODE_NAME.into(),
    };
    ws.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(action), cx)
    });
    wait_until(cx, "the linked machine's state row", |cx| {
        raw_sidebar(&ws, cx)
            .iter()
            .any(|(t, r)| t == "Linked machines" && r.iter().any(|r| r.contains("Changed machine")))
    });

    // As the user guide says: switch to that machine and Trust it there.
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the warning", |cx| {
        state(&ws, cx).contains("different machine")
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.trust_remote_node(
            &TrustRemoteNode {
                node_id: "nSelf".into(),
            },
            window,
            cx,
        )
    });
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
    wait_until(cx, "the new node saved", |_| {
        AppSettings::load(&path).is_ok_and(|s| s.environment_node(NODE_NAME) == Some("nSelf"))
    });

    switch(&ws, None, cx);
    wait_until(
        cx,
        "linked machine's threads back in This Mac's list",
        |cx| {
            raw_sidebar(&ws, cx)
                .iter()
                .flat_map(|(_, r)| r)
                .any(|r| r == ROW)
        },
    );
}
