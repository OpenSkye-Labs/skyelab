//! Acceptance: a stacked PR shows its "Stack 2/2" badge in the PR actions dialog, the details card's cache and the
//! Pull requests page, and Merge stack merges bottom first with the folder's bound account (fake gh only): two clicks,
//! a third while a layer's checks fail, each layer pinned to its head, stopping at a failure, refused after a rebind.

use super::super::manage_harness::{Rig, rig};
use super::super::pr_actions::tests::{
    URL, dialog_open, github, lines, listed_page, row_state, thread_dialog,
};
use super::super::threads_test::wait_until;
use super::*;
use gpui::{Entity, VisualTestContext};
use skyelab_core::AccountsConfig;
use skyelab_git::PrState;

const BELOW: &str = "https://github.com/acme/demo/pull/41";
/// #42 moved onto main once #41 merged.
const EDIT: &str = "pr edit https://github.com/acme/demo/pull/42 --base main";

fn head(n: u64) -> String {
    format!("{n:0>40}")
}

/// #41 (`a` on main) under #42 (`b` on `a`, the rig's thread PR), each with its own head; merges mark one PR merged.
fn stacked(r: &Rig, below_checks: &str, cx: &mut VisualTestContext) -> PathBuf {
    let gh = github(r, cx);
    let row = |n: u64, head_ref: &str, base: &str, conclusion: &str| {
        format!(
            r#"{{"number":{n},"title":"PR {n}","url":"https://github.com/acme/demo/pull/{n}","author":{{"login":"ada"}},"isDraft":false,"reviewDecision":"","headRefOid":"{}","headRefName":"{head_ref}","baseRefName":"{base}","isCrossRepository":false,"statusCheckRollup":[{{"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"{conclusion}"}}],"updatedAt":"2026-11-23T09:00:00Z"}}"#,
            head(n)
        )
    };
    let list = format!(
        "[{},{}]",
        row(42, "b", "a", "SUCCESS"),
        row(41, "a", "main", below_checks)
    );
    std::fs::write(gh.join("fake-prs.json"), list).expect("list");
    std::fs::write(gh.join("fake-stack-mode"), "").expect("mode");
    for n in [41, 42] {
        std::fs::write(gh.join(format!("fake-head-{n}")), head(n)).expect("head");
    }
    gh
}

fn merge_line(n: u64, delete: bool) -> String {
    let tail = if delete { " --delete-branch" } else { "" };
    format!(
        "pr merge https://github.com/acme/demo/pull/{n} --repo github.com/acme/demo --match-head-commit {} --squash{tail}",
        head(n)
    )
}

fn press(dialog: &Entity<PrActions>, cx: &mut VisualTestContext) {
    dialog.update_in(cx, |d, window, cx| d.press_merge_stack(window, cx));
    cx.run_until_parked();
}

fn stage(dialog: &Entity<PrActions>, cx: &mut VisualTestContext) -> Stage {
    dialog.read_with(cx, |d, _| d.stack.stage)
}

#[gpui::test]
fn pr_stack_merge_from_a_thread_takes_two_clicks_and_merges_bottom_first(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = stacked(&r, "SUCCESS", cx);
    let (_, dialog) = thread_dialog(&r, cx);
    let badge = dialog.read_with(cx, |d, _| d.stack.stack.as_ref().map(PrStack::badge));
    assert_eq!(badge.as_deref(), Some("Stack 2/2"));
    let cached = cx.update(|_, cx| PrStacks::get(cx, URL).map(PrStack::badge));
    assert_eq!(
        cached.as_deref(),
        Some("Stack 2/2"),
        "the details card's badge"
    );
    assert_eq!(
        cx.update(|_, cx| PrStacks::get(cx, BELOW).map(PrStack::badge))
            .as_deref(),
        Some("Stack 1/2")
    );
    press(&dialog, cx);
    assert_eq!(stage(&dialog, cx), Stage::Confirm);
    assert!(
        lines(&gh.join("fake-gh.actions")).is_empty(),
        "not merged yet"
    );
    press(&dialog, cx);
    wait_until(cx, "the dialog closes", |cx| !dialog_open(cx));
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [
            merge_line(41, false),
            EDIT.to_owned(),
            merge_line(42, false)
        ]
    );
    let toasts = cx.update(|window, cx| crate::ui::toast::shown(window, cx));
    assert!(
        toasts
            .iter()
            .any(|t| t == "Merged 2 of 2 stacked pull requests"),
        "{toasts:?}"
    );
}

#[gpui::test]
fn pr_stack_a_failing_layer_takes_a_third_click(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = stacked(&r, "FAILURE", cx);
    let (_, dialog) = thread_dialog(&r, cx);
    dialog.update(cx, |d, cx| {
        d.delete_branch = true;
        cx.notify();
    });
    press(&dialog, cx);
    press(&dialog, cx);
    assert_eq!(stage(&dialog, cx), Stage::ConfirmFailing);
    assert!(
        lines(&gh.join("fake-gh.actions")).is_empty(),
        "not merged yet"
    );
    press(&dialog, cx);
    wait_until(cx, "the dialog closes", |cx| !dialog_open(cx));
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        [merge_line(41, true), EDIT.to_owned(), merge_line(42, true)]
    );
}

#[gpui::test]
fn pr_stack_stops_at_a_failed_layer_and_keeps_the_dialog_open(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = stacked(&r, "SUCCESS", cx);
    std::fs::write(
        gh.join("fake-action-fail-42"),
        "Pull request is not mergeable",
    )
    .expect("fail");
    let (_, dialog) = thread_dialog(&r, cx);
    press(&dialog, cx);
    press(&dialog, cx);
    wait_until(cx, "the error", |cx| {
        dialog.read_with(cx, |d, _| d.error.is_some())
    });
    let error = dialog.read_with(cx, |d, _| d.error.clone()).expect("error");
    assert!(
        error.starts_with("merged #41; #42 didn't merge:"),
        "{error}"
    );
    assert!(error.contains("not mergeable"), "{error}");
    assert!(dialog_open(cx));
    assert!(
        dialog.read_with(cx, |d, _| d.stack.stack.is_none()),
        "no stale stack offered again"
    );
    wait_until(cx, "the repo's stacks are forgotten", |cx| {
        cx.update(|_, cx| PrStacks::get(cx, BELOW).is_none() && PrStacks::get(cx, URL).is_none())
    });
    // Reopened, the dialog lists the repo again rather than offering the stack it had.
    let listed = lines(&gh.join("fake-gh.list")).len();
    let (_, again) = thread_dialog(&r, cx);
    assert_eq!(lines(&gh.join("fake-gh.list")).len(), listed + 1);
    drop(again);
}

#[gpui::test]
fn pr_stack_a_rebound_folder_is_refused_before_gh_runs(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = stacked(&r, "SUCCESS", cx);
    let (_, dialog) = thread_dialog(&r, cx);
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.remove_binding(&r.f.project_dir).expect("unbind");
    config.save(&r.f.accounts_path).expect("save");
    press(&dialog, cx);
    press(&dialog, cx);
    wait_until(cx, "refused", |cx| {
        dialog.read_with(cx, |d, _| d.error.is_some())
    });
    let error = dialog.read_with(cx, |d, _| d.error.clone()).expect("error");
    assert!(error.contains("isn't bound"), "{error}");
    assert!(lines(&gh.join("fake-gh.actions")).is_empty());
}

#[gpui::test]
fn pr_stack_page_rows_carry_badges_and_a_stack_merge_reads_every_layer_again(
    cx: &mut gpui::TestAppContext,
) {
    let (r, cx) = rig(cx);
    let gh = stacked(&r, "SUCCESS", cx);
    let page = listed_page(&r, &gh, cx);
    let badges = cx.update(|_, cx| [URL, BELOW].map(|u| PrStacks::get(cx, u).map(PrStack::badge)));
    assert_eq!(badges, [Some("Stack 2/2".into()), Some("Stack 1/2".into())]);
    let project = r.project;
    let dialog = page
        .update_in(cx, |p, window, cx| {
            p.open_pr_actions(project, 42, window, cx)
        })
        .expect("row 42 is listed");
    wait_until(cx, "methods", |cx| {
        dialog.read_with(cx, |d, _| d.methods.is_some())
    });
    assert_eq!(
        lines(&gh.join("fake-gh.list")).len(),
        1,
        "the page's list decides the stack"
    );
    press(&dialog, cx);
    press(&dialog, cx);
    wait_until(cx, "both rows read merged", |cx| {
        row_state(&page, 41, cx) == Some(PrState::Merged)
            && row_state(&page, 42, cx) == Some(PrState::Merged)
    });
    assert!(
        cx.update(|_, cx| PrStacks::get(cx, URL).is_none()),
        "merged: no stack left"
    );
}
