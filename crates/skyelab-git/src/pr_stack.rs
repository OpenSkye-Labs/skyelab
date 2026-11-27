//! Stacked pull requests: open PRs whose base branch is another open PR's head branch, read from one repo's open PR
//! list, and merging such a stack bottom first with the account's gh, each layer pinned to the head commit it was
//! listed with.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::gh_lists::{PrFilter, PrRow, list_prs};
use crate::pr_actions::{ACTION_LIMIT, MergeMethod, merge_pr, pr_link, valid_head};
use crate::publish::{checked_within, gh_command, invalid};
use crate::remote::PR_VIEW_LIMIT;
use crate::{Error, Result};

/// The stack one open PR is in.
#[derive(Debug, Clone, PartialEq)]
pub struct PrStack {
    /// The bottom layer (based on a branch that is no open PR's head, the default branch as a rule) up to the PR
    /// itself, in merge order.
    pub layers: Vec<PrRow>,
    /// The PR's place in the stack, from 1 at the bottom.
    pub position: usize,
    /// The stack's height: the longest chain of open PRs through this one.
    pub total: usize,
}

impl PrStack {
    /// "Stack 2/3".
    pub fn badge(&self) -> String {
        format!("Stack {}/{}", self.position, self.total)
    }

    /// The branch the whole stack lands on: the bottom layer's base.
    pub fn onto(&self) -> &str {
        self.layers.first().map_or("", |l| l.base_ref.as_str())
    }
}

/// The open PR whose head branch `pr` is based on; with two open PRs on one branch, the lowest number (deterministic).
fn below<'a>(by_head: &HashMap<&str, &'a PrRow>, pr: &PrRow) -> Option<&'a PrRow> {
    by_head
        .get(pr.base_ref.as_str())
        .copied()
        .filter(|b| b.number != pr.number)
}

/// How many layers stand on `pr` along its longest chain; `path` stops a loop.
fn height(
    pr: &PrRow,
    open: &[&PrRow],
    by_head: &HashMap<&str, &PrRow>,
    path: &mut HashSet<u64>,
) -> usize {
    let mut best = 0;
    for &up in open {
        if below(by_head, up).is_some_and(|b| b.number == pr.number) && path.insert(up.number) {
            best = best.max(1 + height(up, open, by_head, path));
            path.remove(&up.number);
        }
    }
    best
}

/// The stack open PR `number` is in, from `prs` (one repo's open PRs); `None` when it stands alone, isn't listed or
/// open, or its chain down loops (a loop has no bottom to merge first).
pub fn stack_for(number: u64, prs: &[PrRow]) -> Option<PrStack> {
    let mut open: Vec<&PrRow> = prs.iter().filter(|p| !p.state.is_done()).collect();
    open.sort_by_key(|p| p.number);
    let mut by_head: HashMap<&str, &PrRow> = HashMap::new();
    for p in &open {
        if !p.head_ref.is_empty() {
            by_head.entry(p.head_ref.as_str()).or_insert(p);
        }
    }
    let me = *open.iter().find(|p| p.number == number)?;
    let mut layers = vec![me];
    let mut seen = HashSet::from([me.number]);
    while let Some(down) = layers.last().and_then(|top| below(&by_head, top)) {
        if !seen.insert(down.number) {
            return None;
        }
        layers.push(down);
    }
    layers.reverse();
    let position = layers.len();
    let total = position + height(me, &open, &by_head, &mut seen);
    (total > 1).then(|| PrStack {
        layers: layers.into_iter().cloned().collect(),
        position,
        total,
    })
}

/// The open PRs of the repo of canonical PR link `url`, read with the account's gh, for [`stack_for`].
/// shortcut: one page of `gh pr list` (50 PRs), so a stack in a repo with more open PRs may read short; page through
/// the list if that bites.
pub fn repo_open_prs(gh: &crate::GitHub, url: &str) -> Result<Vec<PrRow>> {
    let mut parts = pr_link(url)?
        .trim_start_matches("https://github.com/")
        .split('/');
    let (owner, name) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    list_prs(gh, &format!("{owner}/{name}"), PrFilter::All)
}

/// A stack merge that stopped: what merged before it, and the layer that didn't.
#[derive(Debug, thiserror::Error)]
#[error("{}#{failed} didn't merge: {error}", merged_note(.merged))]
pub struct StackMergeError {
    /// The layers merged, bottom first.
    pub merged: Vec<u64>,
    pub failed: u64,
    #[source]
    pub error: Error,
}

fn merged_note(merged: &[u64]) -> String {
    if merged.is_empty() {
        return String::new();
    }
    let list: Vec<String> = merged.iter().map(|n| format!("#{n}")).collect();
    format!("merged {}; ", list.join(", "))
}

/// The base branch GitHub has for `url` now.
fn live_base(gh: &crate::GitHub, url: &str) -> Result<String> {
    let args = ["pr", "view", url, "--json", "baseRefName"];
    let out = checked_within(
        gh_command(gh, gh.config_dir, &args)?,
        "gh pr view",
        None,
        Some(PR_VIEW_LIMIT),
    )?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| Error::Gh {
        args: "pr view".into(),
        stderr: format!("unreadable output: {e}"),
    })?;
    Ok(v["baseRefName"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(100)
        .collect())
}

/// One layer: the bottom must still target `onto` (what the dialog showed); a layer above is moved onto it with
/// `gh pr edit --base` (the branch below just merged), so nothing merges into a lower branch and no branch is
/// deleted unless asked. A merge that landed but whose branch delete failed still counts as merged.
fn merge_layer(
    gh: &crate::GitHub,
    l: &PrRow,
    bottom: bool,
    onto: &str,
    method: MergeMethod,
    delete: bool,
) -> Result<()> {
    if bottom {
        let base = live_base(gh, &l.url)?;
        if base != onto {
            return Err(invalid(format!(
                "it targets {base:?} now, not {onto:?}; refresh it, then merge"
            )));
        }
    } else {
        let args = ["pr", "edit", &l.url, "--base", onto];
        checked_within(
            gh_command(gh, gh.config_dir, &args)?,
            "gh pr edit",
            None,
            Some(ACTION_LIMIT),
        )?;
    }
    let head = l.head.as_deref().unwrap_or_default();
    merge_pr(gh, &l.url, head, method, delete).or_else(|e| match crate::pr_status(gh, &l.url) {
        Ok(s) if s.state == crate::PrState::Merged => Ok(()),
        _ => Err(e),
    })
}

/// Merges `stack`'s layers bottom first with the account's gh ([`merge_pr`]: `--match-head-commit` with each layer's
/// listed head), stopping at the first that fails (see [`merge_layer`]). `delete_branch` applies to every layer. Every
/// link and head is checked before the first merge. Returns the merged PR numbers.
pub fn merge_stack(
    gh: &crate::GitHub,
    stack: &PrStack,
    method: MergeMethod,
    delete_branch: bool,
) -> std::result::Result<Vec<u64>, StackMergeError> {
    let stop = |merged: Vec<u64>, failed: u64, error: Error| StackMergeError {
        merged,
        failed,
        error,
    };
    let onto = stack.onto();
    for l in &stack.layers {
        pr_link(&l.url).map_err(|e| stop(Vec::new(), l.number, e))?;
        if !l.head.as_deref().is_some_and(valid_head) || onto.is_empty() {
            let why = "its head commit or base branch isn't known; refresh it, then merge";
            return Err(stop(Vec::new(), l.number, invalid(why)));
        }
    }
    let mut merged = Vec::new();
    for (i, l) in stack.layers.iter().enumerate() {
        if let Err(e) = merge_layer(gh, l, i == 0, onto, method, delete_branch) {
            return Err(stop(merged, l.number, e));
        }
        merged.push(l.number);
    }
    Ok(merged)
}
