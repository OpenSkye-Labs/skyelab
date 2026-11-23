//! A repo's open pull requests and issues, listed with `gh pr list` / `gh issue list` under one account's gh login.
//! gh runs in the account's config dir with the repo named by `--repo`, so it never reads the project's own git
//! config; the repo comes from the project's `origin` URL, and only a github.com one.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::publish::{checked_within, gh_command, invalid};
use crate::remote::{PR_VIEW_LIMIT, review, rollup};
use crate::{Checks, Error, Result, Review, git, lossy, repo_root};

/// The most rows one list keeps (and asks gh for).
pub const MAX_ROWS: usize = 50;
pub const MAX_TITLE: usize = 300;
pub const MAX_LABELS: usize = 10;
pub const MAX_BODY: usize = 4000;
const MAX_NAME: usize = 100;
const MAX_URL: usize = 2048;

#[derive(Debug, Clone, PartialEq)]
pub struct PrRow {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub draft: bool,
    pub checks: Checks,
    pub review: Review,
    /// gh's `updatedAt`, RFC 3339.
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IssueRow {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub updated_at: String,
    /// At most [`MAX_BODY`] characters.
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrFilter {
    Mine,
    ReviewRequested,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueFilter {
    Assigned,
    All,
}

/// `owner/repo` of the folder's `origin`, when it is a github.com https or ssh URL.
pub fn repo_slug(dir: &Path) -> Result<String> {
    let root = repo_root(dir)?;
    let url = lossy(&git(&root, &["remote", "get-url", "origin"])?);
    // The URL itself isn't shown: it may hold a token.
    parse_repo_url(url.trim())
        .ok_or_else(|| invalid("the project's origin remote isn't a github.com repository"))
}

/// `owner/repo` from a github.com remote URL (https without credentials, `git@github.com:` or `ssh://git@github.com/`).
pub fn parse_repo_url(url: &str) -> Option<String> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    (valid_part(owner) && valid_part(name)).then(|| format!("{owner}/{name}"))
}

fn valid_part(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME
        && !s.starts_with(['-', '.'])
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn base_args(kind: &str, repo: &str, fields: &str) -> Vec<String> {
    [
        kind,
        "list",
        "--repo",
        &format!("github.com/{repo}"),
        "--state",
        "open",
        "--limit",
        &MAX_ROWS.to_string(),
        "--json",
        fields,
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn pr_list_args(repo: &str, filter: PrFilter) -> Vec<String> {
    let fields = "number,title,url,author,isDraft,reviewDecision,statusCheckRollup,updatedAt";
    let mut args = base_args("pr", repo, fields);
    match filter {
        PrFilter::Mine => args.extend(["--author".into(), "@me".into()]),
        PrFilter::ReviewRequested => {
            args.extend(["--search".into(), "review-requested:@me".into()]);
        }
        PrFilter::All => {}
    }
    args
}

pub fn issue_list_args(repo: &str, filter: IssueFilter) -> Vec<String> {
    let fields = "number,title,url,author,labels,assignees,updatedAt,body";
    let mut args = base_args("issue", repo, fields);
    if filter == IssueFilter::Assigned {
        args.extend(["--assignee".into(), "@me".into()]);
    }
    args
}

/// Open PRs of `repo` (`owner/repo`), read with the account's gh.
pub fn list_prs(gh: &crate::GitHub, repo: &str, filter: PrFilter) -> Result<Vec<PrRow>> {
    check_repo(repo)?;
    let out = list_within(gh, &pr_list_args(repo, filter), "gh pr list", PR_VIEW_LIMIT)?;
    parse_pr_list(&out)
}

/// Open issues of `repo` (`owner/repo`), read with the account's gh.
pub fn list_issues(gh: &crate::GitHub, repo: &str, filter: IssueFilter) -> Result<Vec<IssueRow>> {
    check_repo(repo)?;
    let args = issue_list_args(repo, filter);
    let out = list_within(gh, &args, "gh issue list", PR_VIEW_LIMIT)?;
    parse_issue_list(&out)
}

fn check_repo(repo: &str) -> Result<()> {
    match repo.split_once('/') {
        Some((o, n)) if valid_part(o) && valid_part(n) => Ok(()),
        _ => Err(invalid(format!("{repo:?} isn't a GitHub owner/repo"))),
    }
}

/// gh with `args` in the account's config dir (not the repo), killed after `limit`.
pub(crate) fn list_within(
    gh: &crate::GitHub,
    args: &[String],
    what: &str,
    limit: Duration,
) -> Result<Vec<u8>> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let cmd = gh_command(gh, gh.config_dir, &args)?;
    Ok(checked_within(cmd, what, None, Some(limit))?.stdout)
}

fn rows(json: &[u8], what: &str) -> Result<Vec<Value>> {
    let bad = |why: String| Error::Gh {
        args: what.into(),
        stderr: why,
    };
    match serde_json::from_slice(json).map_err(|e| bad(format!("unreadable output: {e}")))? {
        Value::Array(rows) => Ok(rows),
        _ => Err(bad("expected a JSON list".into())),
    }
}

fn text(v: &Value, key: &str, max: usize) -> String {
    v[key]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(max)
        .collect()
}

fn login(v: &Value) -> String {
    text(v, "login", MAX_NAME)
}

/// The row's number and link, when the link is a github.com https URL; else the row is dropped.
fn identity(v: &Value) -> Option<(u64, String)> {
    let url = v["url"].as_str()?;
    let ok = url.starts_with("https://github.com/")
        && url.len() <= MAX_URL
        && !url.contains(char::is_whitespace);
    ok.then(|| Some((v["number"].as_u64().filter(|&n| n > 0)?, url.to_owned())))
        .flatten()
}

pub fn parse_pr_list(json: &[u8]) -> Result<Vec<PrRow>> {
    Ok(rows(json, "pr list")?
        .iter()
        .filter_map(|v| {
            let (number, url) = identity(v)?;
            Some(PrRow {
                number,
                url,
                title: text(v, "title", MAX_TITLE),
                author: login(&v["author"]),
                draft: v["isDraft"] == true,
                checks: rollup(&v["statusCheckRollup"]).0,
                review: review(v["reviewDecision"].as_str().unwrap_or_default()),
                updated_at: text(v, "updatedAt", 64),
            })
        })
        .take(MAX_ROWS)
        .collect())
}

pub fn parse_issue_list(json: &[u8]) -> Result<Vec<IssueRow>> {
    let names = |v: &Value, key: &str, field: &str| -> Vec<String> {
        v[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| text(x, field, MAX_NAME))
            .filter(|s| !s.is_empty())
            .take(MAX_LABELS)
            .collect()
    };
    Ok(rows(json, "issue list")?
        .iter()
        .filter_map(|v| {
            let (number, url) = identity(v)?;
            Some(IssueRow {
                number,
                url,
                title: text(v, "title", MAX_TITLE),
                author: login(&v["author"]),
                labels: names(v, "labels", "name"),
                assignees: names(v, "assignees", "login"),
                updated_at: text(v, "updatedAt", 64),
                body: text(v, "body", MAX_BODY),
            })
        })
        .take(MAX_ROWS)
        .collect())
}

#[cfg(test)]
#[path = "gh_lists_test.rs"]
mod tests;
