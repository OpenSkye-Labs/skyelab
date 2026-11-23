//! Which binary a launched agent is. The env it inherits is `skyelab_core::env`'s rule.

use crate::Mode;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Turns `bin` into an absolute path before the child's cwd (an untrusted project) can matter.
/// A bare name is looked up through the parent PATH's absolute entries only.
pub fn resolve_bin(bin: &Path) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    if bin.components().count() != 1 {
        bail!("{} must be an absolute path or a bare name", bin.display());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    skyelab_core::env::absolute_entries(&path)
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

pub(crate) fn claude_args(
    cmd: &mut Command,
    mcp_config: Option<&Path>,
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
        "--strict-mcp-config",
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
    // Our own server only (--strict-mcp-config keeps every other out); its tools enforce their own rules.
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

/// Claude Code's session ids and Codex's thread ids are UUIDs; allowing only id-safe characters keeps anything flag-like off the command line.
pub(crate) fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
