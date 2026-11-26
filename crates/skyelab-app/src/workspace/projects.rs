//! Projects and threads: the sidebar's two sections and what selecting a row does.

use super::center::Page;
use super::thread::{ThreadView, ThreadViewEvent};
use super::worktree;
use super::{
    AddProject, NewThread, NewThreadIn, OpenThread, SelectProject, Workspace, account_label, lock,
};
use gpui::{AppContext as _, Context, Entity, Window};

use skyelab_store::Thread;

/// This machine's shell, read once (the snapshot of a subscription nobody follows).
fn shell(local: &skyelab_env::LocalSource) -> anyhow::Result<skyelab_protocol::ShellSnapshot> {
    use skyelab_env::Source as _;
    Ok(local.subscribe_shell()?.snapshot)
}

#[path = "projects_add.rs"]
mod projects_add;

#[path = "project_name.rs"]
mod project_name;
pub(in crate::workspace) use project_name::name_project;

impl Workspace {
    /// Lists projects from the store (off the UI thread), selecting `select`, else the current one, else the first.
    pub(super) fn load_projects(&mut self, select: Option<i64>, cx: &mut Context<Self>) {
        let local = self.local.clone();
        let task = cx.background_spawn(async move { shell(&local).map(|s| s.projects) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                // Remote starts waiting for this resolve now, found or not.
                this.starts.loaded();
                match loaded {
                    Ok(projects) => {
                        this.projects = projects.iter().map(skyelab_env::store_project).collect();
                        this.sync_project_names(cx);
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
                this.setup.loaded = true;
                this.sync_setup(cx);
            });
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
        // Only over the app's own backend (main.rs): a window's fallback one (tests) would start a stream per window.
        if self._local_shell.is_none() && cx.try_global::<super::LocalBackend>().is_some() {
            self.follow_local_shell(cx);
        }
        let local = self.local.clone();
        let task = cx.background_spawn(async move { shell(&local).map(|s| s.threads) });
        self._threads = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(threads) => {
                        crate::debug_mode::launch_milestone("threads listed");
                        // The shell carries each thread's last reply: only the branches are read again.
                        this.previews.replies = threads
                            .iter()
                            .filter_map(|t| Some((t.id, t.preview.clone()?)))
                            .collect();
                        this.threads = threads.iter().map(skyelab_env::store_thread).collect();
                        this.rerun_search(cx);
                        this.load_previews(false, cx);
                        this.poll_prs(true, cx); // whichever page shows
                    }
                    Err(e) => this.store_error = Some(format!("{e:#}").into()),
                }
                this.refresh_sidebar(cx);
                cx.notify();
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

    /// The palette as a project picker: "New thread in: <project>" for each one, then Add project….
    fn pick_new_thread_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Nothing focused (onboarding just closed): Back returns to the workspace.
        let prev = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let mut entries = self.new_thread_in_entries();
        entries.push(
            crate::palette::Entry::new(
                "Add project…",
                crate::palette::Kind::Action("skyelab::AddProject"),
                std::rc::Rc::new(|_, _| {}),
            )
            .icon(crate::ui::SkyeIcon::FolderPlus)
            .subtitle("Add a folder, create a project or clone one")
            .page(self.add_project_page(cx)),
        );
        crate::palette::open_page("New thread in", entries, prev, window, cx);
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
                // Shows another window's view if one opened it meanwhile (two windows restoring one thread).
                Ok(Some(thread)) => {
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
        if self.show_shared(thread.id, window, cx) {
            return;
        }
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| ThreadView::open(thread, config, backend, window, cx));
        name_project(&self.projects, &view, cx);
        self.add_thread_tab(view, true, window, cx);
    }

    /// Open threads name their project as the sidebar does, after a rename too.
    pub(super) fn sync_project_names(&self, cx: &mut Context<Self>) {
        for view in &self.open_threads {
            name_project(&self.projects, view, cx);
        }
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
        self.track_thread(&view, window, cx);
        self.share_thread(&view, cx);
        if focus {
            return self.show(Page::Thread(view.downgrade()), window, cx);
        }
        view.update(cx, |v, _| v.background = true);
        self.refresh_sidebar(cx);
        cx.notify();
    }

    /// Follows `view` in this window: its row in the sidebar and its events. Only the window that owns the view
    /// (last showed it) acts on what the user did in it or on what needs one answer (a notification, a script).
    pub(super) fn track_thread(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        view.update(cx, |v, _| v.lists.add(window.window_handle()));
        // Its state shows in the sidebar: rebuild that when what the row shows changed, not on every notify.
        let observe = cx.observe_in(view, window, |this, view, window, cx| {
            this.thread_changed(&view, window, cx)
        });
        let events = cx.subscribe_in(
            view,
            window,
            |this, view, e: &ThreadViewEvent, window, cx| match e {
                ThreadViewEvent::Started(thread) => {
                    this.thread_started(thread, cx);
                    this.retitle_changes(view, cx);
                    // A draft just got its row, so the saved page can name it now.
                    if this.current_thread().as_ref() == Some(view) {
                        this.schedule_layout_save(window, cx);
                    } else if this.started_unseen(view, thread.id, window, cx) {
                        this.mark_unread(view, cx);
                    }
                }
                ThreadViewEvent::TurnDone => {
                    this.refresh_changes(view, cx);
                    this.refresh_details(cx);
                    this.load_previews(true, cx);
                    if view.read(cx).owned_by(window) {
                        this.turn_done(view, window, cx);
                    }
                }
                ThreadViewEvent::FilesChanged => this.refresh_changes(view, cx),
                ThreadViewEvent::Retitled(title) => {
                    this.retitle_changes(view, cx);
                    if let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) {
                        this.update_row(id, |t| t.title = title.clone(), cx);
                    }
                }
                ThreadViewEvent::UsageReported => this.refresh_usage(cx),
                ThreadViewEvent::LimitResume(at) => this.limit_resume_changed(view, *at, cx),
                ThreadViewEvent::ScriptsChanged => this.load_scripts(cx),
                _ if !view.read(cx).owned_by(window) => {}
                ThreadViewEvent::Side(ask) => this.side_chat(view, ask.clone(), window, cx),
                ThreadViewEvent::Compare(account) => this.compare_send(view, account, window, cx),
                ThreadViewEvent::OpenChanges => _ = this.open_changes(view, window, cx),
                ThreadViewEvent::Git(action) => {
                    let tab = this.open_changes(view, window, cx);
                    tab.update(cx, |c, cx| c.run_git(*action, window, cx));
                }
                ThreadViewEvent::OpenPath(path) => this.open_path(view, path, window, cx),
                ThreadViewEvent::Preview(path, line) => {
                    let root = view.read(cx).cwd.clone();
                    this.open_preview(root, path.clone(), *line, window, cx)
                }
                ThreadViewEvent::OpenTurnPath(turn, path) => {
                    let changes = this.open_changes(view, window, cx);
                    changes.update(cx, |c, cx| {
                        c.reveal_after_load = Some(path.clone());
                        c.set_scope(super::changes_model::Scope::Turn(*turn), cx);
                    });
                }
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
                ThreadViewEvent::McpSignIn(server) => {
                    this.mcp_sign_in(view.clone(), server.clone(), window, cx)
                }
                ThreadViewEvent::RunScript(script) => {
                    let v = view.read(cx);
                    let account = v.thread.as_ref().map(|t| t.account_id.clone());
                    let cwd = v.cwd.clone();
                    this.run_script(script, cwd, account, window, cx);
                }
                ThreadViewEvent::PrActed => {
                    if let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) {
                        this.reread_pr(id, cx);
                    }
                }
                ThreadViewEvent::PrUnlinked(url) => {
                    if let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) {
                        this.pr_unlinked(id, url.clone(), window, cx);
                    }
                }
                ThreadViewEvent::WorktreeCreated { cwd, account_id } => {
                    let project = view.read(cx).project_id;
                    this.run_setup_scripts(project, cwd.clone(), account_id.clone(), window, cx);
                }
            },
        );
        let sig = super::thread_rows::row_sig(view.read(cx));
        let subs = super::thread_rows::ThreadSubs {
            sig,
            _subs: [observe, events],
        };
        self.thread_subs.insert(view.entity_id(), subs);
        view.update(cx, |v, cx| {
            v.remote_window = super::env_mode::shows_remote(cx, v.window)
        });
        self.open_threads.push(view.clone());
    }

    pub(super) fn thread_started(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        self.child_started(thread, cx);
        if self.projects.iter().any(|p| p.id == thread.project_id) {
            // A reopened thread reports its start too: keep its one row where it is.
            match self.threads.iter_mut().find(|t| t.id == thread.id) {
                Some(row) => *row = thread.clone(),
                None => {
                    self.threads.insert(0, thread.clone());
                    let id = format!("thread-{}", thread.id).into();
                    let now = std::time::Instant::now();
                    self.sidebar.update(cx, |s, _| s.appeared = Some((id, now)));
                }
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
        self.push_side_config(cx);
        self.sync_setup(cx);
    }
}
