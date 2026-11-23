//! Acceptance: the workspace reads every linked PR, including a thread that was never opened, shows its state on
//! the sidebar badge, keeps it in the store, and stops reading once the PR merged. Fake gh only.

use super::manage_harness::rig;
use super::pr_watch::PrStatuses;
use super::threads_test::wait_until;
use gpui::SharedString;
use skyelab_git::{Checks, PrState};

const URL: &str = "https://github.com/acme/demo/pull/42";

#[gpui::test]
fn never_opened_thread_gets_its_pr_state_and_merged_stops_reads(cx: &mut gpui::TestAppContext) {
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
    std::fs::write(
        gh_dir.join("fake-pr.json"),
        r#"{"state":"OPEN","isDraft":false,"statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE"}]}"#,
    )
    .expect("pr");
    let fake_gh = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../skyelab-git/tests/fixtures/fake-gh.sh");
    let gh = gh_dir.clone();
    r.ws.update(cx, |ws, cx| {
        cx.set_global(super::GhBin(fake_gh));
        ws.config.accounts[0].gh_config_dir = Some(gh);
    });
    let id = r.thread("Never opened", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_pr_url(id, Some(URL))
        .expect("link");
    r.reload(cx);

    let calls =
        || std::fs::read_to_string(gh_dir.join("fake-gh.prview")).map_or(0, |t| t.lines().count());
    let badge = |cx: &mut gpui::VisualTestContext| {
        let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| {
            s.marks
                .get(&SharedString::from(format!("thread-{id}")))
                .and_then(|m| m.pr.as_ref().map(|p| (p.state, p.checks)))
        })
    };
    // Loading the thread list reads its PRs, whatever page shows: no thread was opened or shown.
    wait_until(cx, "first read", |cx| {
        badge(cx).is_some_and(|(s, _)| s.is_some())
    });
    assert_eq!(badge(cx), Some((Some(PrState::Open), Checks::Failing)));
    assert_eq!(calls(), 1);
    assert_eq!(r.row(id).and_then(|t| t.pr_state).as_deref(), Some("open"));

    // Not due again yet: no second call.
    r.ws.update(cx, |ws, cx| ws.poll_prs(true, cx));
    cx.run_until_parked();
    assert_eq!(calls(), 1);

    let age = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| {
            if let Some(e) = cx.global_mut::<PrStatuses>().0.get_mut(&id) {
                e.checked_ms = 0;
            }
        })
    };
    std::fs::write(
        gh_dir.join("fake-pr.json"),
        r#"{"state":"MERGED","mergedAt":"2026-11-02T00:00:00Z"}"#,
    )
    .expect("pr");
    age(cx);
    r.ws.update(cx, |ws, cx| ws.poll_prs(true, cx));
    wait_until(cx, "merged read", |cx| {
        badge(cx).is_some_and(|(s, _)| s == Some(PrState::Merged))
    });
    assert_eq!(calls(), 2);
    assert_eq!(
        r.row(id).and_then(|t| t.pr_state).as_deref(),
        Some("merged")
    );

    // Merged: never read again, however old the last read.
    age(cx);
    r.ws.update(cx, |ws, cx| ws.poll_prs(true, cx));
    cx.run_until_parked();
    assert_eq!(calls(), 2);
}
