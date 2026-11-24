//! New projects: `git init` a fresh folder, or clone an https URL with the bound account's gh as the only
//! credential source (or none, for a public repo). Hooks and templates stay off, so nothing in the clone runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::publish::{checked, credential_args, invalid};
use crate::{FIXED_OFF, GitHub, HOOK_EVENTS, Result};

/// git with SkyeLab's fixed safety config (hooks, helpers, askpass, fsmonitor off) and no template dir, so a user's
/// `init.templateDir` hooks are never copied in.
fn fresh(dir: &Path) -> Command {
    let mut cmd = crate::hooked_command(Path::new("git"), dir);
    for (k, v) in FIXED_OFF {
        cmd.arg("-c").arg(format!("{k}={v}"));
    }
    for event in HOOK_EVENTS {
        cmd.arg("-c").arg(format!("hook.{event}.enabled=false"));
    }
    cmd.args(["-c", "init.templateDir="]);
    cmd
}

/// `git init` in `dir` (created when missing), on branch `main`.
pub fn init(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut cmd = fresh(dir);
    cmd.args(["init", "-q", "-b", "main"]);
    checked(cmd, "git init", None)?;
    Ok(())
}

/// The `-c` args a clone of `url` runs with: credentials only from `gh` (none at all without one: every helper,
/// askpass and extra header cleared), and https as the only transport.
pub fn clone_config_args(gh: Option<&GitHub>, url: &str) -> Result<Vec<String>> {
    let mut args: Vec<String> = match gh {
        Some(gh) => credential_args(gh, &[url])?,
        None => ["credential.helper=", "core.askPass=", "http.extraHeader="]
            .iter()
            .flat_map(|kv| ["-c".to_owned(), (*kv).to_owned()])
            .collect(),
    };
    for kv in ["protocol.allow=never", "protocol.https.allow=always"] {
        args.extend(["-c".to_owned(), kv.to_owned()]);
    }
    Ok(args)
}

/// Clones `url` (plain https, no login in it: see `skyelab_core::add_project::clone_name`) into `dest`, which must
/// not exist yet. Credentials come only from `gh` (the account bound to `dest`), else from nowhere: every helper and
/// askpass is cleared and prompts are off, so only a public repo clones. Submodules aren't fetched. A failed clone
/// leaves no folder behind.
pub fn clone(url: &str, dest: &Path, gh: Option<&GitHub>) -> Result<PathBuf> {
    skyelab_core::add_project::clone_name(url).map_err(|e| invalid(format!("{e:#}")))?;
    if dest.exists() {
        return Err(invalid(format!("{} already exists", dest.display())));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| invalid("a clone needs a parent folder"))?;
    std::fs::create_dir_all(parent)?;
    let mut cmd = fresh(parent);
    cmd.args(clone_config_args(gh, url)?);
    if let Some(gh) = gh {
        cmd.env("GH_CONFIG_DIR", gh.config_dir);
    }
    cmd.args(["clone", "-q", "--no-recurse-submodules", "--"])
        .arg(url)
        .arg(dest)
        .envs(crate::test_ca_env())
        .env("GIT_TERMINAL_PROMPT", "0");
    let result = checked(cmd, "git clone", None);
    if result.is_err() {
        // Only what this clone made: `dest` didn't exist before.
        let _ = std::fs::remove_dir_all(dest);
    }
    result?;
    Ok(dest.to_owned())
}
