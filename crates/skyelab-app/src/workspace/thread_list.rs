//! The sidebar's rows: projects, and the selected project's threads in their order, the archived split, and the
//! thread filter.

use super::panels::{ArchivedRows, RowAccount, RowMarks, SidebarItem, SidebarSection};
use super::{AddProject, NewThread, OpenThread, SelectProject, Workspace, lock};
use crate::ui::Status;
use gpui::{AppContext as _, Context, SharedString};
use skyelab_store::Thread;

impl Workspace {
    pub(super) fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Some(sections) = self.remote_sections(cx) {
            return self.sidebar.update(cx, |sidebar, cx| {
                sidebar.sections = sections;
                sidebar.marks.clear();
                sidebar.archived = None;
                sidebar.searchable = false;
                cx.notify();
            });
        }
        let current = self.project.as_ref().map(|p| p.id);
        // A project that isn't selected shows its unread threads on its row: the stored ones as of the last thread
        // load, and the open views marked since.
        let unread_projects: std::collections::HashSet<i64> = self
            .open_threads
            .iter()
            .filter_map(|v| v.read(cx).thread.as_ref().filter(|t| t.unread))
            .map(|t| t.project_id)
            .chain(self.unread_projects.iter().copied())
            .filter(|&p| Some(p) != current)
            .collect();
        let mut marks = std::collections::HashMap::new();
        for p in &self.projects {
            marks.insert(
                SharedString::from(format!("project-{}", p.id)),
                RowMarks {
                    unread: unread_projects.contains(&p.id),
                    menu: super::manage::project_menu(p.id),
                    ..Default::default()
                },
            );
        }
        let projects = self
            .projects
            .iter()
            .map(|p| SidebarItem {
                id: format!("project-{}", p.id).into(),
                label: p.name.clone().into(),
                selected: Some(p.id) == current,
                action: Box::new(SelectProject { id: p.id }),
                status: None,
                updated_ms: None,
                account: Some(match self.project_accounts.get(&p.id) {
                    Some(Some(a)) => RowAccount::Bound(a.clone()),
                    Some(None) => RowAccount::Unbound,
                    None => RowAccount::Hidden,
                }),
            })
            .collect();
        // An open tab knows its thread's state now; the row is only as fresh as the last project load.
        let live: std::collections::HashMap<i64, Status> = self
            .open_threads
            .iter()
            .filter_map(|v| {
                let v = v.read(cx);
                Some((v.thread.as_ref()?.id, v.live_status()))
            })
            .collect();
        let shown = self.shown_thread(cx);
        let mut item = |t: &Thread| {
            let id: SharedString = format!("thread-{}", t.id).into();
            marks.insert(
                id.clone(),
                RowMarks {
                    pinned: t.pinned_at.is_some(),
                    unread: t.unread,
                    menu: super::manage::thread_menu(t),
                },
            );
            SidebarItem {
                id,
                label: t.title.clone().into(),
                selected: Some(t.id) == shown,
                action: Box::new(OpenThread { id: t.id }),
                status: Some(
                    live.get(&t.id)
                        .copied()
                        .unwrap_or_else(|| Status::parse(&t.status)),
                ),
                updated_ms: Some(t.updated_at),
                account: None,
            }
        };
        let (listed, archived) = self.listed_threads();
        let threads: Vec<SidebarItem> = listed.into_iter().map(&mut item).collect();
        let archived = ArchivedRows {
            items: archived.into_iter().map(&mut item).collect(),
            open: self.show_archived,
            toggle: Box::new(super::manage::ToggleArchivedThreads),
        };
        let has_project = self.project.is_some();
        let searching = self.thread_search.is_some();
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.marks = marks;
            sidebar.archived = Some(archived);
            sidebar.searchable = true;
            sidebar.sections = vec![
                SidebarSection {
                    title: "Projects".into(),
                    items: projects,
                    empty_title: "No projects yet".into(),
                    empty_hint: "Add a folder to start. It runs on the account it is bound to."
                        .into(),
                    button: Some(("Add".into(), Box::new(AddProject))),
                    nested: false,
                },
                SidebarSection {
                    title: "Threads".into(),
                    items: threads,
                    empty_title: if searching {
                        "No matching threads"
                    } else {
                        "No threads yet"
                    }
                    .into(),
                    empty_hint: if searching {
                        "Titles and messages are searched."
                    } else if has_project {
                        "Start one with New thread."
                    } else {
                        "Select a project to see its threads."
                    }
                    .into(),
                    button: has_project.then(|| {
                        (
                            SharedString::from("New thread"),
                            Box::new(NewThread) as Box<_>,
                        )
                    }),
                    nested: true,
                },
            ];
            cx.notify();
        });
    }

    /// The selected project's threads as the sidebar lists them: (pinned first, then by last change; the
    /// archived ones), both narrowed by the thread filter.
    pub(super) fn listed_threads(&self) -> (Vec<&Thread>, Vec<&Thread>) {
        // A title is checked live (rows change after a search ran); message matches come from the last search,
        // which reruns whenever the thread list reloads.
        let matches = |t: &&Thread| match &self.thread_search {
            None => true,
            Some((q, ids)) => {
                t.title.to_lowercase().contains(&q.to_lowercase())
                    || ids.as_ref().is_some_and(|ids| ids.contains(&t.id))
            }
        };
        let (mut listed, archived): (Vec<&Thread>, Vec<&Thread>) = self
            .threads
            .iter()
            .filter(matches)
            .partition(|t| t.archived_at.is_none());
        listed.sort_by_key(|t| t.pinned_at.is_none()); // stable, so each group keeps its order
        (listed, archived)
    }

    /// The order thread keys walk (previous/next, 1–9): the sidebar's, archived ones left out.
    pub(super) fn thread_order(&self) -> Vec<i64> {
        self.listed_threads().0.iter().map(|t| t.id).collect()
    }

    /// The thread list changed (a load, a project switch, a finished turn): run the filter's search again.
    pub(super) fn rerun_search(&mut self, cx: &mut Context<Self>) {
        if let Some((q, _)) = &self.thread_search {
            let q = SharedString::from(q.clone());
            self.search_threads(q, cx);
        }
    }

    /// The sidebar's filter changed: search titles and message text of the selected project's threads.
    pub(super) fn search_threads(&mut self, query: SharedString, cx: &mut Context<Self>) {
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.thread_search = None;
            self._search = None;
            return self.refresh_sidebar(cx);
        }
        // Keep the last answer until the new one lands, so rows don't flicker on a rerun.
        let kept = match self.thread_search.take() {
            Some((q, ids)) if q == query => ids,
            _ => None,
        };
        self.thread_search = Some((query.clone(), kept));
        self.refresh_sidebar(cx);
        let (store, project) = (
            self.backend.store.clone(),
            self.project.as_ref().map(|p| p.id),
        );
        let task =
            cx.background_spawn(async move { lock(&store)?.search_threads(&query, project, 500) });
        self._search = Some(cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                match found {
                    Ok(found) => {
                        if let Some((_, ids)) = this.thread_search.as_mut() {
                            *ids = Some(found.into_iter().map(|t| t.id).collect());
                        }
                    }
                    Err(e) => this.store_error = Some(format!("{e:#}").into()),
                }
                this.refresh_sidebar(cx);
            });
        }));
    }
}
