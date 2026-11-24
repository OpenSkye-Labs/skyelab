//! Project scripts (T3's project actions), kept in SkyeLab's store rather than the repo (docs/decisions.md says why).
//! Each runs in a new terminal tab on the folder's bound account, typed into the shell so the tab stays open; setup
//! scripts also run in every new worktree the user starts.

use super::lock;
use super::thread::{ThreadView, ThreadViewEvent};
use super::{SharedStore, Workspace};
use crate::ui;
use anyhow::{Context as _, Result, bail};
use gpui::{
    Action, AppContext as _, Context, Entity, FontWeight, SharedString, Task, WeakEntity, Window,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use skyelab_core::{Account, AccountsConfig};
use skyelab_store::ProjectScript;
use skyelab_terminal::SpawnOptions;
use std::path::{Path, PathBuf};

/// Runs project script `id` of the current project, in the project folder (the palette's entries).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct RunProjectScript {
    pub id: i64,
}

/// A script's terminal: the folder's terminal env under its bound account (`account_id`, when the thread names
/// one, must be the account the folder resolves to), plus `SKYELAB_PROJECT_ROOT` and, in a worktree,
/// `SKYELAB_WORKTREE_PATH`. An unbound folder is refused: a script never runs without an account.
pub(super) fn script_options(
    config: &AccountsConfig,
    cwd: &Path,
    project: &Path,
    account_id: Option<&str>,
) -> Result<(SpawnOptions, Option<Account>)> {
    let account = match account_id {
        Some(id) => {
            super::worktree::resolves_to(config, cwd, id)?;
            config.accounts.iter().find(|a| a.id == id).cloned()
        }
        None => Some(config.resolve(cwd).cloned().with_context(|| {
            format!(
                "{} isn't bound to an account; bind it before running scripts",
                cwd.display()
            )
        })?),
    };
    let text = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .with_context(|| format!("{} is not valid UTF-8", p.display()))
    };
    let mut env = super::terminal_env(config, cwd, &super::unbound_root()?)?;
    env.insert("SKYELAB_PROJECT_ROOT".into(), text(project)?);
    if cwd != project {
        env.insert("SKYELAB_WORKTREE_PATH".into(), text(cwd)?);
    }
    let options = SpawnOptions {
        cwd: cwd.to_owned(),
        command: None,
        env,
        cols: 80,
        lines: 24,
    };
    Ok((options, account))
}

/// What is typed into the shell. The leading space (as T3 types it) keeps it out of the history of shells set to skip
/// such lines (zsh `HIST_IGNORE_SPACE`, bash `HISTCONTROL=ignorespace`). One line only: a newline or a control
/// character would type more than the command shown.
pub(super) fn typed(command: &str) -> Result<String> {
    if command.trim().is_empty() || command.chars().any(char::is_control) {
        bail!("a script needs a command");
    }
    Ok(format!(" {}\r", command.trim()))
}

impl Workspace {
    /// Runs `script` in a new terminal on `cwd` under its bound account.
    pub(super) fn run_script(
        &mut self,
        script: &ProjectScript,
        cwd: PathBuf,
        account_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .projects
            .iter()
            .find(|p| p.id == script.project_id)
            .map(|p| p.path.clone())
        else {
            return;
        };
        let input = match typed(&script.command) {
            Ok(i) => i,
            Err(e) => return self.script_failed(e, cx),
        };
        let config = self.accounts_job(cx);
        let job = move || script_options(&config(), &cwd, &project, account_id.as_deref());
        let tab = Some((format!("Run: {}", script.name).into(), input));
        self.open_terminal_tab(
            job,
            |_, _| true,
            tab,
            window,
            cx,
            |this, e, cx| this.script_failed(e, cx),
        );
    }

    fn script_failed(&mut self, e: anyhow::Error, cx: &mut Context<Self>) {
        self.store_error = Some(format!("{:#}", e.context("running the script")).into());
        cx.notify();
    }

    pub(super) fn on_run_project_script(
        &mut self,
        a: &RunProjectScript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return;
        }
        let Some(script) = self.scripts.iter().find(|s| s.id == a.id).cloned() else {
            return;
        };
        let folder = self.folder.clone();
        self.run_script(&script, folder, None, window, cx);
    }

    /// The current project's scripts, for the palette.
    pub(super) fn load_scripts(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.project.as_ref().map(|p| p.id) else {
            return;
        };
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.project_scripts(project_id) });
        self._scripts = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, _| {
                if this.project.as_ref().map(|p| p.id) == Some(project_id) {
                    this.scripts = loaded.unwrap_or_default();
                }
            });
        }));
    }

    /// A new worktree thread: every setup script of its project runs in the worktree, on the thread's account.
    pub(super) fn run_setup_scripts(
        &mut self,
        project_id: i64,
        cwd: PathBuf,
        account_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.project_scripts(project_id) });
        cx.spawn_in(window, async move |this, cx| {
            let scripts = task.await;
            let _ = this.update_in(cx, |this, window, cx| match scripts {
                Ok(scripts) => {
                    for s in scripts.iter().filter(|s| s.setup) {
                        this.run_script(s, cwd.clone(), Some(account_id.clone()), window, cx);
                    }
                }
                Err(e) => this.script_failed(e, cx),
            });
        })
        .detach();
    }
}

/// The header's Run menu: the project's scripts, each with a run and a remove button, and a form to add one.
pub struct ScriptMenu {
    owner: WeakEntity<ThreadView>,
    store: SharedStore,
    project_id: i64,
    pub(super) open: bool,
    pub(super) scripts: Vec<ProjectScript>,
    name: Entity<InputState>,
    command: Entity<InputState>,
    setup: bool,
    pub(super) error: Option<SharedString>,
    _task: Option<Task<()>>,
}

impl ScriptMenu {
    pub fn new(
        owner: WeakEntity<ThreadView>,
        store: SharedStore,
        project_id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name (Dev server)"));
        let command = cx.new(|cx| InputState::new(window, cx).placeholder("Command (npm run dev)"));
        let mut this = Self {
            owner,
            store,
            project_id,
            open: false,
            scripts: Vec::new(),
            name,
            command,
            setup: false,
            error: None,
            _task: None,
        };
        this.reload(cx);
        this
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            self.error = None;
            self.reload(cx);
        }
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let id = self.project_id;
        self.store_job(false, move |s| s.project_scripts(id), cx);
    }

    /// Runs `job` on the store off the UI thread, after the jobs before it (chained, so none is dropped and edits land
    /// in order); its list replaces the shown scripts. Once an `edit` is stored the owner hears of it, so the
    /// palette's list catches up.
    fn store_job(
        &mut self,
        edit: bool,
        job: impl FnOnce(&skyelab_store::Store) -> Result<Vec<ProjectScript>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let work = cx.background_executor().clone();
        let prev = self._task.take();
        self._task = Some(cx.spawn(async move |this, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
            let done = work.spawn(async move { job(&*lock(&store)?) }).await;
            let _ = this.update(cx, |this, cx| {
                match done {
                    Ok(list) => {
                        this.scripts = list;
                        if edit {
                            this.changed(cx);
                        }
                    }
                    Err(e) => this.error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        let command = self.command.read(cx).value().trim().to_owned();
        if name.is_empty() || typed(&command).is_err() {
            self.error = Some("A script needs a name and a command.".into());
            cx.notify();
            return;
        }
        for input in [&self.name, &self.command] {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        let (id, setup) = (self.project_id, std::mem::take(&mut self.setup));
        self.store_job(
            true,
            move |s| {
                s.add_project_script(id, &name, &command, setup)?;
                s.project_scripts(id)
            },
            cx,
        );
    }

    fn remove(&mut self, script: i64, cx: &mut Context<Self>) {
        let id = self.project_id;
        self.store_job(
            true,
            move |s| {
                s.delete_project_script(script)?;
                s.project_scripts(id)
            },
            cx,
        );
    }

    fn changed(&self, cx: &mut Context<Self>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |_, cx| cx.emit(ThreadViewEvent::ScriptsChanged));
        }
    }

    fn run(&mut self, script: ProjectScript, cx: &mut Context<Self>) {
        self.set_open(false, cx);
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |_, cx| {
                cx.emit(ThreadViewEvent::RunScript(Box::new(script)))
            });
        }
    }
}

impl Render for ScriptMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("ScriptMenu");
        let theme = cx.theme();
        let caption = |text: &'static str| {
            div()
                .px_1()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let rows = self.scripts.iter().map(|s| {
            let (run, id) = (s.clone(), s.id);
            h_flex()
                .id(("script", s.id as u64))
                .gap_2()
                .child(
                    Button::new(("run-script", s.id as u64))
                        .ghost()
                        .xsmall()
                        .icon(gpui_component::Icon::new(crate::ui::SkyeIcon::Play))
                        .label(s.name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| this.run(run.clone(), cx))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(theme.mono_font_family.clone())
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(s.command.clone()),
                )
                .when(s.setup, |r| {
                    r.child(
                        div()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child("setup"),
                    )
                })
                .child(
                    Button::new(("remove-script", s.id as u64))
                        .ghost()
                        .xsmall()
                        .icon(gpui_component::Icon::new(crate::ui::SkyeIcon::Delete))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove(id, cx))),
                )
        });
        let setup = self.setup;
        v_flex()
            .id("script-menu")
            .w(px(360.))
            .gap_1p5()
            .child(caption("Project scripts"))
            .when(self.scripts.is_empty(), |v| {
                v.child(
                    div()
                        .px_1()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child("No scripts yet. They run in a terminal on this folder's account."),
                )
            })
            .children(rows)
            .child(caption("Add a script"))
            .child(Input::new(&self.name).small())
            .child(Input::new(&self.command).small())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("script-setup")
                            .xsmall()
                            .label("Run in every new worktree")
                            .checked(setup)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.setup = !this.setup;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("add-script")
                            .small()
                            .primary()
                            .label("Add")
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
            .children(self.error.clone().map(|e| {
                div()
                    .px_1()
                    .text_size(ui::META)
                    .text_color(crate::ui::ink(theme, theme.danger))
                    .child(e)
            }))
    }
}
