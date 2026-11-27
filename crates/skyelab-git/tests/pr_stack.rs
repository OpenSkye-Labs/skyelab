//! Stacked pull requests: chains found from a repo's open PR list (cycles, forks and ties included), and Merge stack
//! with the account's gh (the fake gh only): bottom first, each layer pinned to its head, stopping at a failure.

use skyelab_git::{Checks, MergeMethod, PrRow, PrStack, PrState, Review, stack_for};

fn pr(number: u64, head_ref: &str, base_ref: &str) -> PrRow {
    PrRow {
        number,
        title: format!("PR {number}"),
        url: format!("https://github.com/acme/demo/pull/{number}"),
        author: "octo".into(),
        draft: false,
        checks: Checks::Passing,
        review: Review::None,
        updated_at: String::new(),
        head: Some(format!("{number:0>40}")),
        head_ref: head_ref.into(),
        base_ref: base_ref.into(),
        state: PrState::Open,
    }
}

fn numbers(stack: &PrStack) -> Vec<u64> {
    stack.layers.iter().map(|l| l.number).collect()
}

/// main <- a (#10) <- b (#11) <- c (#12), listed out of order, plus #20 on its own.
fn chain() -> Vec<PrRow> {
    vec![
        pr(12, "c", "b"),
        pr(20, "solo", "main"),
        pr(10, "a", "main"),
        pr(11, "b", "a"),
    ]
}

#[test]
fn a_chain_reads_bottom_up_with_its_place() {
    let prs = chain();
    let middle = stack_for(11, &prs).expect("stacked");
    assert_eq!(numbers(&middle), [10, 11]);
    assert_eq!((middle.position, middle.total), (2, 3));
    assert_eq!(middle.badge(), "Stack 2/3");
    assert_eq!(middle.onto(), "main");
    let top = stack_for(12, &prs).expect("stacked");
    assert_eq!(numbers(&top), [10, 11, 12]);
    assert_eq!(top.badge(), "Stack 3/3");
    assert_eq!(stack_for(10, &prs).expect("stacked").badge(), "Stack 1/3");
    assert_eq!(stack_for(20, &prs), None, "a lone PR is no stack");
    assert_eq!(stack_for(99, &prs), None, "not listed");
}

#[test]
fn merged_layers_and_fork_branches_are_not_part_of_a_stack() {
    let mut prs = chain();
    prs[2].state = PrState::Merged; // #10
    let s = stack_for(12, &prs).expect("stacked");
    assert_eq!(numbers(&s), [11, 12], "the open part only");
    assert_eq!(s.onto(), "a");
    assert_eq!(stack_for(10, &prs), None);
    // A fork's PR lists no head branch, so a PR on `main` is never stacked on a fork's `main`.
    let prs = vec![pr(5, "", "main"), pr(6, "x", "main")];
    assert_eq!(stack_for(6, &prs), None);
}

#[test]
fn branches_take_the_longest_chain_and_ties_the_lowest_number() {
    // #10 has #11 and #12 on it; #13 stands on #12. Two PRs share head `b`: the lower (#11) is the one below #14.
    let prs = vec![
        pr(10, "a", "main"),
        pr(11, "b", "a"),
        pr(12, "c", "a"),
        pr(13, "d", "c"),
        pr(16, "b", "a"),
        pr(14, "e", "b"),
    ];
    assert_eq!(stack_for(10, &prs).expect("stacked").badge(), "Stack 1/3");
    assert_eq!(stack_for(11, &prs).expect("stacked").badge(), "Stack 2/3");
    assert_eq!(
        numbers(&stack_for(14, &prs).expect("stacked")),
        [10, 11, 14]
    );
    assert_eq!(
        numbers(&stack_for(13, &prs).expect("stacked")),
        [10, 12, 13]
    );
    let mut shuffled = prs.clone();
    shuffled.reverse();
    for n in [10, 11, 12, 13, 14, 16] {
        assert_eq!(
            stack_for(n, &prs),
            stack_for(n, &shuffled),
            "#{n}: order-free"
        );
    }
}

#[test]
fn a_cycle_or_a_self_loop_is_no_stack() {
    let prs = vec![pr(1, "a", "b"), pr(2, "b", "a"), pr(3, "c", "a")];
    assert_eq!(stack_for(1, &prs), None);
    assert_eq!(stack_for(2, &prs), None);
    assert_eq!(stack_for(3, &prs), None, "standing on a loop has no bottom");
    assert_eq!(stack_for(4, &[pr(4, "x", "x")]), None);
}

#[test]
fn the_list_reads_branches_and_drops_a_fork_head() {
    let json = br#"[
        {"number":1,"url":"https://github.com/acme/demo/pull/1","headRefName":"a","baseRefName":"main","isCrossRepository":false},
        {"number":2,"url":"https://github.com/acme/demo/pull/2","headRefName":"main","baseRefName":"main","isCrossRepository":true},
        {"number":3,"url":"https://github.com/acme/demo/pull/3","headRefName":"b","baseRefName":"a"}
    ]"#;
    let rows = skyelab_git::parse_pr_list(json).expect("parses");
    let refs: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.head_ref.as_str(), r.base_ref.as_str()))
        .collect();
    // No isCrossRepository reads as a fork's: a head branch is trusted only when gh says it is the repo's own.
    assert_eq!(refs, [("a", "main"), ("", "main"), ("", "a")]);
    assert!(
        skyelab_git::pr_list_args("acme/demo", skyelab_git::PrFilter::All)
            .iter()
            .any(|a| a.contains("headRefName,baseRefName,isCrossRepository"))
    );
}

#[cfg(unix)] // the fake gh is a shell script
mod merge {
    use super::*;
    use skyelab_git::GitHub;
    use std::path::{Path, PathBuf};

    fn fake_gh() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
    }

    /// A logged-in fake gh dir where each PR's head is its own (`fake-head-N`) and merges mark one PR merged.
    fn logged_in(prs: &[PrRow]) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
        std::fs::write(dir.join("fake-login"), "octo").expect("login");
        std::fs::write(dir.join("fake-stack-mode"), "").expect("mode");
        for p in prs {
            let head = p.head.as_deref().expect("head");
            std::fs::write(dir.join(format!("fake-head-{}", p.number)), head).expect("head");
        }
        (tmp, dir)
    }

    fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .map(|t| t.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn merge_line(n: u64, delete: bool) -> String {
        let head = format!("{n:0>40}");
        let tail = if delete { " --delete-branch" } else { "" };
        format!(
            "pr merge https://github.com/acme/demo/pull/{n} --repo github.com/acme/demo --match-head-commit {head} --squash{tail}"
        )
    }

    fn edit_line(n: u64) -> String {
        format!("pr edit https://github.com/acme/demo/pull/{n} --base main")
    }

    fn gh_in(dir: &Path, bin: &Path, f: impl FnOnce(&GitHub)) {
        f(&GitHub {
            gh: bin,
            config_dir: dir,
        })
    }

    #[test]
    fn merges_bottom_first_pinned_to_each_head_moving_each_layer_onto_the_base() {
        let prs = chain();
        let (_tmp, dir) = logged_in(&prs);
        let stack = stack_for(12, &prs).expect("stacked");
        gh_in(&dir, &fake_gh(), |gh| {
            let merged =
                skyelab_git::merge_stack(gh, &stack, MergeMethod::Squash, false).expect("merges");
            assert_eq!(merged, [10, 11, 12]);
        });
        // No branch is deleted unasked: each layer above is moved onto main itself, right before its merge.
        assert_eq!(
            lines(&dir.join("fake-gh.actions")),
            [
                merge_line(10, false),
                edit_line(11),
                merge_line(11, false),
                edit_line(12),
                merge_line(12, false)
            ]
        );
        let views = lines(&dir.join("fake-gh.prview"));
        assert_eq!(
            views.len(),
            1,
            "the bottom's live base is checked: {views:?}"
        );
        assert!(
            views[0].contains("/pull/10 --json baseRefName"),
            "{views:?}"
        );
        let env = std::fs::read_to_string(dir.join("fake-gh.fullenv")).expect("env");
        assert!(env.contains(&format!("GH_CONFIG_DIR={}", dir.display())));
    }

    #[test]
    fn delete_branch_applies_to_every_layer_and_a_shared_base_is_never_deleted_unasked() {
        // A release PR develop -> main with a feature PR on develop and another (#3) not in the stack.
        let prs = vec![
            pr(1, "develop", "main"),
            pr(2, "feat", "develop"),
            pr(3, "other", "develop"),
        ];
        let (_tmp, dir) = logged_in(&prs);
        let stack = stack_for(2, &prs).expect("stacked");
        gh_in(&dir, &fake_gh(), |gh| {
            skyelab_git::merge_stack(gh, &stack, MergeMethod::Squash, false).expect("merges");
        });
        let actions = lines(&dir.join("fake-gh.actions"));
        assert!(
            actions
                .iter()
                .all(|a| !a.contains("--delete-branch") && !a.contains("/pull/3")),
            "{actions:?}"
        );
        let (_tmp, dir) = logged_in(&prs);
        gh_in(&dir, &fake_gh(), |gh| {
            skyelab_git::merge_stack(gh, &stack, MergeMethod::Squash, true).expect("merges");
        });
        let merges: Vec<String> = lines(&dir.join("fake-gh.actions"))
            .into_iter()
            .filter(|a| a.starts_with("pr merge"))
            .collect();
        assert!(
            merges.len() == 2 && merges.iter().all(|a| a.ends_with("--delete-branch")),
            "{merges:?}"
        );
    }

    #[test]
    fn stops_at_the_first_failure_and_says_what_merged() {
        let prs = chain();
        let (_tmp, dir) = logged_in(&prs);
        // #11 moved since it was listed: its pinned merge is refused, and #12 is never tried.
        std::fs::write(dir.join("fake-head-11"), "f".repeat(40)).expect("moved");
        let stack = stack_for(12, &prs).expect("stacked");
        gh_in(&dir, &fake_gh(), |gh| {
            let e =
                skyelab_git::merge_stack(gh, &stack, MergeMethod::Squash, true).expect_err("stops");
            assert_eq!((e.merged.as_slice(), e.failed), (&[10][..], 11));
            assert!(
                e.to_string().starts_with("merged #10; #11 didn't merge:"),
                "{e}"
            );
            assert!(e.to_string().contains("Head branch was modified"), "{e}");
        });
        let actions = lines(&dir.join("fake-gh.actions"));
        assert_eq!(actions.len(), 3, "#12 not tried: {actions:?}");
        assert!(dir.join("fake-merged-10").exists() && !dir.join("fake-merged-12").exists());
    }

    #[test]
    fn a_merge_that_landed_counts_even_when_gh_failed_after_it() {
        let prs = chain();
        let (_tmp, dir) = logged_in(&prs);
        // gh merged #10 but failed deleting its branch: GitHub reads it merged.
        std::fs::write(
            dir.join("fake-action-fail-10"),
            "couldn't delete the branch",
        )
        .expect("fail");
        std::fs::write(dir.join("fake-merged-10"), "").expect("merged");
        let stack = stack_for(11, &prs).expect("stacked");
        gh_in(&dir, &fake_gh(), |gh| {
            let merged =
                skyelab_git::merge_stack(gh, &stack, MergeMethod::Squash, true).expect("merges");
            assert_eq!(merged, [10, 11]);
        });
    }

    #[test]
    fn a_bottom_moved_since_it_was_listed_is_never_merged() {
        let prs = chain();
        let (_tmp, dir) = logged_in(&prs);
        std::fs::write(dir.join("fake-base-10"), "release").expect("moved");
        let stack = stack_for(11, &prs).expect("stacked");
        gh_in(&dir, &fake_gh(), |gh| {
            let e =
                skyelab_git::merge_stack(gh, &stack, MergeMethod::Merge, false).expect_err("stops");
            assert_eq!((e.merged.len(), e.failed), (0, 10));
            assert!(e.to_string().contains("targets \"release\" now"), "{e}");
        });
        assert!(
            lines(&dir.join("fake-gh.actions")).is_empty(),
            "nothing merged"
        );
    }

    #[test]
    fn a_layer_without_a_known_head_stops_the_stack_before_gh_runs() {
        let mut prs = chain();
        prs[0].head = None; // #12
        let (_tmp, dir) = logged_in(&prs[1..]);
        let bin = fake_gh();
        let gh = GitHub {
            gh: &bin,
            config_dir: &dir,
        };
        let stack = stack_for(12, &prs).expect("stacked");
        let e =
            skyelab_git::merge_stack(&gh, &stack, MergeMethod::Squash, false).expect_err("refused");
        assert_eq!((e.merged.len(), e.failed), (0, 12));
        assert!(
            lines(&dir.join("fake-gh.actions")).is_empty(),
            "nothing merged"
        );
    }
}
