//! Commit, push and open a pull request, each under one account's `gh` login.

use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Error, Result, command, git, git_succeeds, hooked_command, lossy, repo_root};

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
/// The repo's hooks run, with the scrubbed env plus `account_env` (the bound account's `launch_env`).
///
/// With `reviewed`, the [`snapshot_tree`] the user looked at: nothing is committed unless the working tree still
/// matches it (for an empty `paths`, the staged tree must equal it exactly). Either way, a commit that touches a path
/// outside `paths` (or, committing everything, outside what changed between `HEAD` and `reviewed`) is undone: a
/// pre-commit hook, which an agent can write, may `git add` anything.
pub fn commit(
    dir: &Path,
    message: &str,
    paths: &[PathBuf],
    account_env: &[(&str, PathBuf)],
    reviewed: Option<&str>,
) -> Result<String> {
    if message.trim().is_empty() || message.contains('\0') || message.len() > MAX_MESSAGE {
        return Err(invalid(
            "a commit message must be non-empty text under 64 KiB",
        ));
    }
    let root = repo_root(dir)?;
    let changed = || {
        invalid(
            "the files changed since you looked; nothing was committed. Check the Changes tab again, then commit",
        )
    };
    if let Some(reviewed) = reviewed
        && snapshot_tree(&root)? != reviewed
    {
        return Err(changed());
    }
    let before = head(&root)?;
    // Literal, so a ticked file named `*` is that file, not a glob.
    let literal: &OsStr = "--literal-pathspecs".as_ref();
    let mut commit: Vec<&OsStr> = vec![literal, "commit".as_ref(), "-F".as_ref(), "-".as_ref()];
    let allowed: Option<Vec<String>> = if paths.is_empty() {
        git(&root, &["add", "-A"])?;
        match reviewed {
            Some(reviewed) => {
                if lossy(&git(&root, &["write-tree"])?).trim() != reviewed {
                    return Err(changed());
                }
                Some(changed_paths(&root, before.as_deref(), reviewed)?)
            }
            None => None,
        }
    } else {
        // A staged deletion or a rename's old path is in neither the index nor the tree, so `add` would refuse
        // it; `commit -- <path>` still records it.
        let add: Vec<&OsStr> = [literal, "add".as_ref(), "-A".as_ref(), "--".as_ref()]
            .into_iter()
            .chain(present(&root, paths)?.into_iter().map(|p| p.as_os_str()))
            .collect();
        if add.len() > 4 {
            git(&root, &add)?;
        }
        commit.push("--".as_ref());
        commit.extend(paths.iter().map(|p| p.as_os_str()));
        Some(
            paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        )
    };
    let mut cmd = hooked_command(Path::new("git"), &root);
    cmd.args(&commit)
        .envs(account_env.iter().map(|(k, v)| (k, v)));
    checked(cmd, "git commit", Some(message.as_bytes()))?;
    let sha = lossy(&git(&root, &["rev-parse", "HEAD"])?)
        .trim()
        .to_owned();
    if let Some(allowed) = allowed {
        let extra: Vec<String> = changed_paths(&root, before.as_deref(), &sha)?
            .into_iter()
            .filter(|p| !allowed.contains(p))
            .collect();
        if !extra.is_empty() {
            match &before {
                Some(b) => git(&root, &["reset", "-q", "--soft", b])?,
                None => git(&root, &["update-ref", "-d", "HEAD"])?,
            };
            return Err(invalid(format!(
                "a commit hook added files you didn't review ({}); the commit was undone and they are staged",
                extra.join(", ")
            )));
        }
    }
    Ok(sha)
}

/// `HEAD`'s commit, `None` in a repo with no commits yet.
fn head(root: &Path) -> Result<Option<String>> {
    let out = crate::output(
        Path::new("git"),
        root,
        &["rev-parse", "--verify", "-q", "HEAD"],
    )?;
    Ok(out
        .status
        .success()
        .then(|| lossy(&out.stdout).trim().to_owned()))
}

/// The paths that differ between commit (or tree) `from` (`None`: nothing) and `to`.
fn changed_paths(root: &Path, from: Option<&str>, to: &str) -> Result<Vec<String>> {
    let out = match from {
        Some(from) => git(
            root,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--name-only",
                "--no-renames",
                from,
                to,
            ],
        )?,
        None => git(
            root,
            &["ls-tree", "-r", "-z", "--name-only", "--full-tree", to],
        )?,
    };
    Ok(lossy(&out)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The `paths` that are on disk or in the index, so `git add` accepts them.
fn present<'p>(root: &Path, paths: &'p [PathBuf]) -> Result<Vec<&'p PathBuf>> {
    let literal: &OsStr = "--literal-pathspecs".as_ref();
    let args: Vec<&OsStr> = [literal, "ls-files".as_ref(), "-z".as_ref(), "--".as_ref()]
        .into_iter()
        .chain(paths.iter().map(|p| p.as_os_str()))
        .collect();
    let indexed = lossy(&git(root, &args)?);
    let indexed: Vec<&str> = indexed.split('\0').collect();
    Ok(paths
        .iter()
        .filter(|p| {
            std::fs::symlink_metadata(root.join(p)).is_ok()
                || indexed.contains(&p.to_string_lossy().as_ref())
        })
        .collect())
}

/// The tree id of everything the working tree of `dir`'s repo holds now (untracked files included, ignored ones
/// not), as a commit of all of it would record. Built in a scratch index in a fresh private dir (never in `.git`,
/// which an agent can write), so the real index is untouched.
pub fn snapshot_tree(dir: &Path) -> Result<String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = repo_root(dir)?;
    let index = PathBuf::from(
        lossy(&git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )?)
        .trim_end_matches('\n'),
    );
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let scratch_dir = std::env::temp_dir().join(format!(
        "skyelab-snapshot-{}-{n}-{nanos}",
        std::process::id()
    ));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&scratch_dir)?; // exclusive: fails rather than reuse a dir someone else made
    let scratch = scratch_dir.join("index");
    let tree = (|| {
        // Starting from the real index keeps its stat cache, so unchanged files aren't hashed again.
        match std::fs::copy(&index, &scratch) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let run = |args: &[&str]| -> Result<Vec<u8>> {
            let mut cmd = command(Path::new("git"), &root);
            cmd.args(args).env("GIT_INDEX_FILE", &scratch);
            crate::finish(&root, args, cmd.output()?)
        };
        run(&["add", "-A"]).and_then(|_| run(&["write-tree"]))
    })();
    let _ = std::fs::remove_dir_all(&scratch_dir);
    Ok(lossy(&tree?).trim().to_owned())
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
    // Only an upstream of the same name: `switch -c fix origin/main` tracks main, and pushing there would land on main.
    let remote = match (
        config(format!("branch.{branch}.remote"))?,
        config(format!("branch.{branch}.merge"))?,
    ) {
        (Some(remote), Some(merge)) if remote != "." && merge == format!("refs/heads/{branch}") => {
            Some(remote)
        }
        _ => None,
    };
    let set_upstream = remote.is_none();
    let remote = remote.unwrap_or_else(|| "origin".into());
    if remote.starts_with('-') {
        return Err(invalid(format!(
            "SkyeLab won't push to a remote named {remote}"
        )));
    }
    let urls = check_push_urls(&root, &remote)?;
    let urls: Vec<&str> = urls.iter().map(String::as_str).collect();
    let refspec = format!("HEAD:refs/heads/{branch}");
    let mut cmd = hooked_command(Path::new("git"), &root);
    cmd.args(credential_args(gh, &urls)?)
        .arg("push")
        // A submodule's own remote is never checked, so never pushed to.
        .arg("--recurse-submodules=no")
        .args(set_upstream.then_some("--set-upstream"))
        .args([&remote, &refspec])
        .env("GH_CONFIG_DIR", gh.config_dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    checked(cmd, "git push", None)?;
    Ok(format!("{remote}/{branch}"))
}

/// Every URL `remote` pushes to (git pushes to all of them), each only an https URL without credentials (the bound
/// gh answers) or a local path (nothing to log in to). An ssh key, a token in the URL or a rewrite to either would
/// push as someone no account binds.
fn check_push_urls(root: &Path, remote: &str) -> Result<Vec<String>> {
    let out = lossy(&git(
        root,
        &["remote", "get-url", "--push", "--all", remote],
    )?);
    let urls: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_owned)
        .collect();
    let plain = |url: &str| {
        let https = url.strip_prefix("https://").is_some_and(|rest| {
            let authority = rest.split('/').next().unwrap_or_default();
            // `=` would end the key of the `-c http.<url>.extraHeader=` reset.
            !authority.is_empty() && !authority.contains('@') && !url.contains('=')
        });
        https || url.starts_with("file://") || Path::new(url).is_absolute()
    };
    if !urls.is_empty() && urls.iter().all(|u| plain(u)) {
        return Ok(urls);
    }
    // The URL itself isn't shown: it may hold a token.
    Err(invalid(format!(
        "a push URL of {remote} isn't a plain https:// URL. SkyeLab pushes only over https, where the account's \
         gh login answers; an ssh key or a token in the URL can't be bound to an account"
    )))
}

/// The `-c` args that make git ask only the bound `gh` for https credentials: the empty helper drops every
/// helper from the user's and system config (the keychain's login among them), then `gh auth git-credential`.
/// `http.extraHeader` is cleared generically and scoped to each https URL in `push_urls`: a header scoped to the
/// exact URL is the most specific, so no other config header can win over the reset.
pub fn credential_args(gh: &GitHub, push_urls: &[&str]) -> Result<Vec<String>> {
    let bin = resolve(gh.gh)?;
    let bin = bin.to_str().filter(|b| !b.contains('\'')).ok_or_else(|| {
        invalid(format!(
            "can't use {} as a credential helper",
            bin.display()
        ))
    })?;
    // git runs a `!` helper through the shell, so the path is single-quoted.
    let mut args: Vec<String> = vec![
        "-c".into(),
        "credential.helper=".into(),
        "-c".into(),
        "http.extraHeader=".into(),
        // No askpass from the user's config: if gh can't answer, nobody does.
        "-c".into(),
        "core.askPass=".into(),
        "-c".into(),
        format!("credential.helper=!'{bin}' auth git-credential"),
    ];
    for url in push_urls.iter().filter(|u| u.starts_with("https://")) {
        args.extend(["-c".into(), format!("http.{url}.extraHeader=")]);
    }
    Ok(args)
}

/// Opens a pull request for the current branch with `gh pr create`; the base is gh's default (the repo's
/// default branch). The body goes through stdin. Returns the URL gh printed.
pub fn create_pr(dir: &Path, gh: &GitHub, title: &str, body: &str) -> Result<String> {
    let one_line = !title.contains(['\n', '\r', '\0']);
    let title = title.trim();
    if title.is_empty() || !one_line || title.chars().count() > MAX_TITLE {
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
    if !gh.config_dir.is_dir() {
        return Ok(None); // gh creates it at the first login
    }
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

/// `gh` in `dir` with the scrubbed env (see `hooked_command`) and the bound config dir.
fn gh_command(gh: &GitHub, dir: &Path, args: &[&str]) -> Result<Command> {
    let mut cmd = Command::new(resolve(gh.gh)?);
    cmd.env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false))
        .args(args)
        .current_dir(dir)
        .env("GH_CONFIG_DIR", gh.config_dir)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    Ok(cmd)
}

fn current_branch(root: &Path) -> Result<String> {
    let out = crate::output(Path::new("git"), root, &["symbolic-ref", "--quiet", "HEAD"])?;
    let full = lossy(&out.stdout).trim().to_owned();
    // Not `--short`: with a tag of the same name it answers `heads/<branch>`.
    let Some(branch) = full
        .strip_prefix("refs/heads/")
        .filter(|_| out.status.success())
    else {
        return Err(invalid("HEAD is detached; check out a branch first"));
    };
    let branch = branch.to_owned();
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
