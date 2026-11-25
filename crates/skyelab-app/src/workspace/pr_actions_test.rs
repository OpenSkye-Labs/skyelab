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

pub(super) const URL: &str = "https://github.com/acme/demo/pull/42";

/// The rig's project as a github.com repo, its account (acme) logged in to a fake gh dir, accounts.toml saved.
pub(in crate::workspace) fn github(r: &Rig, cx: &mut VisualTestContext) -> PathBuf {
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

pub(in crate::workspace) fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn status(id: i64, cx: &mut VisualTestContext) -> Option<PrView> {
    cx.update(|_, cx| PrStatuses::view(cx, id, URL).cloned())
}

/// A thread with `URL` linked, read once, opened; its PR actions dialog opened from the details card.
pub(super) fn thread_dialog(r: &Rig, cx: &mut VisualTestContext) -> (i64, Entity<PrActions>) {
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

pub(super) fn dialog_open(cx: &mut VisualTestContext) -> bool {
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
        r#"{"state":"OPEN","isDraft":false,"headRefOid":"1111111111111111111111111111111111111111","statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE"}]}"#,
    )
    .expect("pr");
    let (id, dialog) = thread_dialog(&r, cx);
    assert!(dialog_open(cx));
    assert_eq!(
        dialog.read_with(cx, |d, _| d.target.checks),
        Some(Checks::Failing)
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
        [format!(
            "pr merge {URL} --repo github.com/acme/demo --match-head-commit 1111111111111111111111111111111111111111 --squash --delete-branch"
        )]
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
    // fake-gh logs the call before it reads the body, and the dialog closes once gh has exited.
    wait_until(cx, "review sent and the dialog closed", |cx| {
        !dialog_open(cx) && !lines(&gh.join("fake-gh.actions")).is_empty()
    });
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [format!("pr review {URL} --request-changes --body-file -")]
    );
    assert_eq!(
        std::fs::read_to_string(gh.join("fake-gh.review-body")).expect("body"),
        "Please add a test."
    );

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
    // After the dialog opened, the folder is rebound to another account (with its own GitHub login), then unbound.
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    let beta_gh = root.join("gh-beta");
    std::fs::create_dir_all(&beta_gh).expect("mkdir");
    std::fs::write(beta_gh.join("fake-login"), "other").expect("login");
    std::fs::write(beta_gh.join("hosts.yml"), "github.com:\n").expect("hosts");
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config
        .add_account(skyelab_core::Account {
            id: "beta".into(),
            label: "Beta".into(),
            config_dir: root.join(".claude-beta"),
            provider: Default::default(),
            gh_config_dir: Some(beta_gh.clone()),
            ssh_agent: false,
        })
        .expect("account");
    config.remove_binding(&r.f.project_dir).expect("unbind");
    config
        .add_binding(skyelab_core::Binding::new(
            r.f.project_dir.clone(),
            "beta".into(),
        ))
        .expect("rebind");
    config.save(&r.f.accounts_path).expect("save");
    let refused = |cx: &mut VisualTestContext| {
        dialog.update(cx, |d, cx| {
            d.error = None;
            cx.notify();
        });
        dialog.update_in(cx, |d, window, cx| d.review(Verdict::Approve, window, cx));
        wait_until(cx, "refused", |cx| {
            dialog.read_with(cx, |d, _| d.error.is_some())
        });
        dialog.read_with(cx, |d, _| d.error.clone()).expect("error")
    };
    let error = refused(cx);
    assert!(
        error.contains("is bound to Beta, not this thread's account"),
        "{error}"
    );
    assert!(
        lines(&beta_gh.join("fake-gh.actions")).is_empty(),
        "never the other account's gh"
    );

    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.remove_binding(&r.f.project_dir).expect("unbind");
    config.save(&r.f.accounts_path).expect("save");
    let error = refused(cx);
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

#[gpui::test]
fn unread_checks_ask_once_more_and_a_cancelled_merge_still_reads_the_pr_again(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let (id, dialog) = thread_dialog(&r, cx);
    // As if the PR was never read this session.
    dialog.update(cx, |d, _| d.target.checks = None);
    for _ in 0..2 {
        dialog.update_in(cx, |d, window, cx| d.press_merge(window, cx));
    }
    assert_eq!(dialog.read_with(cx, |d, _| d.stage), Stage::ConfirmFailing);
    // The last click, then Cancel before gh answers: the merge happened, so the PR is read again anyway.
    dialog.update_in(cx, |d, window, cx| {
        d.press_merge(window, cx);
        window.close_dialog(cx);
    });
    drop(dialog);
    wait_until(
        cx,
        "merged read again",
        |cx| matches!(status(id, cx), Some(PrView::Status(s)) if s.state == PrState::Merged),
    );
    assert_eq!(lines(&gh.join("fake-gh.actions")).len(), 1);
}

#[gpui::test]
fn a_read_that_started_before_the_stored_one_landed_is_dropped(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let (id, _dialog) = thread_dialog(&r, cx);
    let views = || lines(&gh.join("fake-gh.prview")).len();
    let before = views();
    // A newer read already landed (stamped later than this one starts); gh now says merged.
    cx.update(|_, cx| {
        super::super::pr_watch::PrLanded::get(cx).mark(id, URL, crate::ui::now_ms() + 60_000)
    });
    std::fs::write(gh.join("fake-merged"), "").expect("merged");
    r.ws.update(cx, |ws, cx| ws.reread_pr(id, cx));
    wait_until(cx, "read ran", |_| views() == before + 1);
    cx.run_until_parked();
    assert!(
        matches!(status(id, cx), Some(PrView::Status(s)) if s.state == PrState::Open),
        "the older read must not replace the newer one"
    );
}

#[gpui::test]
fn a_merge_without_a_known_head_is_refused_and_a_moved_branch_shows_ghs_refusal(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let (id, dialog) = thread_dialog(&r, cx);
    let merge = |cx: &mut VisualTestContext| {
        for _ in 0..2 {
            dialog.update_in(cx, |d, window, cx| d.press_merge(window, cx));
        }
        cx.run_until_parked();
    };
    // The read gave no head: nothing runs, and the dialog asks for a refresh.
    let head = dialog.update(cx, |d, _| d.target.head.take());
    assert_eq!(
        head.as_deref(),
        Some("1111111111111111111111111111111111111111")
    );
    merge(cx);
    let error = dialog
        .read_with(cx, |d, _| d.error.clone())
        .expect("refused");
    assert!(error.contains("Refresh it, then merge"), "{error}");
    assert!(lines(&gh.join("fake-gh.actions")).is_empty());

    // The head from the read, but a commit was pushed since: GitHub's refusal shows, nothing merges.
    dialog.update(cx, |d, _| d.target.head = head);
    std::fs::write(
        gh.join("fake-head"),
        "2222222222222222222222222222222222222222",
    )
    .expect("push");
    merge(cx);
    wait_until(cx, "refused", |cx| {
        dialog.read_with(cx, |d, _| {
            d.error
                .as_ref()
                .is_some_and(|e| e.contains("Head branch was modified"))
        })
    });
    assert!(dialog_open(cx));
    assert!(matches!(status(id, cx), Some(PrView::Status(s)) if s.state == PrState::Open));
}

/// A read is stamped with when it started: a poll that began before a merge and lands after the merge's re-read
/// started must not make that fresher re-read look older and drop it.
#[gpui::test]
fn a_read_is_stamped_when_it_started_not_when_it_landed(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let (id, _dialog) = thread_dialog(&r, cx);
    let landed = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<PrStatuses>().0.get(&id).map(|e| e.checked_ms))
    };
    let first = landed(cx);
    // The read takes a second, so its start and its landing are far apart on the clock.
    std::fs::write(gh.join("fake-view-delay"), "1").expect("delay");
    r.ws.update(cx, |ws, cx| ws.reread_pr(id, cx));
    wait_until(cx, "read landed", |cx| landed(cx) != first);
    let seen = crate::ui::now_ms();
    let stamp = landed(cx).expect("stamped");
    assert!(
        stamp + 500 <= seen,
        "STAMPED_ON_LANDING: {stamp} is within 500 ms of the read landing ({seen})"
    );
}
