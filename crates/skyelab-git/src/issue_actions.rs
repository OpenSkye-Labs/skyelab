//! Acting on an issue under one account's gh login: comment on it, close it (completed or not planned) or reopen it,
//! and read its row again. Like PR actions, gh runs in the account's config dir with the issue named by its canonical
//! link, so it never reads a project's git config. Comments go through stdin: `gh issue close` and `gh issue reopen`
//! take one only as an argument, so a comment that goes with them is posted first with `gh issue comment`.

use crate::gh_lists::{IssueRow, parse_issue_list, valid_part};
use crate::pr_actions::ACTION_LIMIT;
use crate::publish::{MAX_MESSAGE, checked_within, gh_command, invalid};
use crate::remote::PR_VIEW_LIMIT;
use crate::{Error, Result};

/// Starts the error of a close or reopen that failed after its comment was posted: a retry must not post it again.
pub const COMMENT_POSTED: &str = "the comment was posted";

/// Why an issue is closed (`gh issue close --reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Completed,
    NotPlanned,
}

impl CloseReason {
    fn arg(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not planned",
        }
    }
}

/// `url` as given when it is a canonical GitHub issue link (`https://github.com/<owner>/<repo>/issues/<n>`), else an
/// error before gh runs.
pub fn issue_link(url: &str) -> Result<&str> {
    let canonical = url
        .strip_prefix("https://github.com/")
        .and_then(|rest| {
            let mut parts = rest.split('/');
            let (owner, repo, kind, n) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            let n: u64 = n.parse().ok().filter(|&n| n > 0)?;
            (parts.next().is_none() && valid_part(owner) && valid_part(repo) && kind == "issues")
                .then(|| format!("https://github.com/{owner}/{repo}/issues/{n}"))
        })
        .is_some_and(|c| c == url);
    if canonical {
        Ok(url)
    } else {
        Err(invalid("not a GitHub issue link"))
    }
}

fn check_body(body: &str) -> Result<()> {
    if body.len() > MAX_MESSAGE || body.contains('\0') {
        return Err(invalid("a comment must be text under 64 KiB"));
    }
    Ok(())
}

/// Comments on the issue at `url`; the body goes through stdin.
pub fn comment_issue(gh: &crate::GitHub, url: &str, body: &str) -> Result<()> {
    let url = issue_link(url)?;
    check_body(body)?;
    if body.trim().is_empty() {
        return Err(invalid("write the comment first"));
    }
    let args = ["issue", "comment", url, "--body-file", "-"];
    let cmd = gh_command(gh, gh.config_dir, &args)?;
    checked_within(
        cmd,
        "gh issue comment",
        Some(body.as_bytes()),
        Some(ACTION_LIMIT),
    )
    .map(drop)
}

/// Posts `comment` when it has text, then runs `gh issue <args…>`; a failure after the comment says it was posted.
fn change_state(gh: &crate::GitHub, url: &str, comment: &str, args: &[&str]) -> Result<()> {
    let url = issue_link(url)?;
    check_body(comment)?;
    let commented = !comment.trim().is_empty();
    if commented {
        comment_issue(gh, url, comment)?;
    }
    let mut full = vec!["issue", args[0], url];
    full.extend(&args[1..]);
    let what = format!("gh issue {}", args[0]);
    let cmd = gh_command(gh, gh.config_dir, &full)?;
    match checked_within(cmd, &what, None, Some(ACTION_LIMIT)) {
        Err(e) if commented => Err(invalid(format!("{COMMENT_POSTED}, but {what} failed: {e}"))),
        r => r.map(drop),
    }
}

/// Closes the issue at `url` for `reason`, after posting `comment` when it has text.
pub fn close_issue(
    gh: &crate::GitHub,
    url: &str,
    reason: CloseReason,
    comment: &str,
) -> Result<()> {
    change_state(gh, url, comment, &["close", "--reason", reason.arg()])
}

/// Reopens the issue at `url`, after posting `comment` when it has text.
pub fn reopen_issue(gh: &crate::GitHub, url: &str, comment: &str) -> Result<()> {
    change_state(gh, url, comment, &["reopen"])
}

/// The issue at `url` read again as a list row (open or closed), with `gh issue view`. Its JSON is untrusted and
/// cleaned like a list's; an answer about another issue is an error.
pub fn issue_view(gh: &crate::GitHub, url: &str) -> Result<IssueRow> {
    let url = issue_link(url)?;
    let fields = "number,title,url,author,labels,assignees,updatedAt,body,state";
    let args = ["issue", "view", url, "--json", fields];
    let out = checked_within(
        gh_command(gh, gh.config_dir, &args)?,
        "gh issue view",
        None,
        Some(PR_VIEW_LIMIT),
    )?;
    let list = [&b"["[..], &out.stdout, b"]"].concat();
    parse_issue_list(&list)?
        .into_iter()
        .next()
        // The link is canonical, so its last part is the number.
        .filter(|row| row.url == url && url.ends_with(&format!("/{}", row.number)))
        .ok_or_else(|| Error::Gh {
            args: "issue view".into(),
            stderr: "gh answered about another issue".into(),
        })
}
