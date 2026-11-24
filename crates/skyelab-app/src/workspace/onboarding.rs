//! First-run setup on the Welcome page (T3's Welcome page): which agents are installed, the config dirs found in
//! home offered as accounts, and a project folder bound to one of them. Detection only reads names; nothing is
//! written until a click, and the workspace does the writing (`OnboardingEvent`).

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, PathPromptOptions, Task, Window};
use gpui_component::input::InputState;
use skyelab_core::onboarding::FoundAccount;
use skyelab_core::{Account, AccountsConfig, Provider};
use std::path::{Path, PathBuf};

mod projects;
mod render;

pub enum OnboardingEvent {
    /// The user ticked these found dirs and clicked Add.
    AddAccounts(Vec<Account>),
    /// Bind `dir` to `account` (unless it already resolves there) and add it as a project.
    AddProject { dir: PathBuf, account: String },
    /// Done or Skip: show the normal Welcome page.
    Close,
}

/// A found config dir, and its name field once the user chose to rename it.
pub(super) struct Found {
    pub found: FoundAccount,
    pub label: Option<Entity<InputState>>,
}

/// `--version` of an agent: `None` while checking.
pub(super) struct Agent {
    pub name: &'static str,
    pub version: Option<Result<String, String>>,
    /// A newer release of the CLI than the one installed (T3's provider update notice); never installed from here.
    pub update: Option<String>,
}

/// What the workspace tells the page each time it may have changed.
pub struct Sync {
    pub config: AccountsConfig,
    pub projects: Vec<PathBuf>,
    pub load_error: Option<String>,
    pub claude_bin: PathBuf,
    pub codex_bin: PathBuf,
}

pub struct Onboarding {
    /// Where to look for config dirs and repos; tests point it at a temp dir.
    pub(super) home: Option<PathBuf>,
    pub(super) shown: bool,
    pub(super) config: AccountsConfig,
    pub(super) projects: Vec<PathBuf>,
    pub(super) load_error: Option<String>,
    pub(super) agents: Vec<Agent>,
    pub(super) found: Vec<Found>,
    pub(super) repos: Vec<PathBuf>,
    /// The project folder picked, and the account chosen for it. Never defaulted except with exactly one account.
    pub(super) folder: Option<PathBuf>,
    pub(super) account: Option<String>,
    /// `account` was picked by `preselect`, not the user.
    auto_account: bool,
    pub error: Option<String>,
    _detect: Option<Task<()>>,
    _pick: Option<Task<()>>,
}

impl EventEmitter<OnboardingEvent> for Onboarding {}

impl Onboarding {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            // Tests set it themselves, so no test reads the real home folder.
            home: if cfg!(test) { None } else { dirs::home_dir() },
            shown: false,
            config: AccountsConfig::default(),
            projects: Vec::new(),
            load_error: None,
            agents: Vec::new(),
            found: Vec::new(),
            repos: Vec::new(),
            folder: None,
            account: None,
            auto_account: false,
            error: None,
            _detect: None,
            _pick: None,
        }
    }

    /// Shows or hides the page with the workspace's current accounts and projects; detects again each time it opens.
    pub fn sync(&mut self, show: bool, s: Sync, cx: &mut Context<Self>) {
        let opened = show && !self.shown;
        self.shown = show;
        self.config = s.config;
        self.projects = s.projects;
        self.load_error = s.load_error;
        // Rows for dirs added since (here or in Settings) go away.
        let added = |f: &Found| {
            self.config
                .accounts
                .iter()
                .any(|a| a.config_dir == f.found.config_dir)
        };
        self.found.retain(|f| !added(f));
        if self
            .account
            .as_ref()
            .is_some_and(|id| !self.config.accounts.iter().any(|a| &a.id == id))
        {
            self.account = None;
        }
        if self
            .folder
            .as_ref()
            .is_some_and(|d| self.projects.contains(d))
        {
            self.folder = None;
        }
        self.preselect();
        if opened {
            self.detect(s.claude_bin, s.codex_bin, cx);
        }
        cx.notify();
    }

    fn detect(&mut self, claude: PathBuf, codex: PathBuf, cx: &mut Context<Self>) {
        self.agents = ["Claude Code", "Codex"]
            .map(|name| Agent {
                name,
                version: None,
                update: None,
            })
            .into();
        let latest = crate::updates::agent_check(cx);
        let (home, config) = (self.home.clone(), self.config.clone());
        let found = cx.background_spawn(async move {
            let found = home
                .as_deref()
                .map(|h| skyelab_core::onboarding::find_accounts(h, &config))
                .unwrap_or_default();
            let repos = home
                .as_deref()
                .map(skyelab_core::onboarding::find_repos)
                .unwrap_or_default();
            (found, repos)
        });
        let versions = cx.background_spawn(async move {
            [claude, codex].map(|b| skyelab_agent::agent_version(&b).map_err(|e| format!("{e:#}")))
        });
        self._detect = Some(cx.spawn(async move |this, cx| {
            let (found, repos) = found.await;
            let _ = this.update(cx, |this, cx| {
                let added = |f: &FoundAccount| {
                    this.config
                        .accounts
                        .iter()
                        .any(|a| a.config_dir == f.config_dir)
                };
                this.found = found
                    .into_iter()
                    .filter(|f| !added(f))
                    .map(|found| Found { found, label: None })
                    .collect();
                this.repos = repos;
                cx.notify();
            });
            let versions = versions.await;
            let installed = versions.clone().map(Result::ok);
            let _ = this.update(cx, |this, cx| {
                for (agent, v) in this.agents.iter_mut().zip(versions) {
                    agent.version = Some(v);
                }
                cx.notify();
            });
            let Some(fetch) = latest else { return };
            let updates = cx
                .background_spawn(async move {
                    let providers = [Provider::Claude, Provider::Codex];
                    let newer = |(p, line): (Provider, Option<String>)| {
                        let have = skyelab_core::update::version_in(line.as_deref()?)?.to_owned();
                        let newest = fetch(p).ok()?;
                        skyelab_core::update::is_newer(&newest, &have).then_some(newest)
                    };
                    providers
                        .into_iter()
                        .zip(installed)
                        .map(newer)
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for (agent, u) in this.agents.iter_mut().zip(updates) {
                    agent.update = u;
                }
                cx.notify();
            });
        }));
    }

    /// Turns a found dir's name into a field, so it can be renamed before it is added.
    pub(super) fn rename(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.found.iter_mut().find(|r| r.found.config_dir == dir);
        let Some(row) = row.filter(|r| r.label.is_none()) else {
            return;
        };
        let label = row.found.label.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(label));
        input.update(cx, |s, cx| s.focus(window, cx));
        row.label = Some(input);
        cx.notify();
    }

    /// Found dir `dir` as an account, with the name typed (the suggestion when left empty). Rows are looked up by
    /// folder, not index: detection can replace the list between a paint and a click.
    pub(super) fn account(&self, dir: &Path, cx: &App) -> Option<Account> {
        let f = self.found.iter().find(|f| f.found.config_dir == dir)?;
        let typed = f
            .label
            .as_ref()
            .map(|l| l.read(cx).value().trim().to_string());
        Some(Account {
            id: f.found.id.clone(),
            label: typed
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| f.found.label.clone()),
            provider: f.found.provider,
            config_dir: f.found.config_dir.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        })
    }

    /// The user clicked Add on found dir `dir`: the only way an account gets written from here.
    pub(super) fn add_account(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if let Some(account) = self.account(dir, cx) {
            cx.emit(OnboardingEvent::AddAccounts(vec![account]));
        }
    }

    pub(super) fn add_project(&mut self, cx: &mut Context<Self>) {
        if let (Some(dir), Some(account)) = (self.folder.clone(), self.account.clone()) {
            cx.emit(OnboardingEvent::AddProject { dir, account });
        }
    }

    pub(super) fn set_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.folder = Some(dir);
        self.error = None;
        cx.notify();
    }

    pub(super) fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        self._pick = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return; // cancelled, or the platform has no picker
            };
            if let Some(dir) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| this.set_folder(dir, cx));
            }
        }));
    }

    /// With exactly one account it is chosen for the user, who still has to click to bind. That pick is dropped once
    /// a second account exists; a pick the user made stays.
    fn preselect(&mut self) {
        match self.config.accounts.as_slice() {
            [only] if self.account.is_none() => {
                self.account = Some(only.id.clone());
                self.auto_account = true;
            }
            [_] => {}
            _ if self.auto_account => (self.account, self.auto_account) = (None, false),
            _ => {}
        }
    }

    pub fn complete(&self) -> bool {
        !self.config.accounts.is_empty() && !self.projects.is_empty()
    }
}
