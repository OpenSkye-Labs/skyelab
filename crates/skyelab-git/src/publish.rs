//! Push and open a pull request under one account's `gh` login.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::{Error, Result, git, git_succeeds, hooked_command, lossy, repo_root};

pub(crate) const MAX_MESSAGE: usize = 64 * 1024;
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

/// Pushes the current branch to its upstream, or to `origin/<branch>` (and sets that as the upstream).
/// Returns `<remote>/<branch>`. https credentials come only from `gh` (see [`credential_args`]).
pub fn push(dir: &Path, gh: &GitHub) -> Result<String> {
    push_with(dir, gh, &[])
}

/// [`push`], with `env` added for the repo's hooks (the bound account's git credential binding).
pub fn push_with(dir: &Path, gh: &GitHub, env: &[(OsString, OsString)]) -> Result<String> {
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
    let urls = check_urls(&root, &remote, true)?;
    crate::remote::check_repo_config(&root, "push")?;
    let urls: Vec<&str> = urls.iter().map(String::as_str).collect();
    let refspec = format!("HEAD:refs/heads/{branch}");
    let mut cmd = hooked_command(Path::new("git"), &root);
    cmd.args(credential_args(gh, &urls)?)
        .arg("push")
        // A submodule's own remote is never checked, so never pushed to.
        .arg("--recurse-submodules=no")
        .args(set_upstream.then_some("--set-upstream"))
        .args([&remote, &refspec])
        .envs(env.iter().map(|(k, v)| (k, v)))
        .envs(crate::test_ca_env())
        .env("GH_CONFIG_DIR", gh.config_dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    checked(cmd, "git push", None)?;
    Ok(format!("{remote}/{branch}"))
}

/// Every URL `remote` pushes to (git pushes to all of them), or fetches from without `push`, each only an https URL
/// without credentials (the bound gh answers) or a local path (nothing to log in to). An ssh key, a token in the URL
/// or a rewrite to either would act as someone no account binds.
pub(crate) fn check_urls(root: &Path, remote: &str, push: bool) -> Result<Vec<String>> {
    let mut args = vec!["remote", "get-url"];
    args.extend(push.then_some("--push"));
    args.extend(["--all", remote]);
    let out = lossy(&git(root, &args)?);
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
        // Not a UNC path (`\\host\share`, `file://host/`): Windows would log in to that host with the OS user's hash.
        let unc = url.starts_with("\\\\") || url.starts_with("//");
        https || url.starts_with("file:///") || (Path::new(url).is_absolute() && !unc)
    };
    if !urls.is_empty() && urls.iter().all(|u| plain(u)) {
        return Ok(urls);
    }
    // The URL itself isn't shown: it may hold a token.
    Err(invalid(format!(
        "a {} URL of {remote} isn't a plain https:// URL. SkyeLab pushes and fetches only over https, where the \
         account's gh login answers; an ssh key or a token in the URL can't be bound to an account",
        if push { "push" } else { "fetch" }
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
        // A repo config turning TLS checks off would hand the token to whoever answers.
        args.extend(["-c".into(), format!("http.{url}.sslVerify=true")]);
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
pub(crate) fn gh_command(gh: &GitHub, dir: &Path, args: &[&str]) -> Result<Command> {
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

pub(crate) fn current_branch(root: &Path) -> Result<String> {
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
pub(crate) fn checked(cmd: Command, what: &str, input: Option<&[u8]>) -> Result<Output> {
    checked_within(cmd, what, input, None)
}

/// [`checked`], killed after `limit` when one is given.
pub(crate) fn checked_within(
    mut cmd: Command,
    what: &str,
    input: Option<&[u8]>,
    limit: Option<std::time::Duration>,
) -> Result<Output> {
    let out = match limit {
        Some(limit) => skyelab_core::process::output_within(&mut cmd, input, limit),
        None => skyelab_core::process::output(&mut cmd, input),
    };
    let out = out.map_err(|e| match e.kind() {
        io::ErrorKind::NotFound if what.starts_with("gh") => Error::GhNotInstalled,
        io::ErrorKind::NotFound => Error::GitNotInstalled,
        _ => Error::Io(e),
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
    let pathext = std::env::var("PATHEXT").ok();
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .flat_map(|d| candidates(&d, bin, cfg!(windows), pathext.as_deref()))
        .find(|p| p.is_file())
        .ok_or(Error::GhNotInstalled)
}

/// Like Windows itself: there, a name without an extension only matches with a PATHEXT one, so an extensionless sh
/// shim is never picked (the same rule as skyelab-agent's `claude` lookup).
fn candidates(dir: &Path, bin: &Path, windows: bool, pathext: Option<&str>) -> Vec<PathBuf> {
    if !windows || bin.extension().is_some() {
        return vec![dir.join(bin)];
    }
    pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| dir.join(format!("{}{e}", bin.display())))
        .collect()
}

pub(crate) fn invalid(why: impl Into<String>) -> Error {
    Error::Invalid(why.into())
}

#[cfg(test)]
mod tests {
    use super::candidates;
    use std::path::{Path, PathBuf};

    #[test]
    fn gh_candidates_follow_pathext_on_windows_only() {
        let dir = Path::new("bin");
        let got = |bin: &str, windows: bool, pathext: Option<&str>| -> Vec<PathBuf> {
            candidates(dir, Path::new(bin), windows, pathext)
        };
        assert_eq!(got("gh", false, Some(".EXE")), [dir.join("gh")]);
        // Windows: never the extensionless name (a sh shim), and .cmd shims are found too
        assert_eq!(
            got("gh", true, Some(".EXE;;.CMD")),
            [dir.join("gh.EXE"), dir.join("gh.CMD")]
        );
        assert!(!got("gh", true, None).contains(&dir.join("gh")));
        assert_eq!(got("gh.exe", true, Some(".CMD")), [dir.join("gh.exe")]);
    }
}
