//! Git for SkyeLab: a worktree per agent thread, plus status and structured diffs.
//!
//! shortcut: shells out to the `git` CLI, so every call pays a process spawn and we parse text output;
//! move to `gix` once it covers worktree add/remove and status with renames.

mod branch;
mod checkpoint;
mod commit;
mod diff;
mod gh_lists;
mod pr_actions;
mod publish;
mod remote;
mod status;
mod summary;
pub mod watch;
mod worktree;

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub use branch::{Branches, OnDirty, branches, create_branch, switch_branch};
pub use checkpoint::{
    Checkpoint, RewindPlan, capture_checkpoint, checkpoints, delete_checkpoints, plan_rewind,
    rewind,
};
pub use commit::{commit, snapshot_tree};
pub use diff::{
    Against, FileDiff, Hunk, Line, MAX_FILE_LINES, MAX_TOTAL_LINES, diff, diff_merge_base,
    diff_stat, diff_trees, diff_with,
};
pub use gh_lists::{
    IssueFilter, IssueRow, MAX_BODY, MAX_ROWS, PrFilter, PrRow, issue_list_args, list_issues,
    list_prs, parse_issue_list, parse_pr_list, parse_repo_url, pr_list_args, repo_slug,
};
pub use pr_actions::{MergeMethod, Verdict, merge_methods, merge_pr, review_pr};
pub use publish::{GitHub, create_pr, credential_args, gh_login, push, push_with};
pub use remote::{
    Checks, FailedCheck, MAX_FAILED_CHECKS, PrState, PrStatus, Review, ahead_behind, fetch,
    gh_logged_in, parse_pr_view, pr_status, pull_ff,
};
pub use status::{FileChange, Status, status, status_with_ignored};
pub use summary::{MAX_DIFF, PrContext, TRUNCATED, commit_diff, pr_context};
pub use worktree::{Worktree, create_worktree, hook_folders, list_worktrees, remove_worktree};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("git is not installed (no `git` on PATH)")]
    GitNotInstalled,
    #[error("{} is not inside a git repository", .0.display())]
    NotARepo(PathBuf),
    #[error("branch `{0}` already exists")]
    BranchExists(String),
    #[error("`{0}` is not a valid branch name")]
    InvalidBranch(String),
    #[error("worktree {} has uncommitted, untracked or ignored files", .0.display())]
    Dirty(PathBuf),
    #[error("no platform data directory to put worktrees in")]
    NoDataDir,
    #[error("{0}")]
    Invalid(String),
    #[error("gh is not installed (no `gh` on PATH)")]
    GhNotInstalled,
    #[error("`{args}` failed: {stderr}")]
    Gh { args: String, stderr: String },
    #[error("`git {args}` failed: {stderr}")]
    Git { args: String, stderr: String },
    #[error("running git: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// How a file changed, shared by [`status`] and [`diff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
}

/// The top-level directory of the repo (or worktree) containing `path`.
pub fn repo_root(path: &Path) -> Result<PathBuf> {
    let out = git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(lossy(&out).trim_end_matches('\n')))
}

/// The files git tracks under `dir`, relative to it: what the composer's `@` menu offers. Ignored and untracked
/// files never show.
pub fn tracked_files(dir: &Path) -> Result<Vec<String>> {
    ls_files(dir, &["--cached"])
}

/// What the file picker lists: the tracked files under `dir` plus the untracked ones that aren't ignored, relative to
/// it and sorted.
pub fn project_files(dir: &Path) -> Result<Vec<String>> {
    let mut files = ls_files(dir, &["--cached", "--others", "--exclude-standard"])?;
    files.sort();
    files.dedup(); // an unmerged file is listed once per stage
    files.retain(|f| !f.ends_with('/')); // an untracked nested repo is listed as `sub/`
    Ok(files)
}

fn ls_files(dir: &Path, which: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["-c", "core.quotePath=false", "ls-files", "-z"];
    args.extend_from_slice(which);
    let out = git(dir, &args)?;
    Ok(out
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(lossy)
        .collect())
}

/// The branch checked out in `dir`, or `None` on a detached HEAD. Not `--short`: with a tag of the same name it
/// answers `heads/<branch>`.
pub fn current_branch(dir: &Path) -> Result<Option<String>> {
    let args = ["symbolic-ref", "--quiet", "HEAD"];
    let out = output(Path::new("git"), dir, &args)?;
    if out.status.code() == Some(1) && out.stderr.is_empty() {
        return Ok(None);
    }
    let out = finish(dir, &args, out)?;
    let full = lossy(&out);
    Ok(full.trim().strip_prefix("refs/heads/").map(str::to_owned))
}

/// The full SHA of commit-ish `rev` in the repo at `dir`.
pub fn rev_parse(dir: &Path, rev: &str) -> Result<String> {
    let out = git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
    )?;
    Ok(lossy(&out).trim().to_owned())
}

/// Adds `pattern` to the repo's own `info/exclude` (shared by all its worktrees, never committed), once.
pub fn exclude_locally(repo: &Path, pattern: &str) -> Result<()> {
    let out = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let file = PathBuf::from(lossy(&out).trim_end_matches('\n'))
        .join("info")
        .join("exclude");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    if text.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let sep = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file)?;
    io::Write::write_all(&mut f, format!("{sep}{pattern}\n").as_bytes())?;
    Ok(())
}

fn git<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<Vec<u8>> {
    git_with(Path::new("git"), dir, args)
}

fn git_with<S: AsRef<OsStr>>(bin: &Path, dir: &Path, args: &[S]) -> Result<Vec<u8>> {
    finish(dir, args, output(bin, dir, args)?)
}

/// `out`'s stdout, or the error its exit status and stderr name.
fn finish<S: AsRef<OsStr>>(dir: &Path, args: &[S], out: Output) -> Result<Vec<u8>> {
    if out.status.success() {
        return Ok(out.stdout);
    }
    let stderr = lossy(&out.stderr).trim().to_owned();
    if stderr.contains("not a git repository") {
        return Err(Error::NotARepo(dir.to_owned()));
    }
    let args = args
        .iter()
        .map(|a| a.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    Err(Error::Git { args, stderr })
}

/// For questions git answers with its exit status (does this ref exist?).
fn git_succeeds(dir: &Path, args: &[&str]) -> Result<bool> {
    Ok(output(Path::new("git"), dir, args)?.status.success())
}

fn output<S: AsRef<OsStr>>(bin: &Path, dir: &Path, args: &[S]) -> Result<Output> {
    skyelab_core::process::output(command(bin, dir)?.args(args), None).map_err(spawn_error)
}

fn spawn_error(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::NotFound {
        Error::GitNotInstalled
    } else {
        Error::Io(e)
    }
}

/// Git never through a shell, always on `dir`, with repo hooks and every other program the repo's own config names
/// off ([`program_overrides`]): what SkyeLab only reads (status, diff) or sets up (worktrees) must never run code an
/// agent wrote into `.git/hooks` or `.git/config`.
fn command(bin: &Path, dir: &Path) -> Result<Command> {
    let mut cmd = hooked_command(bin, dir);
    for (key, value) in program_overrides(bin, dir)? {
        cmd.arg("-c").arg(format!("{key}={value}"));
    }
    Ok(cmd)
}

/// [`hooked_command`] when `hooks`, else [`command`]: Commit, Push and Pull run hooks (and the repo's other
/// programs) only while the caller found no other account sharing the repo (`AccountsConfig::repo_hooks_allowed`).
fn user_command(dir: &Path, hooks: bool) -> Result<Command> {
    let git = Path::new("git");
    if hooks {
        Ok(hooked_command(git, dir))
    } else {
        command(git, dir)
    }
}

/// Always set: hooks, fsmonitor and a submodule's own config (`submodule.recurse` would check submodules out with
/// their filters, a dirty check runs git inside each) off, no credential helper (push and fetch add only the bound
/// gh after this), no askpass, and no automatic gc or maintenance afterwards (gc runs `gc.recentObjectsHook`).
const FIXED_OFF: [(&str, &str); 8] = [
    ("core.hooksPath", "/dev/null"),
    ("gc.auto", "0"),
    ("maintenance.auto", "false"),
    ("core.fsmonitor", "false"),
    ("submodule.recurse", "false"),
    ("diff.ignoreSubmodules", "dirty"),
    ("credential.helper", ""),
    ("core.askPass", ""),
];

/// Every hook event (githooks(5)). `core.hooksPath` only covers hook scripts; hooks set in config
/// (`hook.<name>.command` + `.event`, git 2.5x) need `hook.<event>.enabled=false`, which needs no listing.
pub(crate) const HOOK_EVENTS: [&str; 28] = [
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// Program keys git reads by a fixed name, with what git uses when neither system nor global config sets one (an
/// empty defaultKeyCommand is none). Always set, so a config write racing the listing can't slip one in.
const FIXED_PROGRAMS: [(&str, &str); 5] = [
    ("core.sshcommand", "ssh"),
    ("gpg.program", "gpg"),
    ("gpg.x509.program", "gpgsm"),
    ("gpg.ssh.program", "ssh-keygen"),
    ("gpg.ssh.defaultkeycommand", ""),
];

/// Every config key that names a program for the git SkyeLab runs. Diff drivers (`diff.external`,
/// `diff.<n>.command`/`textconv`) aren't here: every porcelain diff passes `--no-ext-diff --no-textconv`, and
/// plumbing never runs them.
const PROGRAM_KEYS: &str = r"^(hook\..+\.command|core\.sshcommand|gpg\.(.+\.)?program|gpg\.ssh\.defaultkeycommand|filter\..+\.(clean|smudge|process)|merge\..+\.driver)$";

/// The `-c` pairs that keep git in `dir` from running any program the repo's own config names (local and worktree
/// scope, and whatever they include; an agent can write them). A fixed-name key gets the system or global value, or
/// git's default. A filter or merge driver the repo defines gets the system or global value, or empty: an empty
/// filter passes content through (a `required` one fails, so an encrypting filter never commits plaintext) and an
/// empty merge driver is a conflict. Limit: a driver written between this listing and git's own read isn't seen.
pub(crate) fn program_overrides(bin: &Path, dir: &Path) -> Result<Vec<(String, String)>> {
    let args = ["config", "--show-scope", "-z", "--get-regexp", PROGRAM_KEYS];
    let mut cmd = hooked_command(bin, dir);
    let out = skyelab_core::process::output(cmd.args(args), None).map_err(spawn_error)?;
    // Exit 1 is "no key matched".
    let listing = if out.status.code() == Some(1) {
        Vec::new()
    } else {
        finish(dir, &args, out)?
    };
    overrides_from(&listing)
}

/// [`program_overrides`] from `git config --show-scope -z --get-regexp`'s output (`scope\0key\nvalue\0`...).
fn overrides_from(listing: &[u8]) -> Result<Vec<(String, String)>> {
    let text = std::str::from_utf8(listing)
        .map_err(|_| publish::invalid("the repo's config names a program key that isn't UTF-8"))?;
    let mut trusted = std::collections::HashMap::new();
    let mut repo: Vec<&str> = Vec::new();
    let mut fields = text.split('\0');
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
        // Two names for one setting: the last one set wins either way.
        let key = if key == "gpg.openpgp.program" {
            "gpg.program"
        } else {
            key
        };
        if matches!(scope, "system" | "global") && !key.starts_with("hook.") {
            trusted.insert(key, value);
        } else if !repo.contains(&key) {
            repo.push(key);
        }
    }
    let mut out: Vec<(String, String)> = FIXED_OFF
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .chain(
            HOOK_EVENTS
                .iter()
                .map(|e| (format!("hook.{e}.enabled"), "false".to_owned())),
        )
        .collect();
    for (key, default) in FIXED_PROGRAMS {
        let value = trusted.get(key).copied().unwrap_or(default);
        out.push((key.to_owned(), value.to_owned()));
    }
    for key in repo
        .into_iter()
        .filter(|k| !FIXED_PROGRAMS.iter().any(|(f, _)| f == k))
    {
        // `-c` splits at the first `=`, so such a name can't be switched off.
        if key.contains('=') {
            return Err(publish::invalid(format!(
                "the repo's config sets {key}, a name SkyeLab can't switch off; rename or remove it"
            )));
        }
        out.push(match key.strip_suffix(".command") {
            // A hook set in config, for an event a newer git may add: off, from any scope, as hook scripts are.
            Some(hook) if hook.starts_with("hook.") => {
                (format!("{hook}.enabled"), "false".to_owned())
            }
            _ => (
                key.to_owned(),
                trusted.get(key).copied().unwrap_or_default().to_owned(),
            ),
        });
    }
    Ok(out)
}

/// [`command`] with the repo's hooks on, for Commit and Push (users rely on pre-commit and pre-push). Either way
/// git, and any hook or filter it runs, gets only the agents' env allowlist: nothing of SkyeLab's own env (the
/// parent's credentials, CLAUDE_CONFIG_DIR, GH_*, GIT_* that would point git at another repo). The caller adds
/// the bound account's env where one applies.
fn hooked_command(bin: &Path, dir: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.arg("-C").arg(dir);
    // A repo's config can name a program for core.fsmonitor that every status would run.
    cmd.args(["-c", "core.fsmonitor=false"]);
    cmd.env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false));
    // English messages so the "not a git repository" match holds in any locale.
    cmd.env("LC_ALL", "C");
    // Read-only calls like status must not take index.lock while an agent is committing.
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

/// A CA file the child git trusts for https (`GIT_SSL_CAINFO`) on push and fetch: the fake https remote's, set by
/// tests and by `SKYELAB_GIT_CA_FILE` in debug runs. Only [`trust_test_ca`] sets it, and only debug builds have that.
/// It takes the place of git's CA file (a curl with a system CA folder still trusts that too); the CA's key never
/// leaves the fake remote's memory, so nothing else can present a cert it signed.
static TEST_CA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Debug builds only: push and fetch trust `file`'s CA from now on (the first call wins).
#[cfg(debug_assertions)]
pub fn trust_test_ca(file: &Path) {
    let _ = TEST_CA.set(file.to_owned());
}

/// The env a push or fetch adds for [`TEST_CA`].
fn test_ca_env() -> Option<(&'static str, &'static Path)> {
    TEST_CA.get().map(|ca| ("GIT_SSL_CAINFO", ca.as_path()))
}

// shortcut: non-UTF-8 path bytes become U+FFFD; switch to OsStr-from-bytes if a user hits it.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod whitespace_test;
