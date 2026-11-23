//! The context meter: the fake's `context` turn ends with 170k of a 200k window in context, so the footer and the
//! details card show 85% with the /compact hint, live and again after the thread is reopened from the store.

use super::Backend;
use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread::ThreadView;
use gpui::{AppContext as _, TestAppContext};
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn the_meter_reads_the_last_call_live_and_after_reopen(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin.clone(), None);
    assert_eq!(
        one.view.read_with(cx, |v, _| v.context_meter()),
        None,
        "nothing reported yet"
    );
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("context", window, cx))
        .expect("started");
    wait_until(cx, "the context reading", |cx| {
        one.view
            .read_with(cx, |v, _| v.context_meter().and_then(|m| m.percent))
            == Some(85)
    });
    let meter = one
        .view
        .read_with(cx, |v, _| v.context_meter())
        .expect("meter");
    assert_eq!(meter.text(), "85% context");
    assert_eq!(meter.amount(), "170k / 200k");
    assert!(meter.near_full());
    assert!(meter.tooltip().contains("/compact"), "{}", meter.tooltip());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("footer-context").is_some(),
        "the footer shows the meter"
    );
    // The details card redraws on a new reading: the reading is part of what it compares.
    let sig = one.view.read_with(cx, |v, _| v.details_sig());
    one.view.update(cx, |v, cx| {
        v.push(
            &super::thread_model::Record::Agent(skyelab_agent::Event::Context {
                tokens: Some(20_000),
                window: None,
            }),
            cx,
        )
    });
    let after = one
        .view
        .read_with(cx, |v, _| v.context_meter())
        .expect("meter");
    assert_eq!(
        (after.percent, after.near_full()),
        (Some(10), false),
        "tokens move, the window holds"
    );
    assert!(one.view.read_with(cx, |v, _| v.details_sig()) != sig);

    let (thread, config) = one.view.read_with(cx, |v, _| {
        (v.thread.clone().expect("thread"), v.config.clone())
    });
    let store = one.store.clone();
    let reopened = cx.update(|window, cx| {
        cx.new(|cx| {
            let backend = Backend {
                store,
                claude_bin: bin,
            };
            ThreadView::open(thread, config, backend, window, cx)
        })
    });
    wait_until(cx, "the replayed reading", |cx| {
        reopened.read_with(cx, |v, _| v.context_meter()).is_some()
    });
    assert_eq!(
        reopened.read_with(cx, |v, _| v.context_meter()),
        Some(meter),
        "the stored reading"
    );
}
