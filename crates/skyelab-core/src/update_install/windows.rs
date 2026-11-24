//! Windows: the installer's Authenticode check, and the script that runs it once SkyeLab has quit.

use anyhow::{Context as _, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where a failed install leaves its note (in the updates folder), shown by the next start.
pub const FAILURE_MARKER: &str = "update-failed";

/// What `Get-AuthenticodeSignature` says about a file: its `Status` (`Valid`, `NotSigned`, `HashMismatch`, …) and
/// the signer certificate's subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authenticode {
    pub status: String,
    pub subject: String,
}

/// The two lines [`READ_SIGNATURE`] prints.
pub fn parse_authenticode(out: &str) -> Authenticode {
    let mut lines = out.lines().map(str::trim);
    Authenticode {
        status: lines.next().unwrap_or_default().to_owned(),
        subject: lines.next().unwrap_or_default().to_owned(),
    }
}

/// Whether installer `new` may run over the install signed as `current`: an unsigned install relies on the checksum;
/// a signed one takes only a validly signed installer from the same publisher (subject, since the signing service's
/// certificates are short-lived). An installed signature that can't be read refuses the update.
pub fn authenticode_policy(current: &Authenticode, new: &Authenticode) -> Result<(), String> {
    match current.status.as_str() {
        "NotSigned" => return Ok(()),
        "Valid" => {}
        other => return Err(format!("couldn't read this SkyeLab's signature ({other})")),
    }
    if new.status != "Valid" {
        return Err(format!(
            "the installer isn't validly signed ({}); this SkyeLab is, so it won't run",
            new.status
        ));
    }
    if new.subject != current.subject {
        return Err("the installer is signed by a different publisher".into());
    }
    Ok(())
}

/// Prints a file's Authenticode status and signer subject; the path comes in as `SKYELAB_FILE`.
const READ_SIGNATURE: &str = "$s = Get-AuthenticodeSignature -LiteralPath $env:SKYELAB_FILE; \
     $s.Status.ToString(); if ($s.SignerCertificate) { $s.SignerCertificate.Subject } else { '' }";

/// Waits for SkyeLab to quit, runs the installer silently, notes a failure for the next start, then starts SkyeLab
/// (the new one, or the old one when the install failed).
const INSTALL: &str = "Wait-Process -Id $env:SKYELAB_UPDATE_PID -ErrorAction SilentlyContinue; \
     try { $p = Start-Process -Wait -PassThru -FilePath $env:SKYELAB_UPDATE_INSTALLER -ArgumentList '/S'; \
     $code = $p.ExitCode } catch { $code = $_.Exception.Message }; \
     if (\"$code\" -ne '0') { Set-Content -LiteralPath $env:SKYELAB_UPDATE_MARKER \
     -Value ($env:SKYELAB_UPDATE_TAG + \"`nthe installer failed ($code)\") }; \
     Start-Process -FilePath $env:SKYELAB_UPDATE_EXE";

fn powershell() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

/// PowerShell running `script`, from `env_clear()` plus `env`; paths go in as env vars, never into the script.
fn script(script: &str, env: Vec<(std::ffi::OsString, std::ffi::OsString)>) -> Command {
    let mut cmd = Command::new(powershell());
    cmd.env_clear()
        .envs(env)
        .args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn authenticode(file: &Path) -> anyhow::Result<Authenticode> {
    let mut cmd = script(READ_SIGNATURE, crate::env::inherited_agent_env(false));
    cmd.env("SKYELAB_FILE", file);
    let out = crate::process::output_within(&mut cmd, None, super::mac::TOOL_LIMIT)
        .context("couldn't run PowerShell")?;
    Ok(parse_authenticode(&String::from_utf8_lossy(&out.stdout)))
}

/// Whether `installer` may run over the installed `exe` ([`authenticode_policy`]).
pub(super) fn check_installer(exe: &Path, installer: &Path) -> anyhow::Result<()> {
    authenticode_policy(&authenticode(exe)?, &authenticode(installer)?).map_err(|e| anyhow!(e))
}

/// The detached PowerShell that installs `installer` over `exe` once process `pid` has quit ([`INSTALL`]).
pub fn installer_command(
    installer: &Path,
    exe: &Path,
    pid: u32,
    marker: &Path,
    tag: &str,
) -> Command {
    let mut cmd = script(
        INSTALL,
        crate::env::terminal_vars(std::env::vars_os(), cfg!(windows)),
    );
    cmd.env("SKYELAB_UPDATE_PID", pid.to_string())
        .env("SKYELAB_UPDATE_INSTALLER", installer)
        .env("SKYELAB_UPDATE_EXE", exe)
        .env("SKYELAB_UPDATE_MARKER", marker)
        .env("SKYELAB_UPDATE_TAG", tag);
    cmd
}

/// A failure marker's version and reason; `None` when it isn't one (the tag must read as a plain version).
pub fn parse_failure(text: &str) -> Option<(String, String)> {
    let mut lines = text.lines();
    let tag = lines.next()?.trim();
    let plain = |c: char| c.is_ascii_alphanumeric() || ".-+".contains(c);
    if tag.is_empty() || tag.len() > 64 || !tag.chars().all(plain) {
        return None;
    }
    let why: String = lines
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .chars()
        .take(300)
        .collect();
    let why = if why.is_empty() {
        "the installer failed".to_owned()
    } else {
        why
    };
    Some((tag.to_owned(), why))
}
