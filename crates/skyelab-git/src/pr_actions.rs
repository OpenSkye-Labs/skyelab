//! Acting on a pull request under one account's gh login: merge it, or review it (approve, request changes,
//! comment). gh runs in the account's config dir with the PR named by its canonical link, so it never reads a
//! project's git config, and `--delete-branch` only deletes the branch on GitHub, never a local checkout.

use std::time::Duration;

use serde_json::Value;

use crate::publish::{MAX_MESSAGE, checked_within, gh_command, invalid};
use crate::remote::PR_VIEW_LIMIT;
use crate::{Error, Result};

/// A merge or review that hangs (a dead network) is killed after this long.
const ACTION_LIMIT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    /// T3's order: squash first.
    pub const ALL: [Self; 3] = [Self::Squash, Self::Merge, Self::Rebase];

    fn flag(self) -> &'static str {
        match self {
            Self::Squash => "--squash",
            Self::Merge => "--merge",
            Self::Rebase => "--rebase",
        }
    }

    /// The field of `gh repo view --json` that allows it.
    fn allowed_field(self) -> &'static str {
        match self {
            Self::Squash => "squashMergeAllowed",
            Self::Merge => "mergeCommitAllowed",
            Self::Rebase => "rebaseMergeAllowed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

/// `url` as given when it is a canonical GitHub PR link (what gh gets as the PR), else an error before gh runs.
fn pr_link(url: &str) -> Result<&str> {
    if skyelab_core::github_pr_url(url).as_deref() == Some(url) {
        Ok(url)
    } else {
        Err(invalid("not a GitHub pull request link"))
    }
}

/// `github.com/<owner>/<repo>` of a canonical PR link (`https://github.com/<owner>/<repo>/pull/<n>`).
fn repo_of(url: &str) -> String {
    let mut parts = url.trim_start_matches("https://github.com/").split('/');
    let (owner, name) = (parts.next(), parts.next());
    format!(
        "github.com/{}/{}",
        owner.unwrap_or_default(),
        name.unwrap_or_default()
    )
}

/// The merge methods the PR's repository allows, read with `gh repo view`, in [`MergeMethod::ALL`] order.
pub fn merge_methods(gh: &crate::GitHub, url: &str) -> Result<Vec<MergeMethod>> {
    let repo = repo_of(pr_link(url)?);
    let fields = "mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed";
    let args = ["repo", "view", &repo, "--json", fields];
    let out = checked_within(
        gh_command(gh, gh.config_dir, &args)?,
        "gh repo view",
        None,
        Some(PR_VIEW_LIMIT),
    )?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| Error::Gh {
        args: "repo view".into(),
        stderr: format!("unreadable output: {e}"),
    })?;
    Ok(MergeMethod::ALL
        .into_iter()
        .filter(|m| v[m.allowed_field()] == true)
        .collect())
}

/// Merges the PR at `url` with `gh pr merge`, deleting its branch on GitHub when `delete_branch`. The merge is pinned
/// to `head` (`--match-head-commit`), the commit whose checks the user saw: GitHub refuses it when the branch moved
/// since. gh refuses a merge its branch protection blocks; `--admin` is never passed.
pub fn merge_pr(
    gh: &crate::GitHub,
    url: &str,
    head: &str,
    method: MergeMethod,
    delete_branch: bool,
) -> Result<()> {
    let url = pr_link(url)?;
    if !(matches!(head.len(), 40 | 64) && head.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(invalid(
            "the pull request's head commit isn't known; refresh it, then merge",
        ));
    }
    // With --repo, gh never deletes or switches a local branch, whatever git repo its cwd is in.
    let repo = repo_of(url);
    let mut args = vec![
        "pr",
        "merge",
        url,
        "--repo",
        &repo,
        "--match-head-commit",
        head,
        method.flag(),
    ];
    if delete_branch {
        args.push("--delete-branch");
    }
    let cmd = gh_command(gh, gh.config_dir, &args)?;
    checked_within(cmd, "gh pr merge", None, Some(ACTION_LIMIT)).map(drop)
}

/// Reviews the PR at `url` with `gh pr review`; the body goes through stdin. Request changes and Comment need a body.
pub fn review_pr(gh: &crate::GitHub, url: &str, verdict: Verdict, body: &str) -> Result<()> {
    let url = pr_link(url)?;
    if body.len() > MAX_MESSAGE || body.contains('\0') {
        return Err(invalid("a review must be text under 64 KiB"));
    }
    if verdict != Verdict::Approve && body.trim().is_empty() {
        return Err(invalid("write what the review says first"));
    }
    let flag = match verdict {
        Verdict::Approve => "--approve",
        Verdict::RequestChanges => "--request-changes",
        Verdict::Comment => "--comment",
    };
    let args = ["pr", "review", url, flag, "--body-file", "-"];
    let cmd = gh_command(gh, gh.config_dir, &args)?;
    checked_within(
        cmd,
        "gh pr review",
        Some(body.as_bytes()),
        Some(ACTION_LIMIT),
    )
    .map(drop)
}

/// A pull request's diff and the head commit it was read at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDiff {
    pub files: Vec<crate::FileDiff>,
    /// `None` when gh didn't report a head: line comments can't be pinned then.
    pub head: Option<String>,
}

/// Reads the PR's diff with `gh pr diff`, between two `gh pr view` reads of its head: a head that moved meanwhile is an
/// error, so comments are pinned to the commit the diff shows. The diff comes from other people: it is only parsed
/// into text rows, capped like a local diff, and drawn as GPUI text, never sent to a terminal.
pub fn pr_diff(gh: &crate::GitHub, url: &str) -> Result<PrDiff> {
    let url = pr_link(url)?;
    let before = crate::pr_status(gh, url)?.head;
    let args = ["pr", "diff", url, "--color", "never"];
    // shortcut: gh's stdout is read whole before the parser caps it; GitHub itself refuses PR diffs past ~20k lines.
    let out = checked_within(
        gh_command(gh, gh.config_dir, &args)?,
        "gh pr diff",
        None,
        Some(ACTION_LIMIT),
    )?;
    let after = crate::pr_status(gh, url)?.head;
    if before != after {
        return Err(invalid(
            "the pull request changed while it was read; refresh to see the new diff",
        ));
    }
    Ok(PrDiff {
        files: crate::diff::parse(&out.stdout),
        head: after,
    })
}

/// Which side of the diff a line comment is on: the old file (`LEFT`, a removed line) or the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn api(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }
}

/// A line comment of a review: on `line` of `side`, or on `start..=line` when `start` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    /// Repo-relative, as the diff names the file.
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub start: Option<(u32, Side)>,
    pub body: String,
}

/// The most line comments one review sends.
const MAX_COMMENTS: usize = 100;

/// Posts one review of the PR at `url` with line `comments`, pinned to `head` (the commit their lines were read at),
/// through `gh api` (`gh pr review` can't attach line comments); the JSON goes through stdin. Like T3, a review with
/// comments needs no summary; one without comments goes through [`review_pr`] instead.
pub fn submit_review(
    gh: &crate::GitHub,
    url: &str,
    head: &str,
    verdict: Verdict,
    body: &str,
    comments: &[ReviewComment],
) -> Result<()> {
    let url = pr_link(url)?;
    if !(matches!(head.len(), 40 | 64) && head.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(invalid(
            "the pull request's head commit isn't known; refresh the diff, then review",
        ));
    }
    if comments.is_empty() || comments.len() > MAX_COMMENTS {
        return Err(invalid(format!(
            "a review takes 1 to {MAX_COMMENTS} line comments"
        )));
    }
    let texts = std::iter::once(body).chain(comments.iter().map(|c| c.body.as_str()));
    let total: usize = comments.iter().map(|c| c.body.len() + c.path.len()).sum();
    if total + body.len() > MAX_MESSAGE || texts.clone().any(|t| t.contains('\0')) {
        return Err(invalid("a review must be text under 64 KiB"));
    }
    let event = match verdict {
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
        Verdict::Comment => "COMMENT",
    };
    let comments: Vec<Value> = comments
        .iter()
        .map(|c| {
            let mut v = serde_json::json!({
                "path": c.path,
                "line": c.line,
                "side": c.side.api(),
                "body": c.body,
            });
            // GitHub refuses a start that is the end itself, so a one-line range is sent as one line.
            if let Some((line, side)) = c.start.filter(|&s| s != (c.line, c.side)) {
                v["start_line"] = line.into();
                v["start_side"] = side.api().into();
            }
            v
        })
        .collect();
    let json = serde_json::json!({
        "commit_id": head,
        "event": event,
        "body": body,
        "comments": comments,
    });
    let path = format!(
        "repos/{}/pulls/{}/reviews",
        repo_of(url).trim_start_matches("github.com/"),
        url.rsplit('/').next().unwrap_or_default()
    );
    let args = ["api", "--method", "POST", &path, "--input", "-"];
    let cmd = gh_command(gh, gh.config_dir, &args)?;
    checked_within(
        cmd,
        "gh api reviews",
        Some(json.to_string().as_bytes()),
        Some(ACTION_LIMIT),
    )
    .map(drop)
}
