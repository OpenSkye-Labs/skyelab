//! Projects and threads: the sidebar's two sections and what selecting a row does.

use super::center::Page;
use super::changes::ChangesView;
use super::thread::{ThreadView, ThreadViewEvent};
use super::worktree;
use super::{
    AddProject, NewThread, NewThreadIn, OpenThread, SelectProject, Workspace, account_label, lock,
};
use gpui::{AppContext as _, Context, Entity, PathPromptOptions, Window};

use skyelab_store::Thread;
use std::path::PathBuf;

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
                        let listed = &this.projects;
                        this.scope.retain(|id| listed.iter().any(|p| p.id == *id));
                        this.resolve_project_accounts(cx);
                        this.load_threads(cx);
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
            // A removed project comes back with its threads.
            if let Some(p) = store.readd_project(&dir)? {
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
                Ok(id) => {
                    // The new project's threads must show, and New thread must not start in an older scope.
                    this.scope.clear();
                    this.load_projects(Some(id), cx)
                }
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
        // "Switch to project" narrows the list to it, as the scope filter would.
        self.scope = vec![a.id];
        self.select(a.id, cx);
    }

    pub(super) fn select(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(project) = self.projects.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        self.folder = project.path.clone();
        self.folder_label = project.path.display().to_string().into();
        self.account = account_label(&self.config, &self.folder);
        self.account_id = self.config.resolve(&self.folder).map(|a| a.id.clone());
        self.project = Some(project);
        self.refresh_sidebar(cx);
        self.scripts.clear();
        self.load_scripts(cx);
        cx.notify();
    }

    /// Every listed project's threads, for the sidebar's one list. A newer load replaces (cancels) an older one.
    pub(super) fn load_threads(&mut self, cx: &mut Context<Self>) {
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.list_all_threads() });
        self._threads = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(threads) => {
                        this.threads = threads;
                        this.rerun_search(cx);
                        this.load_previews(cx);
                    }
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

    pub(super) fn new_thread(
        &mut self,
        _: &NewThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // `project` is still the last local one: an agent would start here, not on the shown machine
        }
        // T3's rule: the one project the list is narrowed to, else the only project, else ask which.
        let target = match (self.scope.as_slice(), self.projects.as_slice()) {
            ([id], _) => Some(*id),
            (_, [only]) => Some(only.id),
            _ => None,
        };
        match target {
            Some(id) => self.new_thread_in(&NewThreadIn { id }, window, cx),
            None if self.projects.is_empty() => self.pick_project(&AddProject, window, cx),
            None => self.pick_new_thread_project(window, cx),
        }
    }

    /// The palette as a project picker: "New thread in: <project>" for each one.
    fn pick_new_thread_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prev) = window.focused(cx) else {
            return;
        };
        let entries = self
            .projects
            .iter()
            .map(|p| {
                let id = p.id;
                crate::palette::Entry {
                    label: format!("New thread in: {}", p.name).into(),
                    kind: crate::palette::Kind::Project,
                    binding: None,
                    checked: None,
                    run: std::rc::Rc::new(move |window, cx| {
                        window.dispatch_action(Box::new(NewThreadIn { id }), cx)
                    }),
                }
            })
            .collect();
        crate::palette::open(entries, None, prev, window, cx);
    }

    /// A new thread in project `id`. Its folder's bound account is resolved when the thread starts, never defaulted.
    pub(super) fn new_thread_in(
        &mut self,
        a: &NewThreadIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return;
        }
        if self.project.as_ref().map(|p| p.id) != Some(a.id) {
            self.select(a.id, cx);
        }
        let Some(project) = self.project.clone().filter(|p| p.id == a.id) else {
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
        // The workspace follows the opened thread's project (folder, account, terminals), as a selected one did.
        let project = self
            .threads
            .iter()
            .find(|t| t.id == a.id)
            .map(|t| t.project_id)
            .or_else(|| self.open_view(a.id, cx).map(|v| v.read(cx).project_id));
        if let Some(id) = project.filter(|&id| self.project.as_ref().map(|p| p.id) != Some(id)) {
            self.select(id, cx);
        }
        if let Some(view) = self.open_view(a.id, cx) {
            return self.show(Page::Thread(view.downgrade()), window, cx);
        }
        if let Some(thread) = self.threads.iter().find(|t| t.id == a.id).cloned() {
            return self.open_stored(thread, window, cx);
        }
        // A parent or child link can name a thread in another project.
        let (store, id) = (self.backend.store.clone(), a.id);
        cx.spawn_in(window, async move |this, cx| {
            let row = cx
                .background_spawn(async move { lock(&store)?.listed_thread(id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match row {
                Ok(Some(thread)) if this.open_view(id, cx).is_none() => {
                    if this.project.as_ref().map(|p| p.id) != Some(thread.project_id) {
                        this.select(thread.project_id, cx);
                    }
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

    /// Keeps `view` alive and shows it. `focus: false` for a thread that started in the background (delegated, or
    /// from another device): the page stays, and the thread's row is marked unread until the user opens it.
    pub(super) fn add_thread_tab(
        &mut self,
        view: Entity<ThreadView>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Its state shows in the sidebar, so redraw that when the tab changes.
        self._thread_events
            .push(cx.observe(&view, |this, view, cx| {
                this.sync_lineage(&view, cx);
                this.refresh_sidebar(cx)
            }));
        self._thread_events.push(cx.subscribe_in(
            &view,
            window,
            |this, view, e: &ThreadViewEvent, window, cx| match e {
                ThreadViewEvent::Started(thread) => {
                    this.thread_started(thread, cx);
                    // A draft just got its row, so the saved page can name it now.
                    if this.current_thread().as_ref() == Some(view) {
                        this.schedule_layout_save(window, cx);
                    } else if view.read(cx).background {
                        this.mark_unread(view, cx);
                    }
                }
                ThreadViewEvent::OpenChanges => this.open_changes(view, window, cx),
                ThreadViewEvent::OpenPath(path) => this.open_path(view, path, window, cx),
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
                ThreadViewEvent::TurnDone => {
                    this.refresh_changes(view, cx);
                    this.refresh_details(cx);
                    this.load_previews(cx);
                    this.turn_done(view, window, cx);
                }
                ThreadViewEvent::FilesChanged => this.refresh_changes(view, cx),
                ThreadViewEvent::RunScript(script) => {
                    let v = view.read(cx);
                    let account = v.thread.as_ref().map(|t| t.account_id.clone());
                    let cwd = v.cwd.clone();
                    this.run_script(script, cwd, account, window, cx);
                }
                ThreadViewEvent::ScriptsChanged => this.load_scripts(cx),
                ThreadViewEvent::WorktreeCreated { cwd, account_id } => {
                    let project = view.read(cx).project_id;
                    this.run_setup_scripts(project, cwd.clone(), account_id.clone(), window, cx);
                }
            },
        ));
        self.open_threads.push(view.clone());
        if focus {
            return self.show(Page::Thread(view.downgrade()), window, cx);
        }
        view.update(cx, |v, _| v.background = true);
        self.refresh_sidebar(cx);
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

    /// A file the thread's timeline names: its folder's Changes tab, scrolled to the file once the diff lists it.
    fn open_path(
        &mut self,
        view: &Entity<ThreadView>,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_changes(view, window, cx);
        let dir = view.read(cx).cwd.clone();
        let base = view
            .read(cx)
            .thread
            .as_ref()
            .and_then(|t| t.worktree_base.clone());
        let tab = self
            .changes_views
            .iter()
            .filter_map(|c| c.upgrade())
            .find(|c| c.read(cx).dir == dir && c.read(cx).base == base);
        if let Some(changes) = tab {
            let path = path.to_owned();
            changes.update(cx, |c, cx| {
                // A refresh is on its way (open_changes started it): reveal against what it loads.
                c.reveal_after_load = Some(path.clone());
                c.reveal(&path, cx);
            });
        }
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
        if self.projects.iter().any(|p| p.id == thread.project_id) {
            // A reopened thread reports its start too: keep its one row where it is.
            match self.threads.iter_mut().find(|t| t.id == thread.id) {
                Some(row) => *row = thread.clone(),
                None => self.threads.insert(0, thread.clone()),
            }
            self.rerun_search(cx);
            self.refresh_sidebar(cx);
        }
    }

    /// Settings saved: open threads start their next session from the new bindings.
    pub(super) fn push_config(&mut self, cx: &mut Context<Self>) {
        crate::ui::set_accounts(cx, &self.config);
        self.refresh_details(cx);
        // Show nothing rather than the previous binding until the new one is resolved.
        self.project_accounts.clear();
        self.resolve_project_accounts(cx);
        super::orchestrate::set_config(cx, &self.config);
        let config = self.config.clone();
        for view in &self.open_threads {
            view.update(cx, |t, cx| t.set_config(config.clone(), cx));
        }
    }
}
