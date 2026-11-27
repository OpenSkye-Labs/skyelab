//! Criterion 10: `list_machines` lists only linked machines with "Let my agents delegate here" on, with their
//! state, and a call to a machine with the switch off is refused before anything is sent.

use super::remote_delegation_a_harness::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_agent::mcp::Tool;

#[gpui::test]
fn the_switch_off_lists_and_reaches_nothing(cx: &mut TestAppContext) {
    let (a, cx) = a_side(false, cx);
    assert_eq!(machines(&a, cx), serde_json::json!([]));
    let e = on_b(&a, Tool::ListProjects, cx).expect_err("switch off");
    assert!(e.contains("is off"), "{e}");
    assert!(delegate_calls(&a).is_empty(), "nothing was sent");
}

#[gpui::test]
fn the_switch_on_lists_ready_then_offline(cx: &mut TestAppContext) {
    let (mut a, cx) = a_side(true, cx);
    let listed = machines(&a, cx);
    assert_eq!(listed[0]["machine"], B);
    assert_eq!(listed[0]["state"], "ready");
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert!(on_b(&a, Tool::ListProjects, cx).is_ok());
    assert_eq!(delegate_calls(&a).len(), 1);
    // The caller's account goes by its label too, so B's request can name it as the user does.
    let callers = a.b.agents.callers.lock().expect("callers").clone();
    assert_eq!(callers[0].account_label.as_deref(), Some("Work"));
    a.b.stop();
    wait_until(cx, "B offline", |cx| {
        a.ws.read_with(cx, |ws, cx| ws.linked_envs().iter().all(|e| e.read(cx).node().is_none()))
    });
    assert_eq!(machines(&a, cx)[0]["state"], "offline");
}
