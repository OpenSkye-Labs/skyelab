//! What a project terminal launches with beyond skyelab-terminal's inherited allowlist: the folder's account.

use super::Workspace;
use anyhow::Context as _;
use gpui::{App, AppContext as _, Context, Window};
use skyelab_core::AccountsConfig;
use skyelab_terminal::SpawnOptions;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Env for a terminal in `dir`, on top of the inherited allowlist skyelab-terminal keeps: the bound account's dirs,
/// and git's https credentials from its gh only (none at all without one). A provider home the account doesn't
/// set, and every home plus `GH_CONFIG_DIR` in an unbound folder, is an empty private dir under `unbound`, one set
/// per account, so `claude`, `codex`, `cursor-agent` or `gh` there asks to log in instead of using the default account.
pub(super) fn terminal_env(
    config: &AccountsConfig,
    dir: &Path,
    unbound: &Path,
) -> anyhow::Result<HashMap<String, String>> {
    let text = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .with_context(|| format!("{} is not valid UTF-8", p.display()))
    };
    let account = config.resolve(dir);
    let bound = match account {
        Some(_) => config.launch_env(dir)?,
        None => Vec::new(),
    };
    let mut env = HashMap::new();
    let mut homes = vec![
        ("CLAUDE_CONFIG_DIR", "claude"),
        ("CODEX_HOME", "codex"),
        ("CURSOR_CONFIG_DIR", "cursor"),
    ];
    if account.is_none() {
        homes.push(("GH_CONFIG_DIR", "gh"));
    }
    // Per account, so a login made in one account's folder never answers in another's.
    let base = match account {
        Some(a) => unbound.join(format!("account-{}", hex(&a.id))),
        None => unbound.to_owned(),
    };
    for (var, sub) in homes {
        if !bound.iter().any(|(k, _)| *k == var) {
            let empty = base.join(sub);
            private_dir(&empty)?;
            env.insert(var.to_string(), text(&empty)?);
        }
    }
    for (k, v) in &bound {
        env.insert(k.to_string(), text(v)?);
    }
    let has_gh = account.is_some_and(|a| a.gh_config_dir.is_some());
    let scoped = skyelab_core::env::scoped_extra_headers(dir);
    // ssh stays on: a terminal is the user's own shell, and SSH_AUTH_SOCK passes to it.
    env.extend(skyelab_core::env::git_credential_env(has_gh, &scoped, true));
    Ok(env)
}

/// An account id as a file name: ids are free text.
fn hex(id: &str) -> String {
    id.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Where unbound terminals' empty provider homes live: `<data dir>/skyelab/unbound`.
pub(super) fn unbound_root() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no platform data dir for the unbound terminal homes")?
        .join(skyelab_core::instance::APP_DIR)
        .join("unbound"))
}

fn private_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // A link here could point a terminal at another account's real config dir.
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        anyhow::bail!("{} is a symlink, not a private dir", dir.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("making {} private", dir.display()))?;
    }
    Ok(())
}

impl Workspace {
    /// What a new terminal runs with: the current folder, under its bound account or none. Blocking (it runs
    /// `git config`), so the job runs off the UI thread.
    pub(super) fn terminal_job(
        &self,
        cx: &App,
    ) -> impl FnOnce() -> anyhow::Result<(SpawnOptions, Option<skyelab_core::Account>)> + Send + use<>
    {
        self.terminal_job_in(self.folder.clone(), cx)
    }

    /// [`Self::terminal_job`] for `folder`: its bound account or none, like any terminal.
    pub(super) fn terminal_job_in(
        &self,
        folder: PathBuf,
        cx: &App,
    ) -> impl FnOnce() -> anyhow::Result<(SpawnOptions, Option<skyelab_core::Account>)> + Send + use<>
    {
        let config = self.accounts_job(cx);
        move || {
            let config = config();
            // The tab shows the account this env was built for, from the same read of accounts.toml.
            let account = config.resolve(&folder).cloned();
            let options = SpawnOptions {
                env: terminal_env(&config, &folder, &unbound_root()?)?,
                cwd: folder,
                command: None,
                cols: 80,
                lines: 24,
            };
            Ok((options, account))
        }
    }

    /// The accounts as accounts.toml says when the job runs (the window's copy without a file), like an agent
    /// launch: an outside edit applies to the next terminal at once.
    pub(super) fn accounts_job(&self, cx: &App) -> impl FnOnce() -> AccountsConfig + Send + use<> {
        let file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
        let config = self.config.clone();
        move || super::accounts_now(file.as_deref(), &config)
    }

    /// Builds a terminal's options with `job` in the background, then opens it here (or hands the error to `failed`),
    /// unless the window moved on meanwhile: to another machine, or `wanted` says no.
    pub(super) fn open_terminal(
        &mut self,
        job: impl FnOnce() -> anyhow::Result<(SpawnOptions, Option<skyelab_core::Account>)>
        + Send
        + 'static,
        wanted: impl FnOnce(&Self, &SpawnOptions) -> bool + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        failed: impl FnOnce(&mut Self, anyhow::Error, &mut Context<Self>) + 'static,
    ) {
        self.open_terminal_tab(job, wanted, None, window, cx, failed);
    }

    /// [`Self::open_terminal`] with a tab title and typed input (see [`Workspace::add_terminal_tab`]).
    pub(super) fn open_terminal_tab(
        &mut self,
        job: impl FnOnce() -> anyhow::Result<(SpawnOptions, Option<skyelab_core::Account>)>
        + Send
        + 'static,
        wanted: impl FnOnce(&Self, &SpawnOptions) -> bool + 'static,
        tab: super::terminals::TerminalTab,
        window: &mut Window,
        cx: &mut Context<Self>,
        failed: impl FnOnce(&mut Self, anyhow::Error, &mut Context<Self>) + 'static,
    ) {
        let options = cx.background_spawn(async move { job() });
        // The tab belongs to the thread shown when it was asked for, even if another shows by the time it opens.
        let owner = self.current_thread();
        cx.spawn_in(window, async move |this, cx| {
            let options = options.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let opened = options.and_then(|(o, account)| {
                    if this.is_remote() || !wanted(this, &o) {
                        return Ok(());
                    }
                    this.add_terminal_tab(o, account, tab, window, cx)?;
                    if let Some(panel) = this.terminals.last().cloned() {
                        this.set_terminal_owner(&panel, owner, cx);
                    }
                    this.sync_terminals(window, cx);
                    Ok(())
                });
                if let Err(e) = opened {
                    failed(this, e, cx);
                }
            });
        })
        .detach();
    }
}
