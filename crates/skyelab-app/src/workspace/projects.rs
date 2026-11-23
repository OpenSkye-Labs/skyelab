//! Projects and thread tabs: the sidebar's two sections and what selecting a row does.

use super::changes::ChangesView;
use super::thread::{ThreadView, ThreadViewEvent};
use super::worktree;
use super::{AddProject, NewThread, OpenThread, SelectProject, Workspace, account_label, lock};
use gpui::{AppContext as _, Context, Entity, PathPromptOptions, SharedString, Window};
use gpui_component::dock::{DockPlacement, panel_handle};

use skyelab_store::Thread;
use std::path::PathBuf;

use super::panels::{RowAccount, SidebarItem, SidebarSection};
use crate::ui::Status;

impl Workspace {
    /// Lists projects from the store (off the UI thread), selecting `select`, else the current one, else the first.
    pub(super) fn load_projects(&mut self, select: Option<i64>, cx: &mut Context<Self>) {
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.list_projects() });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                // Remote starts waiting for this resolve now, found or not.
                this.starts.loaded();
                match loaded {
                    Ok(projects) => {
                        this.projects = projects;
                        this.resolve_project_accounts(cx);
                        let current = this.project.as_ref().map(|p| p.id);
                        if let Some(id) = select.or(current).or(this.projects.first().map(|p| p.id))
                        {
                            this.select(id, cx);
                        }
                        this.refresh_sidebar(cx);
                    }
                    Err(e) => {
                        this.store_error = Some(format!("{e:#}").into());
                        cx.notify();
                    }
                }
            });
        }));
    }

    /// Stores `dir` as a project, or finds the one already stored there, and selects it.
    pub fn add_project(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move {
            let store = lock(&store)?;
            let normalized: PathBuf = dir.components().collect();
            if let Some(p) = store
                .list_projects()?
                .into_iter()
                .find(|p| p.path == normalized)
            {
                return anyhow::Ok(p.id);
            }
            let name = dir.file_name().map_or_else(
                || dir.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            Ok(store.create_project(&name, &dir)?.id)
        });
        self._add = Some(cx.spawn(async move |this, cx| {
            let added = task.await;
            let _ = this.update(cx, |this, cx| match added {
                Ok(id) => this.load_projects(Some(id), cx),
                Err(e) => {
                    this.store_error = Some(format!("{e:#}").into());
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn pick_project(&mut self, _: &AddProject, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // a local folder picker would add to this machine while another one is shown
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        self._add = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return; // cancelled, or the platform has no picker
            };
            if let Some(dir) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| this.add_project(dir, cx));
            }
        }));
    }

    pub(super) fn select_project(
        &mut self,
        a: &SelectProject,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select(a.id, cx);
    }

    fn select(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(project) = self.projects.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        self.folder = project.path.clone();
        self.folder_label = project.path.display().to_string().into();
        self.account = account_label(&self.config, &self.folder);
        self.account_id = self.config.resolve(&self.folder).map(|a| a.id.clone());
        self.project = Some(project);
        self.threads.clear(); // never list another project's threads while this one's load
        self.refresh_sidebar(cx);
        self.load_threads(cx);
        cx.notify();
    }

    pub(super) fn load_threads(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.project.as_ref().map(|p| p.id) else {
            return;
        };
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.list_threads(project_id) });
        self._threads = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    // A slow load for a project that is no longer selected changes nothing.
                    Ok(threads) if this.project.as_ref().map(|p| p.id) == Some(project_id) => {
                        this.threads = threads
                    }
                    Ok(_) => {}
                    Err(e) => this.store_error = Some(format!("{e:#}").into()),
                }
                this.refresh_sidebar(cx);
                cx.notify();
            });
        }));
    }

    /// Resolves each project's account in the background (it touches the disk), then redraws the sidebar. A newer
    /// call replaces (cancels) an older one, so the latest config wins.
    fn resolve_project_accounts(&mut self, cx: &mut Context<Self>) {
        let config = self.config.clone();
        let projects: Vec<(i64, PathBuf)> = self
            .projects
            .iter()
            .map(|p| (p.id, p.path.clone()))
            .collect();
        let task = cx.background_spawn(async move {
            projects
                .into_iter()
                .map(|(id, path)| (id, config.resolve(&path).cloned()))
                .collect()
        });
        self._project_accounts = Some(cx.spawn(async move |this, cx| {
            let resolved = task.await;
            let _ = this.update(cx, |this, cx| {
                this.project_accounts = resolved;
                this.refresh_sidebar(cx);
            });
        }));
    }

    pub(super) fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        if let Some(sections) = self.remote_sections(cx) {
            return self.sidebar.update(cx, |sidebar, cx| {
                sidebar.sections = sections;
                cx.notify();
            });
        }
        let current = self.project.as_ref().map(|p| p.id);
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
            .filter_map(|v| v.upgrade())
            .filter_map(|v| {
                let v = v.read(cx);
                Some((v.thread.as_ref()?.id, v.live_status()))
            })
            .collect();
        let threads = self
            .threads
            .iter()
            .map(|t| SidebarItem {
                id: format!("thread-{}", t.id).into(),
                label: t.title.clone().into(),
                selected: false,
                action: Box::new(OpenThread { id: t.id }),
                status: Some(
                    live.get(&t.id)
                        .copied()
                        .unwrap_or_else(|| Status::parse(&t.status)),
                ),
                updated_ms: Some(t.updated_at),
                account: None,
            })
            .collect();
        let has_project = self.project.is_some();
        self.sidebar.update(cx, |sidebar, cx| {
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
                    empty_title: "No threads yet".into(),
                    empty_hint: if has_project {
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

    pub(super) fn new_thread(
        &mut self,
        _: &NewThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // `project` is still the last local one: an agent would start here, not on the shown machine
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
        self.add_thread_tab(view, true, window, cx);
    }

    pub(super) fn open_thread(
        &mut self,
        a: &OpenThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.open_view(a.id, cx) {
            if let Some(at) = self.queued_tabs.iter().position(|q| q == &view) {
                self.queued_tabs.remove(at);
                self.dock_thread(&view, window, cx);
                self.refresh_sidebar(cx);
            }
            let id = view.entity_id().into();
            self.dock
                .update(cx, |dock, cx| dock.select_panel(id, window, cx));
            return;
        }
        if let Some(thread) = self.threads.iter().find(|t| t.id == a.id).cloned() {
            return self.open_stored(thread, window, cx);
        }
        // A parent or child link can name a thread in another project.
        let (store, id) = (self.backend.store.clone(), a.id);
        cx.spawn_in(window, async move |this, cx| {
            let row = cx
                .background_spawn(async move { lock(&store)?.thread(id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match row {
                Ok(Some(thread)) if this.open_view(id, cx).is_none() => {
                    this.open_stored(thread, window, cx)
                }
                Ok(_) => {}
                Err(e) => {
                    this.store_error = Some(format!("{e:#}").into());
                    cx.notify();
                }
            });
        })
        .detach(); // one short read; `this` is weak, so a closed window just drops it
    }

    fn open_stored(&mut self, thread: Thread, window: &mut Window, cx: &mut Context<Self>) {
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| ThreadView::open(thread, config, backend, window, cx));
        self.add_thread_tab(view, true, window, cx);
    }

    fn dock_thread(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(view.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
    }

    /// Back on this machine: the tabs that opened meanwhile join the dock.
    pub(super) fn show_queued_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for view in std::mem::take(&mut self.queued_tabs) {
            self.dock_thread(&view, window, cx);
        }
    }

    /// `focus: false` for a tab a remote client opened, so it never takes the local user's keyboard.
    pub(super) fn add_thread_tab(
        &mut self,
        view: Entity<ThreadView>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Its state shows in the sidebar, so redraw that when the tab changes.
        self._thread_events
            .push(cx.observe(&view, |this, _, cx| this.refresh_sidebar(cx)));
        self._thread_events.push(cx.subscribe_in(
            &view,
            window,
            |this, view, e: &ThreadViewEvent, window, cx| match e {
                ThreadViewEvent::Started(thread) => this.thread_started(thread, cx),
                ThreadViewEvent::OpenChanges => this.open_changes(view, window, cx),
                ThreadViewEvent::OpenTerminal { cwd, account_id } => {
                    let (config, cwd, account_id) =
                        (this.accounts_job(cx), cwd.clone(), account_id.clone());
                    let view = view.downgrade();
                    let job = move || {
                        let config = config();
                        let account = config.accounts.iter().find(|a| a.id == account_id).cloned();
                        Ok((
                            worktree::terminal_options(&config, &cwd, &account_id)?,
                            account,
                        ))
                    };
                    this.open_terminal(
                        job,
                        |_, _| true,
                        window,
                        cx,
                        move |_, e, cx| {
                            let _ = view.update(cx, |v, cx| {
                                v.error = Some(format!("{e:#}").into());
                                cx.notify();
                            });
                        },
                    );
                }
                ThreadViewEvent::TurnDone => this.refresh_changes(view, cx),
            },
        ));
        self.open_threads.retain(|t| t.upgrade().is_some());
        self.open_threads.push(view.downgrade());
        // Another machine is shown: a tab of this one would sit among its tabs. It waits until the user is back.
        if !focus && self.is_remote() {
            self.queued_tabs.push(view);
            self.refresh_sidebar(cx);
            cx.notify();
            return;
        }
        self.dock_thread(&view, window, cx);
        if focus {
            view.update(cx, |v, cx| v.focus_prompt(window, cx));
        }
        cx.notify();
    }

    /// The Changes tab for the thread's folder, in the right dock: refreshed and selected if one is open, else added.
    pub(super) fn open_changes(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = view.read(cx).cwd.clone();
        let base = view
            .read(cx)
            .thread
            .as_ref()
            .and_then(|t| t.worktree_base.clone());
        self.changes_views.retain(|c| c.upgrade().is_some());
        let open = self
            .changes_views
            .iter()
            .filter_map(|c| c.upgrade())
            .find(|c| c.read(cx).dir == dir && c.read(cx).base == base);
        if let Some(changes) = open {
            // One tab per folder; it publishes for (and saves the PR link on) the thread that opened it last.
            // Same folder, same bound account, and publishing re-checks the account anyway.
            let thread = view.downgrade();
            changes.update(cx, |c, cx| {
                c.thread = Some(thread);
                c.refresh(cx)
            });
            self.show_right_panel(changes.entity_id().into(), window, cx);
            return;
        }
        let title = view.read(cx).title();
        let changes = cx.new(|cx| ChangesView::new(dir, base, title, Some(view), window, cx));
        self.add_right_panel(changes.clone(), window, cx);
        self.changes_views.push(changes.downgrade());
        cx.notify();
    }

    /// A thread's turn ended: every Changes tab on its folder may be out of date.
    fn refresh_changes(&mut self, view: &Entity<ThreadView>, cx: &mut Context<Self>) {
        let dir = view.read(cx).cwd.clone();
        for changes in self.changes_views.iter().filter_map(|c| c.upgrade()) {
            if changes.read(cx).dir == dir {
                changes.update(cx, |c, cx| c.refresh(cx));
            }
        }
    }

    fn thread_started(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        #[cfg(unix)]
        self.child_started(thread, cx);
        if self.project.as_ref().map(|p| p.id) == Some(thread.project_id) {
            self.threads.insert(0, thread.clone());
            self.refresh_sidebar(cx);
        }
    }

    /// Settings saved: open threads start their next session from the new bindings.
    pub(super) fn push_config(&mut self, cx: &mut Context<Self>) {
        crate::ui::set_accounts(cx, &self.config);
        // Show nothing rather than the previous binding until the new one is resolved.
        self.project_accounts.clear();
        self.resolve_project_accounts(cx);
        super::orchestrate::set_config(cx, &self.config);
        let config = self.config.clone();
        for view in self.open_threads.iter().filter_map(|t| t.upgrade()) {
            view.update(cx, |t, cx| t.set_config(config.clone(), cx));
        }
    }
}
