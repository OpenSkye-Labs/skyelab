//! macOS: copying the new app out of the release's disk image and checking its code signature.

use super::{beside, remove_all};
use anyhow::{Context as _, anyhow, bail};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What `codesign` and `spctl` say about a bundle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signature {
    /// `codesign --verify --deep --strict` passed.
    pub valid: bool,
    /// The Developer ID team; `None` for an unsigned or ad-hoc signed build.
    pub team: Option<String>,
    /// Gatekeeper (`spctl --assess`) accepts it: signed with a Developer ID and notarized.
    pub accepted: bool,
    /// `CFBundleIdentifier` from its `Info.plist`; `None` when it can't be read.
    pub bundle_id: Option<String>,
}

const HDIUTIL: &str = "/usr/bin/hdiutil";
const DITTO: &str = "/usr/bin/ditto";
const CODESIGN: &str = "/usr/bin/codesign";
const SPCTL: &str = "/usr/sbin/spctl";
const PLUTIL: &str = "/usr/bin/plutil";

/// A tool that runs longer than this (a stuck mount, `spctl` waiting on the network) fails the update.
pub(super) const TOOL_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

fn tool(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear().envs(crate::env::inherited_agent_env(false));
    cmd
}

/// Runs a tool to completion; its stderr becomes the error.
fn run(cmd: &mut Command) -> anyhow::Result<std::process::Output> {
    let out = crate::process::output_within(cmd, None, TOOL_LIMIT)
        .with_context(|| format!("couldn't run {}", cmd.get_program().to_string_lossy()))?;
    if !out.status.success() {
        bail!(
            "{} failed: {}",
            cmd.get_program().to_string_lossy(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out)
}

/// Copies the `.app` out of `dmg` next to `target` and checks its signature, then detaches the image.
pub(super) fn stage_app(dmg: &Path, target: &Path) -> anyhow::Result<PathBuf> {
    let mount = tempfile::Builder::new()
        .prefix("skyelab-update")
        .tempdir()?;
    run(tool(HDIUTIL)
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(mount.path())
        .arg(dmg))?;
    let new = beside(target, "update")?;
    let copied = single_app(mount.path()).and_then(|app| {
        remove_all(&new);
        run(tool(DITTO).arg(&app).arg(&new))
            .with_context(|| format!("couldn't write next to {}", target.display()))
    });
    let _ = run(tool(HDIUTIL).arg("detach").arg(mount.path()).arg("-force"));
    let checked = copied.and_then(|_| check_app(target, &new));
    if let Err(e) = checked {
        remove_all(&new);
        return Err(e);
    }
    let _ = fs::remove_file(dmg);
    Ok(new)
}

/// The one `.app` folder at the image's root (the `Applications` link is skipped).
fn single_app(root: &Path) -> anyhow::Result<PathBuf> {
    let mut apps = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension() == Some(OsStr::new("app")))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()));
    match (apps.next(), apps.next()) {
        (Some(app), None) => Ok(app.path()),
        _ => bail!("the disk image doesn't hold exactly one app"),
    }
}

/// Whether the bundle `new` may replace the installed `target` ([`signature_policy`]).
pub(super) fn check_app(target: &Path, new: &Path) -> anyhow::Result<()> {
    let ok = |cmd: &mut Command| {
        crate::process::output_within(cmd, None, TOOL_LIMIT).is_ok_and(|o| o.status.success())
    };
    let team = |app: &Path| -> anyhow::Result<Option<String>> {
        let out =
            crate::process::output_within(tool(CODESIGN).arg("-dv").arg(app), None, TOOL_LIMIT)
                .context("couldn't run codesign")?;
        installed_team(out.status.success(), &String::from_utf8_lossy(&out.stderr))
            .map_err(|e| anyhow!(e))
    };
    let bundle_id = |app: &Path| {
        run(tool(PLUTIL)
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(app.join("Contents").join("Info.plist")))
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|id| !id.is_empty())
    };
    let current = Signature {
        team: team(target)?,
        bundle_id: bundle_id(target),
        ..Default::default()
    };
    let new_sig = Signature {
        valid: ok(tool(CODESIGN)
            .args(["--verify", "--deep", "--strict"])
            .arg(new)),
        team: team(new).unwrap_or(None),
        accepted: ok(tool(SPCTL).args(["--assess", "--type", "execute"]).arg(new)),
        bundle_id: bundle_id(new),
    };
    signature_policy(&current, &new_sig).map_err(|e| anyhow!(e))
}

/// The installed app's team from `codesign -dv`: `None` only when it says the app isn't signed (or is ad-hoc); any
/// other failure is an error, so a signed install never reads as unsigned.
pub fn installed_team(success: bool, codesign_dv: &str) -> Result<Option<String>, String> {
    if success {
        return Ok(team_id(codesign_dv));
    }
    if codesign_dv.contains("is not signed at all") {
        return Ok(None);
    }
    Err(format!(
        "couldn't read this SkyeLab's signature: {}",
        codesign_dv.trim()
    ))
}

/// The `TeamIdentifier=` in `codesign -dv`'s output; `None` when "not set" (ad-hoc) or missing.
pub fn team_id(codesign_dv: &str) -> Option<String> {
    codesign_dv
        .lines()
        .find_map(|l| l.strip_prefix("TeamIdentifier="))
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != "not set")
        .map(str::to_owned)
}

/// Whether `new` may replace `current`: always the same app (`CFBundleIdentifier`, both readable); for a signed
/// install also a valid, Gatekeeper-accepted build from the same team. An unsigned install (a local or unsigned
/// release build) otherwise relies on the checksum alone.
pub fn signature_policy(current: &Signature, new: &Signature) -> Result<(), String> {
    if current.bundle_id.is_none() || new.bundle_id != current.bundle_id {
        return Err("the update is a different app (its bundle identifier doesn't match)".into());
    }
    let Some(team) = &current.team else {
        return Ok(());
    };
    if new.team.is_none() {
        return Err("the update isn't signed; this SkyeLab is, so it won't be replaced".into());
    }
    if !new.valid {
        return Err("the update's code signature doesn't verify".into());
    }
    if new.team.as_ref() != Some(team) {
        return Err("the update is signed by a different developer".into());
    }
    if !new.accepted {
        return Err("macOS Gatekeeper doesn't accept the update (not notarized)".into());
    }
    Ok(())
}
