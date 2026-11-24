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
}
