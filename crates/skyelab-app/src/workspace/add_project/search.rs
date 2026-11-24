//! The Add project page's searches: the path field's folders (or recent folders), New project's folder and Clone's
//! destination, each as palette rows.

use super::{MAX_FOLDERS, MAX_RECENT, Target, Workspace, add_row, git_line, name_of, short};
use crate::palette::{Entry, Kind, Search};
use crate::ui::SkyeIcon;
use gpui::{App, AppContext as _, SharedString, WeakEntity};
use skyelab_core::add_project as paths;
use skyelab_core::{AccountsConfig, Provider};
use std::path::PathBuf;
use std::rc::Rc;

/// The Add project page's rows for a query: a path's subfolders (and the typed folder itself first), else the
/// recent folders matching the text.
pub(super) fn folder_search(
    ws: WeakEntity<Workspace>,
    config: AccountsConfig,
    projects: Vec<PathBuf>,
    home: Option<PathBuf>,
    store: super::super::SharedStore,
) -> Search {
    Rc::new(move |query: String, cx: &mut App| {
        let (ws, config, projects, home, store) = (
            ws.clone(),
            config.clone(),
            projects.clone(),
            home.clone(),
            store.clone(),
        );
        cx.spawn(async move |cx| {
            if let Some(b) = paths::parse(&query, home.as_deref()) {
                let typed = paths::typed_path(&query, home.as_deref());
                let found = cx
                    .background_spawn({
                        let typed = typed.clone();
                        async move {
                            let exists = typed.as_ref().is_some_and(|t| t.is_dir());
                            (
                                paths::list(&b, MAX_FOLDERS),
                                exists.then_some(typed).flatten(),
                                b,
                            )
                        }
                    })
                    .await;
                let (folders, typed, b) = found;
                let mut rows: Vec<Entry> = typed
                    .map(|dir| add_row(&ws, &config, Target::Folder(dir), home.as_deref()))
                    .into_iter()
                    .collect();
                rows.extend(folders.into_iter().map(|f| {
                    let icon = if f.git.is_some() {
                        SkyeIcon::FolderGit2
                    } else {
                        SkyeIcon::Folder
                    };
                    Entry::new(f.name.clone(), Kind::Project, Rc::new(|_, _| {}))
                        .icon(icon)
                        .subtitle(git_line(&f.git, &f.path, home.as_deref()))
                        .group("Folders")
                        .browse(paths::complete(&b, &f.name))
                }));
                return rows;
            }
            let accounts: Vec<(Provider, PathBuf)> = config
                .accounts
                .iter()
                .map(|a| (a.provider, a.config_dir.clone()))
                .collect();
            let recent = cx
                .background_spawn(async move {
                    // Earlier projects first (removed from the list, still on disk), then agent history.
                    let removed = super::super::lock(&store)
                        .and_then(|s| s.removed_project_paths())
                        .unwrap_or_default();
                    recent_folders(removed, &accounts, &projects)
                })
                .await;
            let query = query.trim().to_owned();
            recent
                .into_iter()
                .filter(|(p, _)| {
                    query.is_empty()
                        || crate::palette::score(&query, &p.to_string_lossy()).is_some()
                })
                .map(|(dir, git)| {
                    let mut row =
                        add_row(&ws, &config, Target::Folder(dir.clone()), home.as_deref());
                    row.title = Some(name_of(&dir).into());
                    row.subtitle = Some(git_line(&git, &dir, home.as_deref()).into());
                    row.icon = Some(if git.is_some() {
                        SkyeIcon::FolderGit2
                    } else {
                        SkyeIcon::Folder
                    });
                    row.group("Recent")
                })
                .collect()
        })
    })
}

/// Earlier projects, then folders the accounts' agents ran in (newest first): those that exist and aren't projects.
fn recent_folders(
    removed: Vec<PathBuf>,
    accounts: &[(Provider, PathBuf)],
    projects: &[PathBuf],
) -> Vec<(PathBuf, Option<Option<String>>)> {
    let mut seen: Vec<(PathBuf, i64)> = accounts
        .iter()
        .flat_map(|(p, dir)| skyelab_agent::import::project_folders(*p, dir))
        .map(|(path, _, ms)| (path, ms))
        .collect();
    seen.sort_by_key(|s| std::cmp::Reverse(s.1));
    let mut out: Vec<(PathBuf, Option<Option<String>>)> = Vec::new();
    for path in removed.into_iter().chain(seen.into_iter().map(|(p, _)| p)) {
        if out.len() == MAX_RECENT {
            break;
        }
        if projects.contains(&path) || out.iter().any(|(p, _)| p == &path) || !path.is_dir() {
            continue;
        }
        let git = paths::git_info(&path);
        out.push((path, git));
    }
    out
}

/// New project's rows: the folder the typed name makes, or why it can't.
pub(super) fn new_project_search(
    ws: WeakEntity<Workspace>,
    config: AccountsConfig,
    base: PathBuf,
    home: Option<PathBuf>,
) -> Search {
    Rc::new(move |query: String, _: &mut App| {
        if query.trim().is_empty() {
            return gpui::Task::ready(Vec::new());
        }
        let row = match paths::new_project_dir(&query, &base, home.as_deref()) {
            Ok(dir) => add_row(&ws, &config, Target::New(dir), home.as_deref()),
            Err(e) => problem(&query, e),
        };
        gpui::Task::ready(vec![row])
    })
}

/// Clone's rows: the folder the URL clones into, or why it can't.
pub(super) fn clone_search(
    ws: WeakEntity<Workspace>,
    config: AccountsConfig,
    base: PathBuf,
    home: Option<PathBuf>,
) -> Search {
    Rc::new(move |query: String, _: &mut App| {
        let url = query.trim().to_owned();
        if url.is_empty() {
            return gpui::Task::ready(Vec::new());
        }
        let row = match paths::clone_name(&url) {
            Ok(name) if base.join(&name).exists() => problem(
                &query,
                anyhow::anyhow!(
                    "{} already exists",
                    short(&base.join(&name), home.as_deref())
                ),
            ),
            Ok(name) => add_row(
                &ws,
                &config,
                Target::Clone {
                    url,
                    dest: base.join(name),
                },
                home.as_deref(),
            ),
            Err(e) => problem(&query, e),
        };
        gpui::Task::ready(vec![row])
    })
}

/// A row that says what is wrong; choosing it keeps the page open with the text as typed.
fn problem(query: &str, e: anyhow::Error) -> Entry {
    Entry::new(
        format!("{e:#}"),
        Kind::Action("skyelab::Problem"),
        Rc::new(|_, _| {}),
    )
    .icon(SkyeIcon::CircleAlert)
    .group("Can't add")
    .browse(SharedString::from(query.to_owned()))
}
