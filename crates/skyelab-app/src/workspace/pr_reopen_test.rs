//! Acceptance: a closed (not merged) PR is read again when its thread is opened or its window focused, at most once
//! an hour, so a reopened PR stops showing Closed. Background polls still skip it, and a merged PR stays final.

use super::manage_harness::rig;
use super::pr_watch::{CLOSED_RECHECK_MS, PrStatuses};
use super::threads_test::wait_until;
use gpui::SharedString;
use skyelab_git::PrState;

const URL: &str = "https://github.com/acme/demo/pull/42";
const OPEN: &str = r#"{"state":"OPEN","isDraft":false,"statusCheckRollup":[]}"#;

#[gpui::test]
fn a_reopened_pr_is_read_again_on_open_and_focus_hourly(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let dir = r.f.project_dir.clone();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("git")
            .success()
    );
    let gh_dir = dir.parent().expect("root").join("gh-acme");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    std::fs::write(gh_dir.join("fake-login"), "octo").expect("login");
    std::fs::write(gh_dir.join("hosts.yml"), "github.com:\n    user: octo\n").expect("hosts");
    std::fs::write(gh_dir.join("fake-pr.json"), OPEN).expect("pr");
    let fake_gh = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../skyelab-git/tests/fixtures/fake-gh.sh");
    let gh = gh_dir.clone();
    r.ws.update(cx, |ws, cx| {
        cx.set_global(super::GhBin(fake_gh));
        ws.config.accounts[0].gh_config_dir = Some(gh);
    });
    // As a restart leaves them: the store says closed or merged, nothing was read this session.
    let closed = r.thread("Closed then reopened", cx);
    let merged = r.thread("Merged", cx);
    {
        let s = r.f.store.lock().expect("lock");
        for (id, state) in [(closed, "closed"), (merged, "merged")] {
            s.set_thread_pr_url(id, Some(URL)).expect("link");
            s.set_thread_pr_state(id, URL, state).expect("state");
        }
    }
    r.reload(cx);
    cx.run_until_parked();
    let calls =
        || std::fs::read_to_string(gh_dir.join("fake-gh.prview")).map_or(0, |t| t.lines().count());
    let badge = |cx: &mut gpui::VisualTestContext, id: i64| {
        let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| {
            s.marks
                .get(&SharedString::from(format!("thread-{id}")))
                .and_then(|m| m.pr.as_ref().and_then(|p| p.state))
        })
    };
    // The background poll on load skips both.
    r.ws.update(cx, |ws, cx| ws.poll_prs(false, cx));
    cx.run_until_parked();
    assert_eq!(calls(), 0, "no background read of a closed or merged PR");
    assert_eq!(badge(cx, closed), Some(PrState::Closed));

    // A merged PR stays final, even when its thread is opened.
    r.open(merged, cx);
    cx.run_until_parked();
    assert_eq!(calls(), 0, "a merged PR is never read again");

    // A second window, whose sidebar must follow the read the first one makes.
    cx.dispatch_action(super::NewWindow);
    cx.run_until_parked();
    let other = cx.update(|_, cx| super::windows::workspaces(cx)[1].clone());
    let other_badge = |cx: &mut gpui::VisualTestContext| {
        let sidebar = other.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| {
            s.marks
                .get(&SharedString::from(format!("thread-{closed}")))
                .and_then(|m| m.pr.as_ref().and_then(|p| p.state))
        })
    };
    assert_eq!(other_badge(cx), Some(PrState::Closed));

    // Opening the closed one reads it: it was reopened.
    r.open(closed, cx);
    wait_until(cx, "the reopened PR read", |cx| {
        badge(cx, closed) == Some(PrState::Open)
    });
    assert_eq!(
        other_badge(cx),
        Some(PrState::Open),
        "the other window's badge too"
    );
    assert_eq!(calls(), 1);
    assert_eq!(
        r.row(closed).and_then(|t| t.pr_state).as_deref(),
        Some("open")
    );

    // Closed again. Focus within the hour doesn't read; an hour later it does.
    std::fs::write(gh_dir.join("fake-pr.json"), r#"{"state":"CLOSED"}"#).expect("pr");
    let age = |cx: &mut gpui::VisualTestContext, by: i64| {
        cx.update(|_, cx| {
            if let Some(e) = cx.global_mut::<PrStatuses>().0.get_mut(&closed) {
                e.checked_ms -= by;
            }
        })
    };
    age(cx, 10 * 60_000);
    r.ws.update(cx, |ws, cx| ws.poll_prs(true, cx));
    wait_until(cx, "closed read", |cx| {
        badge(cx, closed) == Some(PrState::Closed)
    });
    assert_eq!(calls(), 2);
    std::fs::write(gh_dir.join("fake-pr.json"), OPEN).expect("pr");
    age(cx, CLOSED_RECHECK_MS - 60_000);
    r.ws.update(cx, |ws, cx| ws.poll_prs(true, cx));
    cx.run_until_parked();
    assert_eq!(calls(), 2, "not within the hour");
    age(cx, 60_000);
    r.ws.update(cx, |ws, cx| ws.poll_prs(false, cx));
    cx.run_until_parked();
    assert_eq!(calls(), 2, "background polls still skip it");
    r.ws.update(cx, |ws, cx| ws.poll_prs(true, cx));
    wait_until(cx, "the hourly re-read", |cx| {
        badge(cx, closed) == Some(PrState::Open)
    });
    assert_eq!(calls(), 3);
}
