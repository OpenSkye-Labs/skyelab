//! Which binary a launched agent is. The env it inherits is `skyelab_core::env`'s rule.

use crate::Mode;
use anyhow::{Context as _, Result, bail};
use serde_json::json;
use skyelab_core::{Account, AccountsConfig};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Set on every background Claude call (title, commit and PR text, usage poll, login check): they run strict and need
/// none of the account's claude.ai connectors, which agent turns load.
pub(crate) const NO_CLAUDEAI_MCP: (&str, &str) = ("ENABLE_CLAUDEAI_MCP_SERVERS", "false");

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
    let dirs = skyelab_core::env::absolute_entries(&skyelab_core::env::parent_path());
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
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} not found in PATH ({})",
                bin.display(),
                std::env::join_paths(path_dirs)
                    .unwrap_or_default()
                    .to_string_lossy()
            )
        })
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

/// `deny_mcp`: `None` keeps `--strict-mcp-config` (only the `--mcp-config` file's servers); `Some(names)` lets Claude
/// load the account's own MCP config, plugins and claude.ai connectors too, refusing those names (ones that would read
/// as SkyeLab's server). Never the repo's: `--setting-sources user` keeps `.mcp.json` and local-scope servers out.
pub(crate) fn claude_args(
    cmd: &mut Command,
    mcp_config: Option<&Path>,
    deny_mcp: Option<&[String]>,
    resume: Option<&str>,
    mode: Mode,
) {
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Text and thinking as they stream, as `Event::Delta`.
        "--include-partial-messages",
        // `-p` skips the trust dialog, so the project's own settings and .mcp.json must not load.
        "--setting-sources",
        "user",
        // Every permission ask comes to us as a `can_use_tool` control request, answered in the thread.
        "--permission-prompt-tool",
        "stdio",
    ])
    // Always named, so the account's own `defaultMode` setting can't quietly loosen Ask.
    .args(match mode {
        Mode::Default | Mode::Plan | Mode::AcceptEdits => vec!["--permission-mode", mode.claude()],
        Mode::Bypass => vec![
            "--permission-mode",
            mode.claude(),
            "--allow-dangerously-skip-permissions",
        ],
    })
    .args(match deny_mcp {
        None => vec!["--strict-mcp-config".to_owned()],
        Some([]) => Vec::new(),
        Some(names) => {
            let denied: Vec<_> = names.iter().map(|n| json!({ "serverName": n })).collect();
            vec![
                "--settings".to_owned(),
                json!({ "deniedMcpServers": denied }).to_string(),
            ]
        }
    })
    // Only our own server's tools are pre-allowed; they enforce their own rules. Every other server's tools ask.
    .args(mcp_config.into_iter().flat_map(|path| {
        [
            "--mcp-config".as_ref(),
            path.as_os_str(),
            "--allowedTools".as_ref(),
            "mcp__skyelab".as_ref(),
        ]
    }))
    .args(resume.into_iter().flat_map(|id| ["--resume", id]));
}

#[cfg(test)]
mod tests {
    use super::pathext_candidates;
    use std::path::{Path, PathBuf};

    /// A Dock-launched app finds a CLI that only the login shell's PATH has.
    #[cfg(unix)]
    #[test]
    fn resolve_bin_finds_a_cli_only_on_the_captured_path() {
        use std::ffi::OsString;
        use std::os::unix::fs::PermissionsExt as _;
        let home = tempfile::tempdir().expect("tempdir");
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let claude = bin.join("claude-skyelab-test");
        std::fs::write(&claude, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let name = Path::new("claude-skyelab-test");
        let gui = OsString::from("/usr/bin:/bin");
        let dirs = |p: &OsString| skyelab_core::env::absolute_entries(p);
        let err = super::resolve_bin_in(name, &dirs(&gui)).expect_err("not on the GUI PATH");
        assert!(err.to_string().contains("/usr/bin:/bin"), "{err}");
        let captured = [(OsString::from("PATH"), bin.clone().into_os_string())];
        let merged = skyelab_core::login_shell::merge([("PATH".into(), gui)], &captured, &[]);
        let path = merged
            .into_iter()
            .find(|(k, _)| k == "PATH")
            .expect("PATH")
            .1;
        assert_eq!(
            super::resolve_bin_in(name, &dirs(&path)).expect("found"),
            claude
        );
    }

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
