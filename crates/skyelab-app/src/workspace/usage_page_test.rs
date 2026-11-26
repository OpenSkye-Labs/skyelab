//! Acceptance: Open Usage shows Settings → Usage with every account in accounts.toml, each with its own spend for the
//! range, its share, and the projects table, from stored turns only.

use super::{Metric, UsagePage, derive};
use crate::workspace::worktree_thread_test::{fixture, open_thread, wait_until};
use crate::workspace::{OpenUsage, Page};
use skyelab_store::NewThread;
use skyelab_store::usage::Range;

fn result(cost: f64, input: u64) -> String {
    format!(
        r#"{{"Result":{{"is_error":false,"duration_ms":5,"total_cost_usd":{cost},"result":"ok","prompts":1,"usage":{{"input":{input},"output":0}}}}}}"#
    )
}

#[gpui::test]
fn open_usage_compares_every_account(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    let project = ws
        .read_with(cx, |ws, _| ws.project.as_ref().map(|p| p.id))
        .expect("project");
    {
        let mut s = f.store.lock().expect("store");
        for (account, cost, input) in [("acme", 3.0, 100), ("acme", 0.5, 50), ("other", 4.0, 1000)]
        {
            let t = s
                .create_thread(&NewThread {
                    project_id: project,
                    title: "t",
                    cwd: &f.project,
                    account_id: account,
                    status: "idle",
                })
                .expect("thread");
            s.append_event(t.id, "agent", &result(cost, input))
                .expect("result");
        }
    }

    cx.dispatch_action(OpenUsage);
    let page = ws
        .read_with(cx, |ws, cx| {
            assert!(ws.settings_open, "Usage opens in the Settings modal");
            assert!(
                matches!(ws.page, Page::Thread(_)),
                "the thread page stays under it"
            );
            ws.settings.as_ref().and_then(|s| s.read(cx).usage.clone())
        })
        .expect("usage page");
    wait_until(cx, "usage read", |cx| {
        page.read_with(cx, |p: &UsagePage, _| p.data.is_some())
    });

    page.read_with(cx, |p, _| {
        let v = &p.view;
        let ids: Vec<&str> = v.accounts.iter().map(|a| a.usage.id.as_str()).collect();
        assert_eq!(
            ids,
            ["acme", "other"],
            "every account, in accounts.toml order"
        );
        assert_eq!(v.accounts[0].spend.cost_usd, 3.5);
        assert_eq!(v.accounts[0].spend.turns, 2);
        assert!((v.accounts[1].share - 4.0 / 7.5).abs() < 1e-9);
        assert_eq!(v.total.cost_usd, 7.5);
        assert_eq!(
            v.days.last().map(|d| d.iter().sum::<f64>()),
            Some(7.5),
            "today's bar"
        );
        assert_eq!(v.days.len(), 7, "the default range (7 days) charts 7 days");
        assert_eq!(v.projects.len(), 2, "one line per project and account");
        assert_eq!(v.projects[0].account_id, "other", "most first");
    });

    // Tokens instead of cost: the share follows.
    page.update(cx, |p, cx| p.set_metric(Metric::Tokens, cx));
    page.read_with(cx, |p, _| {
        assert_eq!(p.view.accounts[1].spend.tokens(), 1000);
        assert!(p.view.accounts[1].share > 0.86);
        assert_eq!(p.view.projects[0].account_id, "other");
    });
    // Derive is the same function render reads.
    let data = page.read_with(cx, |p, _| p.data.clone()).expect("data");
    assert_eq!(derive(&data, Range::Today, Metric::Cost).total.turns, 3);
    // The chart follows the range: a month charts 30 days, Today a week.
    assert_eq!(derive(&data, Range::Month, Metric::Cost).days.len(), 30);
    assert_eq!(derive(&data, Range::Today, Metric::Cost).days.len(), 7);
}

/// Acceptance: while Usage is shown, each account's own CLI is asked for its limits (the fake answers from its account
/// dir's fake-usage.json); the numbers land on the page, and a failed read says so under its account.
#[gpui::test]
fn shown_usage_asks_each_account_for_its_limits(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    std::fs::create_dir_all(&f.account_dir).expect("acme dir");
    std::fs::write(
        f.account_dir.join("fake-usage.json"),
        r#"{"rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":42,"resets_at":"2099-01-01T00:00:00Z"},"seven_day":{"utilization":17,"resets_at":"2099-01-05T00:00:00Z"}}}"#,
    )
    .expect("acme usage");
    let other = f.account_dir.with_file_name(".claude-other");
    std::fs::create_dir_all(&other).expect("other dir");
    // Available but empty: the CLI's own read failed.
    std::fs::write(
        other.join("fake-usage.json"),
        r#"{"rate_limits_available":true,"rate_limits":null}"#,
    )
    .expect("other usage");
    let (ws, _thread, cx) = open_thread(&f, cx);

    cx.dispatch_action(OpenUsage);
    let page = ws
        .read_with(cx, |ws, cx| {
            ws.settings.as_ref().and_then(|s| s.read(cx).usage.clone())
        })
        .expect("usage page");
    wait_until(cx, "polled limits shown", |cx| {
        page.read_with(cx, |p: &UsagePage, _| {
            p.view.accounts.len() == 2
                && !p.view.accounts[0].usage.windows.is_empty()
                && p.view.accounts[1].usage.refresh_error.is_some()
        })
    });
    page.read_with(cx, |p, _| {
        let acme = &p.view.accounts[0].usage;
        let got: Vec<_> = acme.windows.iter().map(|w| (w.label, w.used)).collect();
        assert_eq!(got, [("5h", 0.42), ("Weekly", 0.17)]);
        assert_eq!(acme.refresh_error, None);
        assert_eq!(
            p.view.accounts[1].usage.refresh_error.as_deref(),
            Some("Claude had no usage to report")
        );
    });
    let calls = std::fs::read_to_string(f.account_dir.join("fake-usage.calls")).expect("asked");
    assert_eq!(
        calls.trim(),
        f.account_dir.to_str().expect("utf-8"),
        "acme's CLI ran once, with acme's own config dir"
    );
}
