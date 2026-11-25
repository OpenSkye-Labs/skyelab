//! Acceptance: an Issues page row's dialog comments, closes (with its reason and comment) and reopens with the
//! folder's bound account's gh (fake gh only), shows gh's errors, refuses after a rebind, and re-reads only that row.

use super::super::gh_pages::{GhPage, Listing};
use super::super::manage_harness::{Rig, rig};
use super::super::pr_actions::tests::{github, lines};
use super::super::threads_test::wait_until;
use super::*;
use gpui::VisualTestContext;
use std::path::Path;

const URL: &str = "https://github.com/acme/demo/issues/31";

fn issues_page(r: &Rig, cx: &mut VisualTestContext) -> Entity<GhPage> {
    r.ws.update_in(cx, |ws, window, cx| {
        ws.open_issues(&super::super::gh_pages::OpenIssues, window, cx)
    });
    let page =
        r.ws.read_with(cx, |ws, _| ws.gh_pages.issues.clone())
            .expect("page");
    wait_until(cx, "listed", |cx| row_closed(&page, cx).is_some());
    page
}

/// Issue 31's `closed` in the shown list, once it is listed.
fn row_closed(page: &Entity<GhPage>, cx: &mut VisualTestContext) -> Option<bool> {
    page.read_with(cx, |p, _| {
        p.shown().iter().find_map(|(_, g)| match &g.listing {
            Listing::Issues(l) => l.iter().find(|r| r.row.url == URL).map(|r| r.row.closed),
            _ => None,
        })
    })
}

fn dialog(r: &Rig, page: &Entity<GhPage>, cx: &mut VisualTestContext) -> Entity<IssueActions> {
    let project = r.project;
    page.update_in(cx, |p, window, cx| {
        p.open_issue_actions(project, 31, window, cx)
    })
    .expect("row 31 is listed")
}

fn type_body(d: &Entity<IssueActions>, text: &str, cx: &mut VisualTestContext) {
    d.update_in(cx, |d, window, cx| {
        d.body.update(cx, |b, cx| b.set_value(text, window, cx))
    });
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

#[gpui::test]
fn close_not_planned_with_a_comment_rereads_the_row_and_reopen_brings_it_back(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let page = issues_page(&r, cx);
    assert_eq!(row_closed(&page, cx), Some(false));

    let d = dialog(&r, &page, cx);
    // Comment needs text: nothing reaches gh.
    d.update_in(cx, |d, window, cx| d.act(Act::Comment, window, cx));
    assert!(lines(&gh.join("fake-gh.actions")).is_empty());
    type_body(&d, "Won't fix: by design.", cx);
    d.update_in(cx, |d, window, cx| {
        d.reason = CloseReason::NotPlanned;
        d.act(Act::Close(CloseReason::NotPlanned), window, cx)
    });
    wait_until(cx, "row closed", |cx| row_closed(&page, cx) == Some(true));
    assert!(!dialog_open(cx));
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [
            format!("issue comment {URL} --body-file -"),
            format!("issue close {URL} --reason not planned"),
        ]
    );
    assert_eq!(
        std::fs::read_to_string(gh.join("fake-gh.review-body")).expect("body"),
        "Won't fix: by design."
    );
    // Only that issue was read again, not the list.
    assert_eq!(lines(&gh.join("fake-gh.list")).len(), 1);
    assert_eq!(lines(&gh.join("fake-gh.issueview")).len(), 1);

    let d = dialog(&r, &page, cx);
    assert!(d.read_with(cx, |d, _| d.target.closed));
    d.update_in(cx, |d, window, cx| d.act(Act::Reopen, window, cx));
    wait_until(cx, "row open", |cx| row_closed(&page, cx) == Some(false));
    assert_eq!(
        lines(&gh.join("fake-gh.actions"))
            .last()
            .map(String::as_str),
        Some(format!("issue reopen {URL}").as_str())
    );
}

#[gpui::test]
fn gh_errors_stay_in_the_dialog_and_a_rebind_is_refused_before_gh(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let page = issues_page(&r, cx);
    let d = dialog(&r, &page, cx);
    std::fs::write(gh.join("fake-action-fail"), "HTTP 403: no triage access").expect("write");
    type_body(&d, "hello", cx);
    d.update_in(cx, |d, window, cx| d.act(Act::Comment, window, cx));
    wait_until(cx, "error", |cx| d.read_with(cx, |d, _| d.error.is_some()));
    let error = d.read_with(cx, |d, _| d.error.clone()).expect("error");
    assert!(error.contains("no triage access"), "{error}");
    assert!(dialog_open(cx));
    assert!(lines(&gh.join("fake-gh.issueview")).is_empty());

    std::fs::remove_file(gh.join("fake-action-fail")).expect("rm");
    let before = lines(&gh.join("fake-gh.actions")).len();
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.remove_binding(&r.f.project_dir).expect("unbind");
    config.save(&r.f.accounts_path).expect("save");
    d.update_in(cx, |d, window, cx| d.act(Act::Comment, window, cx));
    wait_until(cx, "refused", |cx| {
        d.read_with(cx, |d, _| {
            !d.busy && d.error.as_ref().is_some_and(|e| e.contains("isn't bound"))
        })
    });
    assert_eq!(lines(&gh.join("fake-gh.actions")).len(), before);
}

#[gpui::test]
fn a_close_that_fails_after_its_comment_clears_the_box_so_a_retry_posts_it_once(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    // A gh that refuses `issue close` while `close-fails` exists, and is the fake gh otherwise.
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    let wrapper = gh.join("gh-close-fails.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n[ \"$2\" = close ] && [ -f \"$GH_CONFIG_DIR/close-fails\" ] && {{ echo 'closing refused' >&2; exit 1; }}\nexec '{}' \"$@\"\n",
            fake.display()
        ),
    )
    .expect("write");
    let ok = std::process::Command::new("chmod")
        .arg("+x")
        .arg(&wrapper)
        .status()
        .expect("chmod")
        .success();
    assert!(ok);
    std::fs::write(gh.join("close-fails"), "").expect("write");
    r.ws.update(cx, |_, cx| cx.set_global(super::super::GhBin(wrapper)));
    let page = issues_page(&r, cx);
    let d = dialog(&r, &page, cx);
    type_body(&d, "  indented\n", cx);
    d.update_in(cx, |d, window, cx| {
        d.act(Act::Close(CloseReason::Completed), window, cx)
    });
    wait_until(cx, "error", |cx| d.read_with(cx, |d, _| d.error.is_some()));
    let error = d.read_with(cx, |d, _| d.error.clone()).expect("error");
    assert!(
        error.contains("comment was posted") && error.contains("closing refused"),
        "{error}"
    );
    assert!(d.read_with(cx, |d, cx| d.body.read(cx).value().is_empty()));
    // Sent as typed.
    assert_eq!(
        std::fs::read_to_string(gh.join("fake-gh.review-body")).expect("body"),
        "  indented\n"
    );

    std::fs::remove_file(gh.join("close-fails")).expect("rm");
    d.update_in(cx, |d, window, cx| {
        d.act(Act::Close(CloseReason::Completed), window, cx)
    });
    wait_until(cx, "row closed", |cx| row_closed(&page, cx) == Some(true));
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [
            format!("issue comment {URL} --body-file -"),
            format!("issue close {URL} --reason completed"),
        ]
    );
}
