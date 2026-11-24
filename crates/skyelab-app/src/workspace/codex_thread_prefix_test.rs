//! A Codex command ask offers "Allow "cargo test …" for this thread"; taking it allows that ask and the thread's next
//! `cargo test` command without asking, both rows naming the rule.

use super::claude_folded_turns_test::{one_view, row, wait_until};
use super::thread_items::ItemsView as _;
use super::thread_model::{ApprovalItem, Item};
use gpui::TestAppContext;
use skyelab_agent::Decision;
use skyelab_core::Provider;
use std::path::Path;

fn approvals(items: &[Item]) -> Vec<ApprovalItem> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Approval(a) => Some(a.clone()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn a_codex_prefix_allow_covers_the_threads_next_matching_command(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("approve2 please", window, cx))
        .expect("started");
    wait_until(cx, "the first ask", |cx| {
        one.view
            .read_with(cx, |v, _| !approvals(&v.items).is_empty())
    });
    let (id, prefix) = one.view.read_with(cx, |v, _| {
        let a = approvals(&v.items).remove(0);
        (a.id.clone(), v.thread_prefix(&a))
    });
    assert_eq!(prefix.as_deref(), Some("cargo test"));
    one.view.update_in(cx, |v, window, cx| {
        v.allow_thread_prefix(&id, "cargo test", window, cx)
    });
    wait_until(cx, "the turn's result", |cx| {
        row(&one, cx).is_some_and(|(status, n)| n == 1 && status == "idle")
    });
    let decided: Vec<_> = one.view.read_with(cx, |v, _| {
        approvals(&v.items)
            .into_iter()
            .map(|a| (a.summary, a.decision, a.rule))
            .collect()
    });
    let by = Some("shell · starts with cargo test".to_owned());
    assert_eq!(
        decided,
        vec![
            ("cargo test --lib".into(), Some(Decision::AllowThread), by.clone()),
            ("cargo test --doc".into(), Some(Decision::AllowThread), by),
        ]
    );
}
