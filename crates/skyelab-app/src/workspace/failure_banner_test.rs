//! A failed turn shows a banner over the composer with Retry and View output; a signed-out account is named and
//! pointed at Settings → Accounts; an interrupt or a finished turn shows none.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_failure::failure;
use super::thread_model::Item;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::Provider;
use std::path::Path;

fn done(is_error: bool, error: Option<&str>) -> Item {
    Item::Done {
        is_error,
        duration_ms: 1,
        cost_usd: 0.,
        usage: None,
        at_ms: None,
        error: error.map(str::to_owned),
    }
}

fn output(text: &str) -> Item {
    Item::Output {
        id: "output-9".into(),
        text: text.into(),
    }
}

#[test]
fn which_turns_count_as_failed() {
    let p = || Item::Prompt("go".into());
    assert_eq!(failure(&[]), None);
    assert_eq!(failure(&[p(), done(false, None)]), None);
    assert_eq!(
        failure(&[p(), done(true, Some("interrupted"))]),
        None,
        "the user asked for it"
    );
    let f = failure(&[p(), done(true, Some("API Error: 500 overloaded"))]).expect("failed");
    assert_eq!(
        (f.row, f.text.as_str(), f.signed_out),
        (1, "API Error: 500 overloaded", false)
    );
    // The agent exited after an earlier result: its stderr says why.
    let f = failure(&[
        p(),
        done(false, None),
        output("\nfatal: lost the session\nmore"),
    ])
    .expect("crash");
    assert_eq!((f.row, f.text.as_str()), (2, "fatal: lost the session"));
    assert_eq!(f.output.as_deref(), Some("output-9"));
    assert_eq!(f.prompt, "go");
    let f = failure(&[p(), output("Error: 401 Unauthorized")]).expect("signed out");
    assert!(f.signed_out);
    for other in [
        "connect ECONNREFUSED 127.0.0.1:4010",
        "process 14012 exited unexpectedly",
        "syntax error at line 401",
    ] {
        let f = failure(&[p(), output(other)]).expect("a crash");
        assert!(!f.signed_out, "{other} is not a signed-out account");
    }
    let f =
        failure(&[p(), done(true, Some("API Error: 401 {\"type\":\"error\"}"))]).expect("failed");
    assert!(f.signed_out, "an API 401");
    // A newer prompt starts over.
    assert_eq!(failure(&[p(), done(true, Some("x")), p()]), None);
}

fn view(cx: &mut TestAppContext) -> (One, &mut VisualTestContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    one_view(cx, Provider::Claude, bin, None)
}

fn prompts(one: &One, cx: &mut VisualTestContext) -> usize {
    one.view.read_with(cx, |v, _| {
        v.items
            .iter()
            .filter(|i| matches!(i, Item::Prompt(_)))
            .count()
    })
}

#[gpui::test]
fn crash_shows_banner_with_retry_and_output(cx: &mut TestAppContext) {
    let (one, cx) = view(cx);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("crash", window, cx))
        .expect("sent");
    wait_until(cx, "the crash banner", |cx| {
        one.view.read_with(cx, |v, _| v.shown_failure().is_some())
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("failure-banner").is_some());
    let f = one
        .view
        .read_with(cx, |v, _| v.shown_failure())
        .expect("failure");
    assert_eq!(f.text, "fake-claude: lost the session");
    assert!(!f.signed_out);

    // View output opens the stderr row.
    let id = f.output.clone().expect("a stderr row");
    one.view.update(cx, |v, cx| v.view_failure_output(cx));
    assert!(one.view.read_with(cx, |v, _| v.is_expanded(&id)));

    // Retry sends the same prompt again.
    one.view
        .update_in(cx, |v, window, cx| v.retry_failed(window, cx));
    wait_until(cx, "the retried prompt", |cx| prompts(&one, cx) == 2);
    let last = one.view.read_with(cx, |v, _| {
        v.items.iter().rev().find_map(|i| match i {
            Item::Prompt(t) => Some(t.clone()),
            _ => None,
        })
    });
    assert_eq!(last.as_deref(), Some("crash"));

    // Closed, it stays closed for that failure.
    wait_until(cx, "the second crash", |cx| {
        one.view.read_with(cx, |v, _| v.shown_failure().is_some())
    });
    let row = one
        .view
        .read_with(cx, |v, _| v.shown_failure())
        .expect("f")
        .row;
    one.view.update(cx, |v, cx| {
        v.timeline.dismissed_failure = Some(row);
        cx.notify()
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("failure-banner").is_none());
}

#[gpui::test]
fn signed_out_failure_names_the_account(cx: &mut TestAppContext) {
    let (one, cx) = view(cx);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("login", window, cx))
        .expect("sent");
    wait_until(cx, "the failed turn", |cx| {
        one.view.read_with(cx, |v, _| v.shown_failure().is_some())
    });
    let f = one
        .view
        .read_with(cx, |v, _| v.shown_failure())
        .expect("failure");
    assert!(f.signed_out, "{f:?}");
    assert_eq!(
        one.view.read_with(cx, |v, _| v.failure_title(&f)),
        "Account work account isn't signed in"
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("failure-banner").is_some());
}
