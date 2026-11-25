//! Acceptance: "View diff" in the PR actions dialog opens the PR's diff in a Changes tab, read with the bound account's
//! gh (fake gh only); comments on its lines go to GitHub as one review through the dialog, then leave the tab.

use super::super::changes::ChangesView;
use super::super::changes_model::Row;
use super::super::manage_harness::rig;
use super::super::threads_test::wait_until;
use super::tests::{URL, dialog_open, github, lines, thread_dialog};
use super::*;
use gpui::VisualTestContext;

const HEAD: &str = "1111111111111111111111111111111111111111";

fn tab(
    r: &super::super::manage_harness::Rig,
    cx: &mut VisualTestContext,
) -> Option<Entity<ChangesView>> {
    r.ws.read_with(cx, |ws, _| ws.pr_diffs.iter().find_map(|t| t.upgrade()))
}

#[gpui::test]
fn view_diff_opens_the_prs_diff_and_line_comments_go_as_one_review(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let gh = github(&r, cx);
    let (_, dialog) = thread_dialog(&r, cx);
    dialog.update_in(cx, |d, window, cx| d.view_diff(window, cx));
    cx.run_until_parked();
    assert!(!dialog_open(cx), "View diff closes the dialog");
    let tab = tab(&r, cx).expect("a PR diff tab");
    wait_until(cx, "the diff read", |cx| {
        tab.read_with(cx, |t, _| t.changes.is_some() || t.error.is_some())
    });
    tab.read_with(cx, |t, _| {
        assert_eq!(t.error, None);
        let files: Vec<_> = t
            .changes
            .as_ref()
            .expect("read")
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect();
        assert_eq!(files, [PathBuf::from("src/lib.rs")]);
        assert_eq!(t.title().as_ref(), "PR #42 · acme/demo");
        assert_eq!(
            t.pr.as_ref().and_then(|p| p.target.head.as_deref()),
            Some(HEAD)
        );
    });
    // Read in the account's gh dir (the fake logs there), never through the folder's git.
    assert_eq!(
        lines(&gh.join("fake-gh.prdiff")),
        [format!("pr diff {URL} --color never")]
    );
    assert!(
        r.ws.read_with(cx, |ws, _| ws
            .changes_views
            .iter()
            .all(|c| c.upgrade().is_none_or(|c| c != tab))),
        "a PR tab isn't one of the folder's Changes tabs"
    );

    // Comment on `-    41` and `+    // The answer.`: from the old line 2 to the new line 2.
    let (from, to) = tab.read_with(cx, |t, _| {
        let rows = &t.changes.as_ref().expect("read").rows;
        let at = |text: &str| {
            rows.iter()
                .position(|r| matches!(r, Row::Line { text: t, .. } if t == text))
                .expect("row")
        };
        (at("    41"), at("    // The answer."))
    });
    tab.update_in(cx, |t, window, cx| {
        t.open_draft(from, to, window, cx);
        let input = t.draft.as_ref().expect("draft").input.clone();
        input.update(cx, |i, cx| i.set_value("Why not a constant?", window, cx));
        t.save_draft(window, cx);
    });
    assert_eq!(
        tab.read_with(cx, |t, _| t.pr.as_ref().map(|p| p.review.items.len())),
        Some(1)
    );

    // Submit review…: the dialog carries the comment; Comment with no summary posts one review.
    let review = tab
        .update_in(cx, |t, window, cx| t.submit_review(window, cx))
        .expect("a dialog");
    assert!(dialog_open(cx));
    wait_until(cx, "merge methods read", |cx| {
        review.read_with(cx, |d, _| d.methods.is_some())
    });
    assert_eq!(review.read_with(cx, |d, _| d.comments.len()), 1);
    review.update_in(cx, |d, window, cx| d.review(Verdict::Comment, window, cx));
    wait_until(cx, "review posted", |cx| {
        !dialog_open(cx) && !lines(&gh.join("fake-gh.actions")).is_empty()
    });
    assert_eq!(
        lines(&gh.join("fake-gh.actions")),
        ["api --method POST repos/acme/demo/pulls/42/reviews --input -"]
    );
    let sent: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(gh.join("fake-gh.review-body")).expect("body"),
    )
    .expect("json");
    assert_eq!(
        sent,
        serde_json::json!({
            "commit_id": HEAD,
            "event": "COMMENT",
            "body": "",
            "comments": [{
                "path": "src/lib.rs", "line": 2, "side": "RIGHT", "start_line": 2, "start_side": "LEFT",
                "body": "Why not a constant?"
            }]
        })
    );
    wait_until(cx, "pending comments cleared", |cx| {
        tab.read_with(cx, |t, _| {
            t.pr.as_ref().is_some_and(|p| p.review.items.is_empty())
        })
    });
}
