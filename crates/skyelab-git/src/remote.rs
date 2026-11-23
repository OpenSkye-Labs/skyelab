//! Keeping up with the remote under one account's `gh`: fetch the upstream, count ahead/behind, pull fast-forward
//! only, and read the linked pull request's state.

use std::ffi::OsString;
use std::path::Path;

use serde_json::Value;

use crate::publish::{
    check_urls, checked, checked_within, credential_args, current_branch, gh_command, invalid,
};
use crate::{Error, Result, command, git, lossy, output, repo_root};

/// At most this many failing checks are kept from one read; gh's JSON is untrusted.
pub const MAX_FAILED_CHECKS: usize = 20;
const MAX_CHECK_NAME: usize = 200;
const MAX_URL: usize = 2048;
const PR_VIEW_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

/// The branch's upstream remote (`branch.<b>.remote`), `None` without one or for a local upstream (`.`).
fn upstream_remote(root: &Path) -> Result<Option<String>> {
    let Some(branch) = crate::current_branch(root)? else {
        return Ok(None);
    };
    let out = output(
        Path::new("git"),
        root,
        &["config", "--get", &format!("branch.{branch}.remote")],
    )?;
    let remote = lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || remote.is_empty() || remote == "." {
        return Ok(None);
    }
    if remote.starts_with('-') {
        return Err(invalid(format!(
            "SkyeLab won't fetch from a remote named {remote}"
        )));
    }
    Ok(Some(remote))
}

/// Fetches the current branch's upstream remote, https credentials only from the bound `gh` (as Push). `false` when
/// the branch has no upstream remote, so there is nothing to fetch.
pub fn fetch(dir: &Path, gh: &crate::GitHub) -> Result<bool> {
    let root = repo_root(dir)?;
    let Some(remote) = upstream_remote(&root)? else {
        return Ok(false);
    };
    let urls = check_urls(&root, &remote, false)?;
    check_repo_config(&root)?;
    checked(fetch_command(&root, &remote, &urls, gh)?, "git fetch", None)?;
    Ok(true)
}

/// The fetch itself: hooks off, credentials only from the bound gh, no prompt.
pub(crate) fn fetch_command(
    root: &Path,
    remote: &str,
    urls: &[String],
    gh: &crate::GitHub,
) -> Result<std::process::Command> {
    let urls: Vec<&str> = urls.iter().map(String::as_str).collect();
    let mut cmd = command(Path::new("git"), root);
    cmd.args(credential_args(gh, &urls)?)
        // A stalled transfer gives up instead of holding the background fetch for good.
        .args([
            "-c",
            "http.lowSpeedLimit=1000",
            "-c",
            "http.lowSpeedTime=60",
        ])
        .args(["fetch", "--quiet", "--recurse-submodules=no", remote])
        .env("GH_CONFIG_DIR", gh.config_dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    Ok(cmd)
}

/// Keys that name a program for a fetch to run (`vcs` hands even an https remote to `git-remote-<vcs>`), or reroute
/// or re-trust its https (where the bound gh hands over its token). An agent can write the repo's own config, and one
/// `.git/config` can serve folders bound to other accounts.
const UNSAFE_REPO_KEYS: &str = r"^(remote\..*\.(uploadpack|proxy|vcs)|http\..*(proxy|sslcainfo|sslcapath|sslcert|sslkey|curloptresolve|cookiefile|pinnedpubkey))$";

/// Refuses a fetch (it runs on a timer, unasked) while the repo's own config sets any [`UNSAFE_REPO_KEYS`].
fn check_repo_config(root: &Path) -> Result<()> {
    let out = output(
        Path::new("git"),
        root,
        &["config", "--show-scope", "--get-regexp", UNSAFE_REPO_KEYS],
    )?;
    // Exit 1 is "no key matched"; anything else unexpected fails closed.
    match out.status.code() {
        Some(0) => {}
        Some(1) => return Ok(()),
        _ => {
            return Err(Error::Git {
                args: "config --show-scope --get-regexp".into(),
                stderr: lossy(&out.stderr),
            });
        }
    }
    let text = lossy(&out.stdout);
    let repo_key = text.lines().find_map(|line| {
        let (scope, entry) = line.split_once('\t')?;
        matches!(scope, "local" | "worktree").then(|| entry.split(' ').next().unwrap_or_default())
    });
    match repo_key {
        Some(key) => Err(invalid(format!(
            "the repo's own config sets {key}, so SkyeLab won't fetch with the account's GitHub login; remove it \
             (git config --unset {key}) or set it in your global config"
        ))),
        None => Ok(()),
    }
}

/// Commits HEAD has that its upstream doesn't, and the other way round. `None` without an upstream (or detached).
pub fn ahead_behind(dir: &Path) -> Result<Option<(u32, u32)>> {
    let has = output(
        Path::new("git"),
        dir,
        &["rev-parse", "--verify", "--quiet", "@{upstream}"],
    )?;
    if !has.status.success() {
        return Ok(None);
    }
    let out = lossy(&git(
        dir,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?);
    let mut counts = out.split_whitespace().map(str::parse::<u32>);
    match (counts.next(), counts.next()) {
        (Some(Ok(ahead)), Some(Ok(behind))) => Ok(Some((ahead, behind))),
        _ => Err(Error::Git {
            args: "rev-list --left-right --count".into(),
            stderr: format!("unexpected output: {}", out.trim()),
        }),
    }
}

/// Fetches, then fast-forwards the branch to its upstream. Refuses tracked changes (commit or stash them first), a
/// branch with no upstream, and a branch with commits the upstream lacks: those need a merge or rebase by hand.
/// `env` is the bound account's env for the repo's hooks, as Commit and Push pass it.
pub fn pull_ff(dir: &Path, gh: &crate::GitHub, env: &[(OsString, OsString)]) -> Result<String> {
    let root = repo_root(dir)?;
    let branch = current_branch(&root)?;
    let status = crate::status(&root)?;
    if !(status.staged.is_empty() && status.unstaged.is_empty()) {
        return Err(invalid(
            "the folder has uncommitted changes; commit or stash them, then pull",
        ));
    }
    fetch(&root, gh)?;
    let Some((ahead, behind)) = ahead_behind(&root)? else {
        return Err(invalid(format!(
            "{branch} has no upstream branch yet; push it first"
        )));
    };
    if behind == 0 {
        return Ok(format!("{branch} is up to date"));
    }
    if ahead > 0 {
        return Err(invalid(format!(
            "can't fast-forward: {branch} has {ahead} commit(s) its upstream doesn't, and is {behind} behind; \
             merge or rebase it by hand"
        )));
    }
    let mut cmd = crate::hooked_command(Path::new("git"), &root);
    // Git counts ignored files as expendable; a local `.env` the upstream starts tracking may be the only copy.
    cmd.args([
        "merge",
        "--ff-only",
        "--no-edit",
        "--no-overwrite-ignore",
        "@{upstream}",
    ])
    .envs(env.iter().map(|(k, v)| (k, v)))
    .env("GIT_TERMINAL_PROMPT", "0");
    checked(cmd, "git merge --ff-only", None)?;
    Ok(format!("Pulled {behind} commit(s) into {branch}"))
}

/// A pull request's state as SkyeLab shows it; draft is an open PR marked draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

impl PrState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draft => "draft",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Open, Self::Draft, Self::Merged, Self::Closed]
            .into_iter()
            .find(|v| v.as_str() == s)
    }

    /// Merged or closed: nothing will change it on its own, so it isn't read again.
    pub fn is_done(self) -> bool {
        matches!(self, Self::Merged | Self::Closed)
    }
}

/// The checks' rollup: any failure wins, then anything still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    None,
    Pending,
    Passing,
    Failing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    None,
    Required,
    Approved,
    ChangesRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedCheck {
    pub name: String,
    /// Its page, only when it is an https link.
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatus {
    pub state: PrState,
    pub checks: Checks,
    pub review: Review,
    /// The failing checks, at most [`MAX_FAILED_CHECKS`].
    pub failed: Vec<FailedCheck>,
}

/// Whether `config_dir` holds a github.com login, read from gh's `hosts.yml` without running gh: an account that
/// never logged in is never asked.
pub fn gh_logged_in(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join("hosts.yml"))
        .is_ok_and(|t| t.lines().any(|l| l.trim_end() == "github.com:"))
}

/// Reads the pull request at `url` (a GitHub PR link) with `gh pr view` under the account's config dir.
pub fn pr_status(gh: &crate::GitHub, url: &str) -> Result<PrStatus> {
    if skyelab_core::github_pr_url(url).as_deref() != Some(url) {
        return Err(invalid("not a GitHub pull request link"));
    }
    let fields = "state,isDraft,mergedAt,reviewDecision,statusCheckRollup";
    let args = ["pr", "view", url, "--json", fields];
    // A gh that never answers (a keychain prompt, a dead network) is killed, so polls can't pile up behind it.
    let out = checked_within(
        gh_command(gh, gh.config_dir, &args)?,
        "gh pr view",
        None,
        Some(PR_VIEW_LIMIT),
    )?;
    parse_pr_view(&out.stdout)
}

/// gh's `pr view --json` answer as a [`PrStatus`].
pub fn parse_pr_view(json: &[u8]) -> Result<PrStatus> {
    let bad = |why: String| Error::Gh {
        args: "pr view".into(),
        stderr: why,
    };
    let v: Value =
        serde_json::from_slice(json).map_err(|e| bad(format!("unreadable output: {e}")))?;
    let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_owned();
    let state = match (text(&v, "state").as_str(), v["isDraft"] == true) {
        ("MERGED", _) => PrState::Merged,
        ("CLOSED", _) => PrState::Closed,
        ("OPEN", true) => PrState::Draft,
        ("OPEN", false) => PrState::Open,
        (other, _) => return Err(bad(format!("unknown pull request state {other:?}"))),
    };
    let review = match text(&v, "reviewDecision").as_str() {
        "APPROVED" => Review::Approved,
        "CHANGES_REQUESTED" => Review::ChangesRequested,
        "REVIEW_REQUIRED" => Review::Required,
        _ => Review::None,
    };
    let (mut any, mut pending, mut failed) = (false, false, Vec::new());
    for c in v["statusCheckRollup"].as_array().into_iter().flatten() {
        any = true;
        let run = c["__typename"] == "CheckRun";
        let (name, link, fail, wait) = if run {
            let conclusion = text(c, "conclusion");
            (
                text(c, "name"),
                text(c, "detailsUrl"),
                matches!(
                    conclusion.as_str(),
                    "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
                ),
                text(c, "status") != "COMPLETED",
            )
        } else {
            let state = text(c, "state");
            (
                text(c, "context"),
                text(c, "targetUrl"),
                matches!(state.as_str(), "FAILURE" | "ERROR"),
                matches!(state.as_str(), "PENDING" | "EXPECTED"),
            )
        };
        pending |= wait;
        if fail && failed.len() < MAX_FAILED_CHECKS {
            let url = (link.starts_with("https://")
                && link.len() <= MAX_URL
                && !link.contains(char::is_whitespace))
            .then_some(link);
            failed.push(FailedCheck {
                name: name.chars().take(MAX_CHECK_NAME).collect(),
                url,
            });
        }
    }
    let checks = match (any, failed.is_empty(), pending) {
        (false, _, _) => Checks::None,
        (_, false, _) => Checks::Failing,
        (_, _, true) => Checks::Pending,
        _ => Checks::Passing,
    };
    Ok(PrStatus {
        state,
        checks,
        review,
        failed,
    })
}
