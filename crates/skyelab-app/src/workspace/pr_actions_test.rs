//! Acceptance: the PR actions dialog merges and reviews with the folder's bound account's gh (fake gh only), offers
//! only the repo's merge methods, asks once more while checks fail, shows gh's errors, and reads the PR again after.

use super::super::OpenThread;
use super::super::gh_pages::{Kind, Listing};
use super::super::manage_harness::{Rig, rig};
use super::super::pr_watch::{PrStatuses, PrView};
use super::super::threads_test::wait_until;
use super::*;
use gpui::VisualTestContext;
use skyelab_git::PrState;

const URL: &str = "https://github.com/acme/demo/pull/42";

/// The rig's project as a github.com repo, its account (acme) logged in to a fake gh dir, accounts.toml saved.
fn github(r: &Rig, cx: &mut VisualTestContext) -> PathBuf {
    let dir = r.f.project_dir.clone();
    for args in [
        &["init", "-q"][..],
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/demo.git",
        ],
    ] {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    }
    let gh_dir = dir.parent().expect("root").join("gh-acme");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    std::fs::write(gh_dir.join("fake-login"), "octo").expect("login");
    std::fs::write(gh_dir.join("hosts.yml"), "github.com:\n    user: octo\n").expect("hosts");
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.accounts[0].gh_config_dir = Some(gh_dir.clone());
    config.save(&r.f.accounts_path).expect("save");
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    r.ws.update(cx, |ws, cx| {
        cx.set_global(super::super::GhBin(fake));
        // Actions read accounts.toml as saved now, as the app does.
        cx.set_global(super::super::AccountsFile(r.f.accounts_path.clone()));
        ws.config = config;
    });
    gh_dir
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn status(id: i64, cx: &mut VisualTestContext) -> Option<PrView> {
    cx.update(|_, cx| PrStatuses::view(cx, id, URL).cloned())
}

/// A thread with `URL` linked, read once, opened; its PR actions dialog opened from the details card.
fn thread_dialog(r: &Rig, cx: &mut VisualTestContext) -> (i64, Entity<PrActions>) {
    let id = r.thread("Has a PR", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_pr_url(id, Some(URL))
        .expect("link");
    r.reload(cx);
    wait_until(cx, "first PR read", |cx| {
        matches!(status(id, cx), Some(PrView::Status(_)))
    });
    let dialog = r.ws.update_in(cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx);
        let view = ws.current_thread().expect("shown");
        view.update(cx, |v, cx| v.open_pr_actions(window, cx))
            .expect("thread has a PR")
    });
    wait_until(cx, "merge methods read", |cx| {
        dialog.read_with(cx, |d, _| d.methods.is_some())
    });
    (id, dialog)
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

#[gpui::test]
fn merging_with_failing_checks_takes_a_third_click_then_the_pr_reads_merged(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    std::fs::write(
        gh.join("fake-pr.json"),
        r#"{"state":"OPEN","isDraft":false,"statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE"}]}"#,
    )
    .expect("pr");
    let (id, dialog) = thread_dialog(&r, cx);
    assert!(dialog_open(cx));
    assert_eq!(
        dialog.read_with(cx, |d, _| d.target.checks),
        Checks::Failing
    );
    assert_eq!(
        dialog.read_with(cx, |d, _| d.methods.clone()),
        Some(Ok(MergeMethod::ALL.to_vec()))
    );
    dialog.update(cx, |d, cx| {
        d.delete_branch = true;
        cx.notify();
    });
    let press = |cx: &mut VisualTestContext| {
        dialog.update_in(cx, |d, window, cx| d.press_merge(window, cx));
        cx.run_until_parked();
    };
    press(cx);
    assert_eq!(dialog.read_with(cx, |d, _| d.stage), Stage::Confirm);
    press(cx);
    assert_eq!(dialog.read_with(cx, |d, _| d.stage), Stage::ConfirmFailing);
    assert!(
        lines(&gh.join("fake-gh.actions")).is_empty(),
        "not merged yet"
    );
    press(cx);
    wait_until(
        cx,
        "merged read again",
        |cx| matches!(status(id, cx), Some(PrView::Status(s)) if s.state == PrState::Merged),
    );
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [format!("pr merge {URL} --squash --delete-branch")]
    );
    assert!(!dialog_open(cx), "the dialog closes after the merge");
    assert_eq!(
        r.row(id).and_then(|t| t.pr_state).as_deref(),
        Some("merged")
    );
}

#[gpui::test]
fn only_the_repos_methods_are_offered_reviews_send_their_text_and_errors_stay_shown(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    std::fs::write(
        gh.join("fake-repo.json"),
        r#"{"mergeCommitAllowed":false,"squashMergeAllowed":true,"rebaseMergeAllowed":false}"#,
    )
    .expect("repo");
    let (_, dialog) = thread_dialog(&r, cx);
    assert_eq!(
        dialog.read_with(cx, |d, _| d.methods.clone()),
        Some(Ok(vec![MergeMethod::Squash]))
    );

    // Request changes needs text: nothing runs without it.
    dialog.update_in(cx, |d, window, cx| {
        d.review(Verdict::RequestChanges, window, cx)
    });
    cx.run_until_parked();
    assert!(lines(&gh.join("fake-gh.actions")).is_empty());
    let body = dialog.read_with(cx, |d, _| d.body.clone());
    body.update_in(cx, |b, window, cx| {
        b.set_value("Please add a test.", window, cx)
    });
    dialog.update_in(cx, |d, window, cx| {
        d.review(Verdict::RequestChanges, window, cx)
    });
    wait_until(cx, "review sent", |_| {
        !lines(&gh.join("fake-gh.actions")).is_empty()
    });
    cx.run_until_parked();
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [format!("pr review {URL} --request-changes --body-file -")]
    );
    assert_eq!(
        std::fs::read_to_string(gh.join("fake-gh.review-body")).expect("body"),
        "Please add a test."
    );
    assert!(!dialog_open(cx));

    // gh refuses the merge: its words show in the dialog, which stays open.
    std::fs::write(
        gh.join("fake-action-fail"),
        "Required status check \"build\" is failing.",
    )
    .expect("fail");
    let dialog = r.ws.update_in(cx, |ws, window, cx| {
        let view = ws.current_thread().expect("shown");
        view.update(cx, |v, cx| v.open_pr_actions(window, cx))
            .expect("thread has a PR")
    });
    wait_until(cx, "methods", |cx| {
        dialog.read_with(cx, |d, _| d.methods.is_some())
    });
    for _ in 0..2 {
        dialog.update_in(cx, |d, window, cx| d.press_merge(window, cx));
    }
    wait_until(cx, "merge refused", |cx| {
        dialog.read_with(cx, |d, _| d.error.is_some())
    });
    let (error, busy) = dialog.read_with(cx, |d, _| (d.error.clone(), d.busy));
    assert!(
        error
            .as_ref()
            .is_some_and(|e| e.contains("Required status check")),
        "{error:?}"
    );
    assert!(!busy);
    assert!(dialog_open(cx));
}

#[gpui::test]
fn a_folder_bound_to_another_account_is_refused_before_gh_runs(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let (_, dialog) = thread_dialog(&r, cx);
    // The folder loses its binding to the thread's account after the dialog opened.
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.remove_binding(&r.f.project_dir).expect("unbind");
    config.save(&r.f.accounts_path).expect("save");
    dialog.update_in(cx, |d, window, cx| d.review(Verdict::Approve, window, cx));
    wait_until(cx, "refused", |cx| {
        dialog.read_with(cx, |d, _| d.error.is_some())
    });
    let error = dialog.read_with(cx, |d, _| d.error.clone()).expect("error");
    assert!(error.contains("isn't bound"), "{error}");
    assert!(lines(&gh.join("fake-gh.actions")).is_empty());
    assert!(dialog_open(cx));
}

#[gpui::test]
fn a_pull_requests_page_row_acts_with_its_groups_account_and_lists_again(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    r.ws.update_in(cx, |ws, window, cx| {
        ws.open_pull_requests(&super::super::gh_pages::OpenPullRequests, window, cx)
    });
    let page =
        r.ws.read_with(cx, |ws, _| ws.gh_pages.prs.clone())
            .expect("page");
    let listed = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| {
            p.shown()
                .iter()
                .any(|(_, g)| matches!(&g.listing, Listing::Prs(l) if !l.is_empty()))
        })
    };
    wait_until(cx, "listed", listed);
    assert_eq!(lines(&gh.join("fake-gh.list")).len(), 1);
    let project = r.project;
    let dialog = page
        .update_in(cx, |p, window, cx| {
            p.open_pr_actions(project, 12, window, cx)
        })
        .expect("row 12 is listed");
    assert_eq!(page.read_with(cx, |p, _| p.kind), Kind::Prs);
    wait_until(cx, "methods", |cx| {
        dialog.read_with(cx, |d, _| d.methods.is_some())
    });
    dialog.update_in(cx, |d, window, cx| d.review(Verdict::Approve, window, cx));
    wait_until(cx, "listed again", |_| {
        lines(&gh.join("fake-gh.list")).len() == 2
    });
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        ["pr review https://github.com/acme/demo/pull/12 --approve --body-file -"]
    );
    assert!(!dialog_open(cx));
}
