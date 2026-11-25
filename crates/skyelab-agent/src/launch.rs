//! Which binary a launched agent is. The env it inherits is `skyelab_core::env`'s rule.

use anyhow::{Context as _, Result, bail};
use skyelab_core::{Account, AccountsConfig};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The agent's command for `account` in `dir`, checked and scrubbed: `dir` (canonicalized, returned) must still resolve
/// to `account`, and the env is the allowlist plus the account's own, never the parent's.
pub(crate) fn agent_command(
    bin: &Path,
    config: &AccountsConfig,
    account: &Account,
    dir: &Path,
) -> Result<(Command, PathBuf, PathBuf)> {
    // The child starts in the exact folder that was checked, even if a link in `dir` is retargeted.
    let dir = std::fs::canonicalize(dir).with_context(|| format!("opening {}", dir.display()))?;
    match config.resolve(&dir) {
        Some(now) if now.same_login(account) => {}
        Some(now) if now.id == account.id => bail!(
            "account '{}' changed since this launch was prepared (its config dir, gh login or ssh setting); \
             nothing was started",
            now.id
        ),
        Some(now) => bail!(
            "{} is now bound to '{}', not '{}'; nothing was started",
            dir.display(),
            now.id,
            account.id
        ),
        None => bail!("{} is not bound to an account", dir.display()),
    }
    let env = account.launch_env()?;
    // A relative dir would resolve inside the untrusted project, and an empty one means ~/.claude.
    if let Some((var, v)) = env.iter().find(|(_, v)| !v.is_absolute()) {
        bail!(
            "{var} for {} must be an absolute path, got {v:?}",
            dir.display()
        );
    }
    let bin = if account.provider == skyelab_core::Provider::Cursor {
        crate::cursor::resolve_cursor_bin(bin)?
    } else {
        resolve_bin(bin)?
    };
    let mut cmd = Command::new(&bin);
    // Never inherit the parent's account or credentials: start empty, add the allowlist, then the bound account,
    // and git's https credentials from that account's gh only (`git push` in the agent's own shell).
    cmd.env_clear()
        .envs(skyelab_core::env::inherited_agent_env(account.ssh_agent))
        .envs(env)
        .envs(skyelab_core::env::account_git_env(account, &dir));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0); // so kill() reaches its children too
    Ok((cmd, bin, dir))
}

/// SIGKILL to process group `pid` (an agent started with `process_group(0)`, so the group id is its pid). `unsafe` is
/// denied, so no libc::killpg: the kill binary signals the group instead.
#[cfg(unix)]
pub(crate) fn kill_group(pid: u32) {
    let kill = resolve_bin(Path::new("kill")).unwrap_or_else(|_| "/bin/kill".into());
    let _ = skyelab_core::process::spawn(
        Command::new(kill)
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()),
    )
    .and_then(|mut c| c.wait());
}

/// SIGINT to process group `pid` (for interrupting a one-turn-per-process agent like Cursor).
#[cfg(unix)]
pub(crate) fn interrupt_group(pid: u32) {
    let kill = resolve_bin(Path::new("kill")).unwrap_or_else(|_| "/bin/kill".into());
    let _ = skyelab_core::process::spawn(
        Command::new(kill)
            .args(["-INT", "--", &format!("-{pid}")])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()),
    )
    .and_then(|mut c| c.wait());
}

/// Turns `bin` into an absolute path before the child's cwd (an untrusted project) can matter.
/// A bare name is looked up through the parent PATH's absolute entries only.
pub fn resolve_bin(bin: &Path) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs = skyelab_core::env::absolute_entries(&path);
    resolve_bin_in(bin, &dirs)
}

/// Resolves `bin` against an explicit list of absolute PATH directories.
pub(crate) fn resolve_bin_in(bin: &Path, path_dirs: &[PathBuf]) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    if bin.components().count() != 1 {
        bail!("{} must be an absolute path or a bare name", bin.display());
    }
    path_dirs
        .iter()
        .flat_map(|dir| candidates(dir, bin))
        .find(|p| is_executable(p))
        .ok_or_else(|| anyhow::anyhow!("{} not found in PATH", bin.display()))
}

#[cfg(unix)]
fn candidates(dir: &Path, bin: &Path) -> Vec<PathBuf> {
    vec![dir.join(bin)]
}

#[cfg(windows)]
fn candidates(dir: &Path, bin: &Path) -> Vec<PathBuf> {
    pathext_candidates(dir, bin, std::env::var("PATHEXT").ok().as_deref())
}

/// Like Windows itself: a name without an extension only matches with a PATHEXT one, so npm's
/// extensionless sh shim next to `claude.cmd` is never picked.
#[cfg(any(windows, test))]
fn pathext_candidates(dir: &Path, bin: &Path, pathext: Option<&str>) -> Vec<PathBuf> {
    if bin.extension().is_some() {
        return vec![dir.join(bin)];
    }
    pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| dir.join(format!("{}{e}", bin.display())))
        .collect()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::pathext_candidates;
    use std::path::{Path, PathBuf};

    #[test]
    fn pathext_candidates_follow_pathext_and_skip_bare_names() {
        let dir = Path::new("bin");
        let got =
            |bin: &str, pathext: Option<&str>| pathext_candidates(dir, Path::new(bin), pathext);
        assert_eq!(
            got("claude", Some(".EXE;;.CMD")),
            [dir.join("claude.EXE"), dir.join("claude.CMD")]
        );
        // a name with an extension is tried as is, never with PATHEXT appended
        assert_eq!(got("claude.cmd", Some(".EXE")), [dir.join("claude.cmd")]);
        // no PATHEXT: Windows' own default list, still never the bare (sh shim) name
        let default: Vec<PathBuf> = got("claude", None);
        assert_eq!(default.len(), 4);
        assert!(!default.contains(&dir.join("claude")));
        assert!(default.contains(&dir.join("claude.CMD")));
    }
}
