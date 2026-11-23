//! A message sent from another device mid-turn is folded into the running turn by Claude (one result answers both),
//! so the turn's fold must cover the work before it, the same as a steer.

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::Item;
use super::timeline_fold::folded_by;
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::os::unix::fs::PermissionsExt as _;

const FAKE: &str = r#"#!/bin/sh
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-mid","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
uuid() { printf '%s' "$1" | sed -n 's/.*"uuid":"\([^"]*\)".*/\1/p'; }
IFS= read -r first
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"looking"}]},"parent_tool_use_id":null}\n'
IFS= read -r second
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]},"parent_tool_use_id":null}\n'
printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"num_turns":1,"result":"done","session_id":"fake-mid","total_cost_usd":0,"user_message_uuids":["%s","%s"]}\n' "$(uuid "$first")" "$(uuid "$second")"
while IFS= read -r line; do :; done
"#;

#[gpui::test]
fn remote_message_mid_turn_stays_in_the_turns_fold(cx: &mut TestAppContext) {
    let scripts = tempfile::tempdir().expect("tempdir");
    let bin = scripts.path().join("fake-claude-midturn.sh");
    std::fs::write(&bin, FAKE).expect("script");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("first", window, cx))
        .expect("started");
    wait_until(cx, "work streamed in the running turn", |cx| {
        one.view.read_with(cx, |v, _| {
            v.turn_running()
                && v.items
                    .iter()
                    .any(|i| matches!(i, Item::Text(t) if t == "looking"))
        })
    });
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("second", window, cx))
        .expect("sent mid-turn");
    wait_until(cx, "the one result for both", |cx| {
        one.view.read_with(cx, |v, _| {
            v.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    let items = one.view.read_with(cx, |v, _| v.items.clone());
    let work = items
        .iter()
        .position(|i| matches!(i, Item::Text(t) if t == "looking"))
        .expect("work row");
    let done = items
        .iter()
        .position(|i| matches!(i, Item::Done { .. }))
        .expect("done");
    assert_eq!(
        folded_by(&items, work),
        Some(done),
        "work before a mid-turn message folds under its turn: {items:?}"
    );
}
