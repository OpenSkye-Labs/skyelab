//! First-run onboarding, one page per step (T3's content): Welcome → Theme → Agents & accounts →
//! Import projects → Done. Detection only reads names and runs `--version`; nothing is written until a click, and
//! the workspace does the writing (`OnboardingEvent`). Return or Space goes next and Esc back while the page itself
//! has focus (a field or button keeps its keys).

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, Task,
    Window, actions,
};
use gpui_component::input::InputState;
use skyelab_core::onboarding::FoundAccount;
use skyelab_core::{Account, AccountsConfig, ColorMode, Provider};
use std::path::{Path, PathBuf};

pub(crate) mod first_run;
mod import;
pub(crate) mod motion;
mod pages;
mod render;
mod theme;

actions!(skyelab, [OnboardingNext, OnboardingBack]);

const CONTEXT: &str = "Onboarding";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", OnboardingNext, Some(CONTEXT)),
        KeyBinding::new("space", OnboardingNext, Some(CONTEXT)),
        KeyBinding::new("escape", OnboardingBack, Some(CONTEXT)),
    ]);
}

pub enum OnboardingEvent {
    /// The user ticked these found dirs and clicked Add.
    AddAccounts(Vec<Account>),
    /// Import these folders: bind each to its account (`None`: it already resolves) and add it as a project.
    Import(Vec<(PathBuf, Option<String>)>),
    /// Done or Skip: never open by itself again.
    Close,
}

/// The pages, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Welcome,
    Theme,
    Agents,
    Import,
    Done,
}

impl Step {
    pub const ALL: [Step; 5] = [
        Step::Welcome,
        Step::Theme,
        Step::Agents,
        Step::Import,
        Step::Done,
    ];

    fn offset(self, by: isize) -> Option<Step> {
        let at = Self::ALL.iter().position(|s| *s == self)? as isize + by;
        usize::try_from(at)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

/// A found config dir, and its name field once the user chose to rename it.
pub(super) struct Found {
    pub found: FoundAccount,
    pub label: Option<Entity<InputState>>,
}

/// `--version` of an agent or gh: `None` while checking.
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
    pub cursor_bin: PathBuf,
    pub antigravity_bin: PathBuf,
    pub gh_bin: PathBuf,
}

pub struct Onboarding {
    /// Where to look for config dirs; tests point it at a temp dir.
    pub(super) home: Option<PathBuf>,
    pub(super) shown: bool,
    pub step: Step,
    /// The step before the last change, for the dot that shrinks.
    pub(super) from: Step,
    /// The page fading out over the new one after a change, until its fade is over.
    pub(super) leaving: Option<Step>,
    pub(super) focus: FocusHandle,
    /// The page took the focus since it was last shown.
    focused: bool,
    pub(super) config: AccountsConfig,
    pub(super) projects: Vec<PathBuf>,
    pub(super) load_error: Option<String>,
    pub(super) agents: Vec<Agent>,
    pub(super) found: Vec<Found>,
    pub(crate) import: import::Import,
    pub error: Option<String>,
    /// Whether each account is signed in (the workspace asks; shared with Settings).
    pub(super) login: Entity<super::login_status::LoginStatus>,
    _login: gpui::Subscription,
    _detect: Option<Task<()>>,
    _scan: Option<Task<()>>,
    /// Drops `leaving` once its fade is over.
    _leave: Option<Task<()>>,
}

impl EventEmitter<OnboardingEvent> for Onboarding {}

impl Focusable for Onboarding {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Onboarding {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let login = super::login_status::LoginStatus::shared(cx);
        Self {
            _login: cx.observe(&login, |_, _, cx| cx.notify()),
            login,
            // Tests set it themselves, so no test reads the real home folder.
            home: if cfg!(test) { None } else { dirs::home_dir() },
            shown: false,
            step: Step::Welcome,
            from: Step::Welcome,
            leaving: None,
            focus: cx.focus_handle(),
            focused: false,
            config: AccountsConfig::default(),
            projects: Vec::new(),
            load_error: None,
            agents: Vec::new(),
            found: Vec::new(),
            import: Default::default(),
            error: None,
            _detect: None,
            _scan: None,
            _leave: None,
        }
    }

    /// Shows or hides the page with the workspace's current accounts and projects; opens at Welcome and detects
    /// again each time it opens.
    pub fn sync(&mut self, show: bool, s: Sync, cx: &mut Context<Self>) {
        let opened = show && !self.shown;
        self.shown = show;
        if !show {
            self.focused = false;
        }
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
        // Imported folders leave the list (and its groups, which index it); an account removed elsewhere can't
        // stay picked.
        let (projects, config) = (&self.projects, &self.config);
        let before = self.import.rows.len();
        self.import
            .rows
            .retain(|r| !projects.contains(&r.cand.path));
        if self.import.rows.len() != before {
            self.import.regroup();
        }
        for r in &mut self.import.rows {
            if !config.accounts.iter().any(|a| a.id == r.account) {
                r.checked = false;
            }
        }
        if opened {
            (self.step, self.from, self.leaving) = (Step::Welcome, Step::Welcome, None);
            self.import = Default::default();
            self.detect(
                s.claude_bin,
                s.codex_bin,
                s.cursor_bin,
                s.antigravity_bin,
                s.gh_bin,
                cx,
            );
        }
        if self.step == Step::Import {
            self.start_import_scan(cx);
        }
        cx.notify();
    }

    fn detect(
        &mut self,
        claude: PathBuf,
        codex: PathBuf,
        cursor: PathBuf,
        antigravity: PathBuf,
        gh: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.agents = [
            "Claude Code",
            "Codex",
            "Cursor",
            "Antigravity",
            "GitHub CLI",
        ]
        .map(|name| Agent {
            name,
            version: None,
            update: None,
        })
        .into();
        let latest = crate::updates::agent_check(cx);
        let (home, config) = (self.home.clone(), self.config.clone());
        let found = cx.background_spawn(async move {
            home.as_deref()
                .map(|h| skyelab_core::onboarding::find_accounts(h, &config))
                .unwrap_or_default()
        });
        let versions = cx.background_spawn(async move {
            [claude, codex, cursor, antigravity, gh]
                .map(|b| skyelab_agent::agent_version(&b).map_err(|e| format!("{e:#}")))
        });
        self._detect = Some(cx.spawn(async move |this, cx| {
            let found = found.await;
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
                cx.notify();
            });
            let versions = versions.await;
            let installed = [versions[0].clone().ok(), versions[1].clone().ok()];
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

    pub(super) fn go(&mut self, to: Step, cx: &mut Context<Self>) {
        if to == self.step {
            return;
        }
        (self.from, self.step) = (self.step, to);
        self.error = None;
        // The old page fades out over the new one, then goes; with Reduce Motion there is nothing to fade.
        self.leaving = (!cx.reduce_motion()).then_some(self.from);
        self._leave = self.leaving.map(|_| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(motion::FADE_OUT).await;
                let _ = this.update(cx, |this, cx| {
                    this.leaving = None;
                    cx.notify();
                });
            })
        });
        if to == Step::Import {
            self.start_import_scan(cx);
        }
        cx.notify();
    }

    /// Next, Return or Space: the next page; Import imports the ticked folders first; Done closes.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        match self.step {
            // Still scanning: nothing to import yet, and skipping ahead would look like an empty history.
            Step::Import if self.import.scanning => {}
            Step::Import => self.import_picked(cx),
            Step::Done => cx.emit(OnboardingEvent::Close),
            step => {
                if let Some(to) = step.offset(1) {
                    self.go(to, cx);
                }
            }
        }
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(to) = self.step.offset(-1) {
            self.go(to, cx);
        }
    }

    pub fn skip(&mut self, cx: &mut Context<Self>) {
        cx.emit(OnboardingEvent::Close);
    }

    /// The workspace imported the picked folders: on to Done.
    pub fn imported(&mut self, cx: &mut Context<Self>) {
        self.go(Step::Done, cx);
    }

    pub(super) fn pick_family(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let mut s = crate::ui::themes::get(cx);
        s.theme = key.into();
        gpui::TaskExt::detach_and_log_err(crate::ui::themes::set(s, cx), cx);
        cx.notify();
    }

    pub(super) fn pick_mode(&mut self, ix: usize, cx: &mut Context<Self>) {
        let mut s = crate::ui::themes::get(cx);
        s.mode = match ix {
            1 => ColorMode::Light,
            2 => ColorMode::Dark,
            _ => ColorMode::System,
        };
        gpui::TaskExt::detach_and_log_err(crate::ui::themes::set(s, cx), cx);
        cx.notify();
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
}

impl gpui::Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        use gpui::{InteractiveElement as _, ParentElement as _, Styled as _, div};
        let _render = crate::debug_renders::count("Onboarding");
        // Take the keys once per showing; a click elsewhere afterwards keeps its focus.
        if self.shown && !self.focused {
            self.focused = true;
            window.focus(&self.focus, cx);
        }
        let frame = self.frame(window, cx);
        div()
            .id("onboarding")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &OnboardingNext, window, cx| {
                if this.focus.is_focused(window) {
                    this.next(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &OnboardingBack, window, cx| {
                if this.focus.is_focused(window) {
                    this.back(cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(frame)
    }
}
