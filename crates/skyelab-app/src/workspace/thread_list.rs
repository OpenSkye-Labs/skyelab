//! The sidebar's rows: every project's threads in one list (pinned, working, the rest), the archived split, the
//! project scope filter and the thread filter.

use super::panels::{
    ArchivedRows, RowAccount, RowMarks, ScopeMenu, ScopeProject, SidebarItem, SidebarSection,
};
use super::{AddProject, OpenThread, ShowAllProjects, Workspace, lock};
use crate::ui::Status;
use gpui::{Action, AppContext as _, Context, SharedString, Window};
use skyelab_store::Thread;

/// Adds the project to the sidebar's scope filter, or takes it out.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ToggleProjectScope {
    pub id: i64,
}

/// New thread in this project (the project picker's entries).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct NewThreadIn {
    pub id: i64,
}

impl Workspace {
    pub(super) fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Some(sections) = self.remote_sections(cx) {
            return self.sidebar.update(cx, |sidebar, cx| {
                sidebar.sections = sections;
                sidebar.marks.clear();
                sidebar.archived = None;
                sidebar.searchable = false;
                sidebar.scope = None;
                cx.notify();
            });
        }
        // An open tab knows its thread's state now; the row is only as fresh as the last load.
        let live: std::collections::HashMap<i64, (Status, bool)> = self
            .open_threads
            .iter()
            .filter_map(|v| v.upgrade())
            .filter_map(|v| {
                let v = v.read(cx);
                Some((
                    v.thread.as_ref()?.id,
                    (v.live_status(), v.awaiting_approval()),
                ))
            })
            .collect();
        let names: std::collections::HashMap<i64, &str> = self
            .projects
            .iter()
            .map(|p| (p.id, p.name.as_str()))
            .collect();
        let shown = self.shown_thread(cx);
        let mut marks = std::collections::HashMap::new();
        let mut item = |t: &Thread| {
            let id: SharedString = format!("thread-{}", t.id).into();
            let (status, approval) = live
                .get(&t.id)
                .copied()
                .unwrap_or_else(|| (Status::parse(&t.status), false));
            marks.insert(
                id.clone(),
                RowMarks {
                    pinned: t.pinned_at.is_some(),
                    unread: t.unread,
                    approval,
                    menu: super::manage::thread_menu(t),
                },
            );
            SidebarItem {
                id,
                label: t.title.clone().into(),
                selected: Some(t.id) == shown,
                action: Box::new(OpenThread { id: t.id }),
                status: Some(status),
                updated_ms: Some(t.updated_at),
                account: Some(match self.project_accounts.get(&t.project_id) {
                    Some(Some(a)) => RowAccount::Bound(a.clone()),
                    Some(None) => RowAccount::Unbound,
                    None => RowAccount::Hidden,
                }),
                project: Some(
                    names
                        .get(&t.project_id)
                        .copied()
                        .unwrap_or_default()
                        .to_owned()
                        .into(),
                ),
            }
        };
        let groups = self.thread_groups(cx);
        let [pinned, working, rest] =
            groups.map(|g| g.into_iter().map(&mut item).collect::<Vec<_>>());
        let archived = ArchivedRows {
            items: self.listed_threads().1.into_iter().map(&mut item).collect(),
            open: self.show_archived,
            toggle: Box::new(super::manage::ToggleArchivedThreads),
        };
        let scope = ScopeMenu {
            projects: self
                .projects
                .iter()
                .map(|p| ScopeProject {
                    id: p.id,
                    name: p.name.clone().into(),
                    checked: self.scope.contains(&p.id),
                })
                .collect(),
        };
        let searching = self.thread_search.is_some();
        let no_projects = self.projects.is_empty();
        let section = |title: &str, items| SidebarSection {
            title: SharedString::from(title.to_owned()),
            items,
            empty_title: "".into(),
            empty_hint: "".into(),
            button: None,
            nested: false,
        };
        let mut sections = Vec::new();
        if !pinned.is_empty() {
            sections.push(section("Pinned", pinned));
        }
        if !working.is_empty() {
            sections.push(section("Working", working));
        }
        sections.push(SidebarSection {
            empty_title: if searching {
                "No matching threads"
            } else if no_projects {
                "No projects yet"
            } else {
                "No threads yet"
            }
            .into(),
            empty_hint: if searching {
                "Titles and messages are searched."
            } else if no_projects {
                "Add a folder to start. It runs on the account it is bound to."
            } else {
                "Start one with New thread."
            }
            .into(),
            button: no_projects.then(|| {
                (
                    SharedString::from("Add project"),
                    Box::new(AddProject) as Box<_>,
                )
            }),
            ..section("Threads", rest)
        });
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.marks = marks;
            sidebar.archived = Some(archived);
            sidebar.searchable = true;
            sidebar.scope = Some(scope);
            sidebar.sections = sections;
            cx.notify();
        });
    }

    /// The listed threads in the sidebar's three groups: pinned, working (running or starting), the rest. Each keeps
    /// the newest-first order.
    fn thread_groups(&self, cx: &gpui::App) -> [Vec<&Thread>; 3] {
        let working: std::collections::HashSet<i64> = self
            .open_threads
            .iter()
            .filter_map(|v| v.upgrade())
            .filter_map(|v| {
                let v = v.read(cx);
                matches!(v.live_status(), Status::Starting | Status::Running)
                    .then(|| v.thread.as_ref().map(|t| t.id))?
            })
            .collect();
        let mut groups: [Vec<&Thread>; 3] = Default::default();
        for t in self.listed_threads().0 {
            let at = if t.pinned_at.is_some() {
                0
            } else if working.contains(&t.id) {
                1
            } else {
                2
            };
            groups[at].push(t);
        }
        groups
    }

    /// The listed threads (in the scope, matching the filter) as (not archived, archived), newest first.
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
        let in_scope = |t: &&Thread| self.scope.is_empty() || self.scope.contains(&t.project_id);
        self.threads
            .iter()
            .filter(in_scope)
            .filter(matches)
            .partition(|t| t.archived_at.is_none())
    }

    /// The scope filter's check row: the project joins the filter, or leaves it.
    pub(super) fn toggle_project_scope(
        &mut self,
        a: &ToggleProjectScope,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.scope.iter().position(|&id| id == a.id) {
            Some(at) => {
                self.scope.remove(at);
            }
            None => self.scope.push(a.id),
        }
        // Narrowed to one project: the workspace (status bar, terminals, project actions) follows it.
        if let [id] = self.scope[..] {
            return self.select(id, cx);
        }
        self.refresh_sidebar(cx);
    }

    pub(super) fn show_all_projects(
        &mut self,
        _: &ShowAllProjects,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scope.clear();
        self.refresh_sidebar(cx);
    }

    /// The order thread keys walk (previous/next, 1–9): the sidebar's, archived ones left out.
    pub(super) fn thread_order(&self, cx: &gpui::App) -> Vec<i64> {
        self.thread_groups(cx)
            .concat()
            .iter()
            .map(|t| t.id)
            .collect()
    }

    /// The thread list changed (a load, a project switch, a finished turn): run the filter's search again.
    pub(super) fn rerun_search(&mut self, cx: &mut Context<Self>) {
        if let Some((q, _)) = &self.thread_search {
            let q = SharedString::from(q.clone());
            self.search_threads(q, cx);
        }
    }

    /// The sidebar's filter changed: search titles and message text of every project's threads.
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
        let store = self.backend.store.clone();
        let task =
            cx.background_spawn(async move { lock(&store)?.search_threads(&query, None, 500) });
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
