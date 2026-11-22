//! Commit, push and open a pull request, each under one account's `gh` login.

use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::{Error, Result, command, git, git_succeeds, lossy, repo_root};

const MAX_MESSAGE: usize = 64 * 1024;
const MAX_TITLE: usize = 256;
const MAX_BODY: usize = 64 * 1024;
const MAX_URL: usize = 2048;

/// The `gh` to run and the config dir (`GH_CONFIG_DIR`) holding the account's GitHub login.
#[derive(Debug, Clone, Copy)]
pub struct GitHub<'a> {
    /// An absolute path, or a bare name looked up in the absolute entries of `PATH`.
    pub gh: &'a Path,
    pub config_dir: &'a Path,
}

/// Commits `paths` (every change, untracked files included, when empty) with `message`, and returns the new commit.
pub fn commit(dir: &Path, message: &str, paths: &[PathBuf]) -> Result<String> {
    if message.trim().is_empty() || message.contains('\0') || message.len() > MAX_MESSAGE {
        return Err(invalid(
            "a commit message must be non-empty text under 64 KiB",
        ));
    }
    let root = repo_root(dir)?;
    let mut add: Vec<&OsStr> = vec!["add".as_ref(), "-A".as_ref()];
    let mut commit: Vec<&OsStr> = vec!["commit".as_ref(), "-F".as_ref(), "-".as_ref()];
    if !paths.is_empty() {
        let tail = std::iter::once("--".as_ref()).chain(paths.iter().map(|p| p.as_os_str()));
        add.extend(tail.clone());
        commit.extend(tail);
    }
    git(&root, &add)?;
    let mut cmd = command(Path::new("git"), &root);
    cmd.args(&commit);
    checked(cmd, "git commit", Some(message.as_bytes()))?;
    Ok(lossy(&git(&root, &["rev-parse", "HEAD"])?)
        .trim()
        .to_owned())
}

/// Pushes the current branch to its upstream, or to `origin/<branch>` (and sets that as the upstream).
/// Returns `<remote>/<branch>`. https credentials come only from `gh` (see [`credential_args`]).
pub fn push(dir: &Path, gh: &GitHub) -> Result<String> {
    let root = repo_root(dir)?;
    let branch = current_branch(&root)?;
    let config = |key: String| -> Result<Option<String>> {
        let out = crate::output(Path::new("git"), &root, &["config", "--get", &key])?;
        Ok(out
            .status
            .success()
            .then(|| lossy(&out.stdout).trim().to_owned()))
    };
    let upstream = match (
        config(format!("branch.{branch}.remote"))?,
        config(format!("branch.{branch}.merge"))?,
    ) {
        (Some(remote), Some(merge)) if remote != "." => Some((remote, merge)),
        _ => None,
    };
    let (remote, refspec, set_upstream) = match upstream {
        Some((remote, merge)) => {
            if remote.starts_with('-') || !merge.starts_with("refs/heads/") {
                return Err(invalid(format!(
                    "branch {branch} tracks {remote} {merge}, which SkyeLab won't push to"
                )));
            }
            (remote, format!("HEAD:{merge}"), false)
        }
        None => ("origin".into(), format!("HEAD:refs/heads/{branch}"), true),
    };
    let mut cmd = command(Path::new("git"), &root);
    cmd.args(credential_args(gh)?)
        .arg("push")
        .args(set_upstream.then_some("--set-upstream"))
        .args([&remote, &refspec])
        .env("GH_CONFIG_DIR", gh.config_dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    checked(cmd, "git push", None)?;
    let to = refspec.trim_start_matches("HEAD:refs/heads/");
    Ok(format!("{remote}/{to}"))
}

/// The `-c` args that make git ask only the bound `gh` for https credentials: the empty helper drops every
/// helper from the user's and system config (the keychain's login among them), then `gh auth git-credential`.
pub fn credential_args(gh: &GitHub) -> Result<Vec<String>> {
    let bin = resolve(gh.gh)?;
    let bin = bin.to_str().filter(|b| !b.contains('\'')).ok_or_else(|| {
        invalid(format!(
            "can't use {} as a credential helper",
            bin.display()
        ))
    })?;
    // git runs a `!` helper through the shell, so the path is single-quoted.
    Ok(vec![
        "-c".into(),
        "credential.helper=".into(),
        "-c".into(),
        format!("credential.helper=!'{bin}' auth git-credential"),
    ])
}

/// Opens a pull request for the current branch with `gh pr create`; the base is gh's default (the repo's
/// default branch). The body goes through stdin. Returns the URL gh printed.
pub fn create_pr(dir: &Path, gh: &GitHub, title: &str, body: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() || title.contains(['\n', '\r', '\0']) || title.chars().count() > MAX_TITLE {
        return Err(invalid(
            "a PR title must be one line of 1 to 256 characters",
        ));
    }
    if body.len() > MAX_BODY || body.contains('\0') {
        return Err(invalid("a PR body must be text under 64 KiB"));
    }
    let root = repo_root(dir)?;
    let branch = current_branch(&root)?;
    let title_arg = format!("--title={title}");
    let args = [
        "pr",
        "create",
        "--head",
        &branch,
        &title_arg,
        "--body-file",
        "-",
    ];
    let out = checked(
        gh_command(gh, &root, &args)?,
        "gh pr create",
        Some(body.as_bytes()),
    )?;
    lossy(&out.stdout)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .filter(|l| {
            l.starts_with("https://") && l.len() <= MAX_URL && !l.contains(char::is_whitespace)
        })
        .map(str::to_owned)
        .ok_or_else(|| Error::Gh {
            args: "pr create".into(),
            stderr: "gh printed no pull request URL".into(),
        })
}

/// The github.com login active in `gh.config_dir`, `None` when nobody is logged in there.
pub fn gh_login(gh: &GitHub) -> Result<Option<String>> {
    let args = [
        "auth",
        "status",
        "--active",
        "--hostname",
        "github.com",
        "--json",
        "hosts",
    ];
    let out = checked(
        gh_command(gh, gh.config_dir, &args)?,
        "gh auth status",
        None,
    )?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| Error::Gh {
        args: "auth status".into(),
        stderr: format!("unreadable output: {e}"),
    })?;
    let accounts = json["hosts"]["github.com"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(accounts
        .iter()
        .find(|a| a["active"] == true && a["state"] == "success")
        .and_then(|a| a["login"].as_str())
        .map(str::to_owned))
}

/// `gh` in `dir` with nothing of the parent's GitHub env (see `command`), only the bound config dir.
fn gh_command(gh: &GitHub, dir: &Path, args: &[&str]) -> Result<Command> {
    let mut cmd = Command::new(resolve(gh.gh)?);
    for var in crate::PARENT_LOGIN_VARS {
        cmd.env_remove(var);
    }
    cmd.args(args)
        .current_dir(dir)
        .env("GH_CONFIG_DIR", gh.config_dir)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    Ok(cmd)
}

fn current_branch(root: &Path) -> Result<String> {
    let out = crate::output(
        Path::new("git"),
        root,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    if !out.status.success() {
        return Err(invalid("HEAD is detached; check out a branch first"));
    }
    let branch = lossy(&out.stdout).trim().to_owned();
    if branch.starts_with('-')
        || !git_succeeds(root, &["check-ref-format", &format!("refs/heads/{branch}")])?
    {
        return Err(Error::InvalidBranch(branch));
    }
    Ok(branch)
}

/// Runs `cmd` with `input` on stdin; a non-zero exit is an error carrying stderr.
fn checked(mut cmd: Command, what: &str, input: Option<&[u8]>) -> Result<Output> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound if what.starts_with("gh") => Error::GhNotInstalled,
        io::ErrorKind::NotFound => Error::GitNotInstalled,
        _ => Error::Io(e),
    })?;
    let stdin = child.stdin.take();
    let out = std::thread::scope(|s| {
        if let (Some(mut stdin), Some(input)) = (stdin, input) {
            // A child that exits without reading closes the pipe; its exit status tells what went wrong.
            s.spawn(move || match stdin.write_all(input) {
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                other => other,
            });
        }
        child.wait_with_output()
    })?;
    if out.status.success() {
        return Ok(out);
    }
    let stderr = lossy(&out.stderr).trim().to_owned();
    Err(if what.starts_with("gh") {
        Error::Gh {
            args: what.into(),
            stderr,
        }
    } else {
        Error::Git {
            args: what.into(),
            stderr,
        }
    })
}

/// An absolute path as is; a bare name through the absolute entries of `PATH`, so a project's cwd can't supply it.
fn resolve(bin: &Path) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    if bin.components().count() != 1 {
        return Err(invalid(format!(
            "{} must be an absolute path or a bare name",
            bin.display()
        )));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .flat_map(|d| {
            let exe = cfg!(windows).then(|| d.join(bin).with_extension("exe"));
            std::iter::once(d.join(bin)).chain(exe)
        })
        .find(|p| p.is_file())
        .ok_or(Error::GhNotInstalled)
}

fn invalid(why: impl Into<String>) -> Error {
    Error::Invalid(why.into())
}
