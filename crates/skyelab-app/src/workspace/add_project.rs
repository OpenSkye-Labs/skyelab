//! Add project as T3 does it, inside the palette: a path field that lists the typed folder's subfolders (git badge
//! and branch, Tab completes, Enter browses in), recent folders from the accounts' agent history, New project,
//! Clone from URL and Browse… (the native picker, only as a fallback). Every way in ends on the same question: which
//! account the folder runs on. It is asked unless the folder already resolves, and nothing is bound unless the user
//! picks an account.

use super::{AddProject, Workspace, github};
use crate::palette::{Entry, Kind, PageSpec};
use crate::ui::SkyeIcon;
use gpui::{App, AppContext as _, Context, PathPromptOptions, WeakEntity, Window};
use skyelab_core::AccountsConfig;
use skyelab_core::add_project as paths;
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod search;
use search::{clone_search, folder_search, new_project_search};

/// The most subfolders one listing shows.
const MAX_FOLDERS: usize = 200;
/// The most recent folders the empty page lists.
const MAX_RECENT: usize = 8;

/// What the chosen account is for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    /// An existing folder.
    Folder(PathBuf),
    /// A folder to create and `git init`.
    New(PathBuf),
    /// An https URL to clone into `dest`.
    Clone { url: String, dest: PathBuf },
}

impl Target {
    pub(super) fn dir(&self) -> &Path {
        match self {
            Target::Folder(d) | Target::New(d) => d,
            Target::Clone { dest, .. } => dest,
        }
    }
}

fn short(path: &Path, home: Option<&Path>) -> String {
    crate::ui::short_path(path, home)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// What a folder row says under its name: the branch of a repo, and where it is.
fn git_line(git: &Option<Option<String>>, path: &Path, home: Option<&Path>) -> String {
    let at = short(path, home);
    match git {
        Some(Some(branch)) => format!("git · {branch} · {at}"),
        Some(None) => format!("git · detached · {at}"),
        None => at,
    }
}

impl Workspace {
    /// The home the page writes `~` for; tests point it at a temp dir through the onboarding page.
    pub(super) fn home(&self, cx: &App) -> Option<PathBuf> {
        self.welcome.read(cx).setup.read(cx).home.clone()
    }

    /// Where New project and a clone go by default: the current project's parent, else `~/code`, else home.
    fn projects_root(&self, cx: &App) -> Option<PathBuf> {
        let parent = self
            .project
            .as_ref()
            .and_then(|p| p.path.parent().map(Path::to_owned));
        let home = self.home(cx);
        parent
            .or_else(|| home.as_ref().map(|h| h.join("code")).filter(|c| c.is_dir()))
            .or(home)
    }

    /// AddProject: the palette's Add project page. Never a native picker unless the user picks Browse….
    pub(super) fn pick_project(
        &mut self,
        _: &AddProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // it adds this machine's folders while another one is shown
        }
        // Nothing focused (onboarding just closed): Back returns to the workspace.
        let prev = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let spec = self.add_project_page(cx);
        crate::palette::open_spec(spec, prev, window, cx);
    }

    pub(super) fn add_project_page(&self, cx: &mut Context<Self>) -> PageSpec {
        let ws = cx.entity().downgrade();
        let home = self.home(cx);
        let root = self.projects_root(cx);
        let config = self.config.clone();
        let projects: Vec<PathBuf> = self.projects.iter().map(|p| p.path.clone()).collect();
        let base = root.clone().unwrap_or_default();
        let new = PageSpec::new("New project", Vec::new())
            .placeholder("Project name, or a full path")
            .search(new_project_search(
                ws.clone(),
                config.clone(),
                base.clone(),
                home.clone(),
            ));
        let clone = PageSpec::new("Clone from URL", Vec::new())
            .placeholder("https://github.com/owner/repo")
            .search(clone_search(ws.clone(), config.clone(), base, home.clone()));
        let browse = run_on(&ws, |ws, window, cx| ws.browse_for_folder(window, cx));
        let entries = vec![
            Entry::new(
                "New project",
                Kind::Action("skyelab::Submenu"),
                Rc::new(|_, _| {}),
            )
            .icon(SkyeIcon::FolderGit2)
            .subtitle(match &root {
                Some(r) => format!(
                    "Create a folder with a git repo in {}",
                    short(r, home.as_deref())
                ),
                None => "Create a folder with a git repo".into(),
            })
            .page(new),
            Entry::new(
                "Clone from URL",
                Kind::Action("skyelab::Submenu"),
                Rc::new(|_, _| {}),
            )
            .icon(SkyeIcon::Link)
            .subtitle("Clone an https:// repository")
            .page(clone),
            Entry::new("Browse…", Kind::Action("skyelab::Browse"), browse)
                .icon(SkyeIcon::FolderOpen)
                .subtitle("Pick a folder in the system dialog"),
        ];
        PageSpec::new("Add project", entries)
            .placeholder("Type a path (~/code/app) or pick below")
            .search(folder_search(
                ws,
                config,
                projects,
                home,
                self.backend.store.clone(),
            ))
    }

    /// Browse…: the native picker, then the same account question.
    fn browse_for_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        self._add = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return; // cancelled, or the platform has no picker
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.ask_account(Target::Folder(dir), window, cx)
            });
        }));
    }

    /// The folder already resolves: add it on that account. Otherwise open the account question.
    pub(super) fn ask_account(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(account) = self.config.resolve(target.dir()) {
            let id = account.id.clone();
            return self.finish_add(target, Some(id), window, cx);
        }
        let spec = bind_page(cx.entity().downgrade(), &self.config, target, self.home(cx));
        let prev = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        crate::palette::open_spec(spec, prev, window, cx);
    }

    /// Creates or clones the folder when asked to, binds it to `account` when one was picked, and adds it.
    /// A failed create or clone binds nothing.
    pub(super) fn finish_add(
        &mut self,
        target: Target,
        account: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gh = account
            .as_deref()
            .and_then(|id| self.config.accounts.iter().find(|a| a.id == id))
            .and_then(|a| a.gh_config_dir.clone())
            .map(|dir| (github::gh_bin(cx), dir));
        let make = target.clone();
        let task = cx.background_spawn(async move {
            match make {
                Target::Folder(dir) => Ok(dir),
                Target::New(dir) => skyelab_git::init(&dir)
                    .map(|()| dir)
                    .map_err(anyhow::Error::from),
                Target::Clone { url, dest } => {
                    let gh = gh.as_ref().map(|(gh, dir)| skyelab_git::GitHub {
                        gh,
                        config_dir: dir,
                    });
                    skyelab_git::clone(&url, &dest, gh.as_ref()).map_err(anyhow::Error::from)
                }
            }
        });
        // Its own task, not the shared `_add` slot: a later add must not cancel a create or clone halfway.
        cx.spawn_in(window, async move |this, cx| {
            let made = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let bound = made.and_then(|dir| {
                    if let Some(id) = &account {
                        this.edit_accounts(
                            |c| skyelab_core::onboarding::bind_project(c, &dir, id),
                            cx,
                        )?;
                    }
                    Ok(dir)
                });
                match bound {
                    Ok(dir) => this.add_project(dir, cx),
                    Err(e) => this.say(format!("Couldn't add the project: {e:#}"), window, cx),
                }
            });
        })
        .detach();
    }
}

fn run_on(
    ws: &WeakEntity<Workspace>,
    f: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> crate::palette::Run {
    let ws = ws.clone();
    Rc::new(move |window, cx| {
        _ = ws.update(cx, |ws, cx| f(ws, window, cx));
    })
}

/// "Add <folder>": straight in when the folder resolves already, else the account question as the next page.
fn add_row(
    ws: &WeakEntity<Workspace>,
    config: &AccountsConfig,
    target: Target,
    home: Option<&Path>,
) -> Entry {
    let dir = target.dir().to_owned();
    let label = match &target {
        Target::Folder(_) => format!("Add {}", short(&dir, home)),
        Target::New(_) => format!("Create {}", short(&dir, home)),
        Target::Clone { .. } => format!("Clone into {}", short(&dir, home)),
    };
    let icon = match &target {
        Target::Clone { .. } => SkyeIcon::Link,
        _ => SkyeIcon::FolderPlus,
    };
    let bound = config.resolve(&dir).map(|a| a.label.clone());
    let subtitle = match (&bound, paths::git_info(&dir)) {
        (Some(label), _) => format!("Runs on {label}, the account its folder is bound to"),
        (None, Some(Some(branch))) => format!("git · {branch} · choose its account next"),
        (None, _) => "Choose the account it runs on next".into(),
    };
    let entry = Entry::new(
        label,
        Kind::Action("skyelab::AddFolder"),
        Rc::new(|_, _| {}),
    )
    .icon(icon)
    .subtitle(subtitle)
    .group("Add");
    match bound {
        Some(_) => {
            let id = config.resolve(&dir).map(|a| a.id.clone());
            Entry {
                run: run_on(ws, move |ws, window, cx| {
                    ws.finish_add(target.clone(), id.clone(), window, cx)
                }),
                ..entry
            }
        }
        None => entry.page(bind_page(
            ws.clone(),
            config,
            target,
            home.map(Path::to_owned),
        )),
    }
}

/// "Which account does <folder> run on?": every account, Add account…, and Leave unbound. Only a pick binds.
pub(super) fn bind_page(
    ws: WeakEntity<Workspace>,
    config: &AccountsConfig,
    target: Target,
    home: Option<PathBuf>,
) -> PageSpec {
    let name = name_of(target.dir());
    let mut entries: Vec<Entry> = config
        .accounts
        .iter()
        .map(|a| {
            let (id, target) = (a.id.clone(), target.clone());
            let icon = SkyeIcon::UserCheck; // a palette glyph: provider logos are images only
            Entry::new(
                format!("Bind to {}", a.label),
                Kind::Action("skyelab::BindAccount"),
                run_on(&ws, move |ws, window, cx| {
                    ws.finish_add(target.clone(), Some(id.clone()), window, cx)
                }),
            )
            .title(a.label.clone())
            .subtitle(format!(
                "{} · {}",
                a.provider.label(),
                short(&a.config_dir, home.as_deref())
            ))
            .icon(icon)
        })
        .collect();
    entries.push(
        Entry::new(
            "Add account…",
            Kind::Action("skyelab::OpenSettingsSection"),
            Rc::new(|window, cx| {
                window.dispatch_action(
                    Box::new(super::OpenSettingsSection {
                        section: crate::settings::Section::Accounts,
                    }),
                    cx,
                )
            }),
        )
        .icon(SkyeIcon::Plus)
        .subtitle("Add or sign in to an account in Settings, then add the project again"),
    );
    let unbound = target.clone();
    entries.push(
        Entry::new(
            "Leave unbound",
            Kind::Action("skyelab::LeaveUnbound"),
            run_on(&ws, move |ws, window, cx| {
                ws.finish_add(unbound.clone(), None, window, cx)
            }),
        )
        .icon(SkyeIcon::Unlink)
        .subtitle(if matches!(target, Target::Clone { .. }) {
            "Clone without a login (public repos only); agents ask for an account before they start"
        } else {
            "Agents ask for an account before they start"
        }),
    );
    PageSpec::new(format!("Which account does {name} run on?"), entries)
}
