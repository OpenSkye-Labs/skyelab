//! The shown thread's details, as the floating thread details card (T3's `ThreadDetailsPanel`): this machine and the
//! thread's folder, branch and git actions, scripts, the agents it delegated, every account's usage, its pull request
//! and editor. Everything acts on the thread's own folder under its bound account, so the header stays a breadcrumb.
//!
//! Pickers (branch, scripts, editors, finished agents) open as pages inside the card.

use super::Workspace;
use super::thread::ThreadView;
use crate::ui::Status;
use gpui::{Action, AppContext as _, Context, Entity, SharedString, Task};
use std::path::Path;

/// Click handlers that update the thread view, like `cx.listener`, for a card drawn from a read borrow: the Details
/// tab draws the card without updating the thread view inside its render.
pub(in crate::workspace) struct On(pub gpui::WeakEntity<ThreadView>);

impl On {
    pub fn click(
        &self,
        f: impl Fn(&mut ThreadView, &mut gpui::Window, &mut Context<ThreadView>) + 'static,
    ) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
        let view = self.0.clone();
        move |_, window, cx| _ = view.update(cx, |v, cx| f(v, window, cx))
    }
}

#[path = "thread_details_card.rs"]
mod card;
#[path = "thread_details_git.rs"]
mod git;
#[path = "thread_details_pr.rs"]
mod pr;
#[path = "thread_details_remote.rs"]
pub(super) mod remote;
#[cfg(test)]
#[path = "thread_details_test.rs"]
mod tests;

/// Opens the shown thread's folder in `editor` and keeps it as the editor setting.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenInEditorWith {
    pub editor: String,
}

/// Editors offered in the chooser when they are on PATH.
const KNOWN_EDITORS: [&str; 5] = ["code", "cursor", "zed", "windsurf", "subl"];

/// A thread this one delegated, listed under Lineage in the details card.
#[derive(Debug, Clone, PartialEq)]
pub struct ChildLink {
    pub id: i64,
    pub title: SharedString,
    pub status: crate::ui::Status,
    /// When it was created, for how long it has run.
    pub started_ms: i64,
}

#[derive(Default)]
pub struct Details {
    page: Page,
    machine: Option<SharedString>,
    /// Lines added and removed in the thread's folder since its base (`None` until loaded, or not a repo).
    pub(in crate::workspace) stat: Option<(u64, u64)>,
    editors: Option<Editors>,
    /// This thread's side chats (id, title), newest first.
    pub(in crate::workspace) side_chats: Vec<(i64, SharedString)>,
    /// The MCP servers of the thread's latest agent process, as it reported them.
    pub(in crate::workspace) mcp: Vec<skyelab_agent::McpServer>,
    /// The threads this one started on other machines, as recorded here.
    pub(in crate::workspace) remote: Vec<skyelab_store::RemoteChild>,
    /// Who the Commit & push menu pushes as: the account's GitHub login, or why there is none.
    pub(in crate::workspace) gh_login: Option<SharedString>,
    /// The "Link pull request…" field and what was wrong with the last link.
    pr_input: Option<Entity<crate::ui::TextInput>>,
    pr_error: Option<SharedString>,
    /// Commits ahead of and behind the branch's upstream; `None` without one (or not read yet).
    sync: Option<(u32, u32)>,
    /// Why the last background fetch failed.
    fetch_error: Option<SharedString>,
    /// When the last fetch started (ms since the epoch), for Settings → Threads' interval.
    fetched_ms: i64,
    fetching: bool,
    _fetch: Option<Task<()>>,
    _load: Option<Task<()>>,
    /// A read of `stat` and `sync` only, after the folder changed on disk.
    _stat: Option<Task<()>>,
    _gh: Option<Task<()>>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Page {
    #[default]
    Main,
    Editors,
    Previous,
    /// The Commit & push menu.
    Git,
    /// "Link pull request…": paste a URL.
    LinkPr,
}

#[derive(Clone, PartialEq)]
struct Editors {
    /// The editor "Open in" runs now (settings, `$VISUAL`, `$EDITOR`, else one on PATH).
    current: Option<String>,
    choices: Vec<String>,
}

/// Folds an MCP report into `list`: a launch report (a new agent process) starts it over, any other updates each server
/// it names. Returns whether `list` changed.
pub(in crate::workspace) fn fold_mcp(
    list: &mut Vec<skyelab_agent::McpServer>,
    record: &super::thread_model::Record,
) -> bool {
    let super::thread_model::Record::Agent(skyelab_agent::Event::Mcp {
        servers, launch, ..
    }) = record
    else {
        return false;
    };
    let before = list.clone();
    if *launch {
        list.clear();
    }
    for s in servers {
        match list.iter_mut().find(|m| m.name == s.name) {
            Some(m) => *m = s.clone(),
            None => list.push(s.clone()),
        }
    }
    *list != before
}

/// What the card says of an MCP server: its last report, or "Stopped" once the agent ended (a failure, or a claude.ai
/// connector that isn't connected, stays).
pub(in crate::workspace) fn mcp_state(s: &skyelab_agent::McpServer, ended: bool) -> &str {
    match s.status.as_str() {
        "failed" | "needs-auth" | "cancelled" | "not-connected" => s.state(),
        _ if ended => "Stopped",
        _ => s.state(),
    }
}

/// A running (or starting) child is listed under Lineage; the rest are previous agents.
fn is_running(status: Status) -> bool {
    matches!(status, Status::Starting | Status::Running)
}

/// What the user calls an editor binary.
pub(super) fn editor_name(bin: &str) -> String {
    let stem = Path::new(bin)
        .file_stem()
        .map_or_else(|| bin.to_owned(), |s| s.to_string_lossy().into_owned());
    match stem.as_str() {
        "code" => "VS Code".into(),
        "cursor" => "Cursor".into(),
        "zed" => "Zed".into(),
        "windsurf" => "Windsurf".into(),
        "subl" => "Sublime Text".into(),
        _ => stem,
    }
}

fn editors() -> Editors {
    let on_path = |n: &str| skyelab_agent::resolve_bin(Path::new(n)).is_ok();
    let setting = skyelab_core::AppSettings::default_path()
        .and_then(|p| skyelab_core::AppSettings::load(&p).ok())
        .and_then(|s| s.editor);
    let current = skyelab_core::editor_argv(setting.as_deref(), |k| std::env::var(k).ok(), on_path)
        .and_then(|argv| argv.into_iter().next());
    let choices = KNOWN_EDITORS
        .into_iter()
        .filter(|n| on_path(n))
        .map(str::to_owned)
        .collect();
    Editors { current, choices }
}

/// Everything the details card reads from its thread view, so the card redraws when one of them changes and not on
/// every notify of the view (each streamed delta, keystroke, tick). The branch picker and script menu notify the card
/// themselves, pull request reads notify it from the workspace, and a timer moves its "3m" labels. A new read in
/// `render_details` goes here too.
#[derive(PartialEq)]
pub(in crate::workspace) struct DetailsSig {
    page: Page,
    machine: Option<SharedString>,
    stat: Option<(u64, u64)>,
    editors: Option<Editors>,
    side_chats: Vec<(i64, SharedString)>,
    remote: Vec<skyelab_store::RemoteChild>,
    mcp: Vec<skyelab_agent::McpServer>,
    gh_login: Option<SharedString>,
    pr_input: Option<gpui::EntityId>,
    pr_error: Option<SharedString>,
    sync: Option<(u32, u32)>,
    fetch_error: Option<SharedString>,
    fetching: bool,
    children: Vec<ChildLink>,
    thread: Option<skyelab_store::Thread>,
    cwd: std::path::PathBuf,
    use_worktree: bool,
    status: Status,
    context: Option<super::thread::context_meter::Meter>,
}

impl ThreadView {
    pub(in crate::workspace) fn details_sig(&self) -> DetailsSig {
        let d = &self.details;
        DetailsSig {
            page: d.page,
            machine: d.machine.clone(),
            stat: d.stat,
            editors: d.editors.clone(),
            side_chats: d.side_chats.clone(),
            remote: d.remote.clone(),
            mcp: d.mcp.clone(),
            gh_login: d.gh_login.clone(),
            pr_input: d.pr_input.as_ref().map(|i| i.entity_id()),
            pr_error: d.pr_error.clone(),
            sync: d.sync,
            fetch_error: d.fetch_error.clone(),
            fetching: d.fetching,
            children: self.children.clone(),
            thread: self.thread.clone(),
            cwd: self.cwd.clone(),
            use_worktree: self.wt.use_worktree,
            status: self.live_status(),
            context: self.context_meter(),
        }
    }

    pub(in crate::workspace) fn live_status(&self) -> crate::ui::Status {
        use crate::ui::Status;
        let running = self.turns.load(std::sync::atomic::Ordering::SeqCst) > 0;
        if self.starting {
            Status::Starting
        } else if self.session.is_some() && running {
            Status::Running // still working, so closing it would cut the work off
        } else if self.limited.is_some() {
            Status::Limited
        } else if self.session.is_some() {
            Status::Idle
        } else {
            match self.thread.as_ref().map(|t| Status::parse(&t.status)) {
                Some(Status::Starting | Status::Running) | None => Status::Stopped,
                Some(other) => other,
            }
        }
    }

    /// Mid-turn or starting: what archiving, the compare cap and the Compare page all count as working.
    pub(in crate::workspace) fn working(&self) -> bool {
        matches!(
            self.live_status(),
            crate::ui::Status::Starting | crate::ui::Status::Running
        )
    }

    /// Back to the card's main page, its pickers closed.
    pub(super) fn details_home(&mut self, cx: &mut Context<Self>) {
        self.details.page = Page::Main;
        self.branch_picker.update(cx, |p, cx| {
            if p.open {
                p.set_open(false, cx)
            }
        });
        self.scripts.update(cx, |m, cx| {
            if m.open {
                m.set_open(false, cx)
            }
        });
        cx.notify();
    }

    /// Reads what the card shows off the UI thread: the diff's size, the editors, this machine's name.
    pub(super) fn load_details(&mut self, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        let base = self.thread.as_ref().and_then(|t| t.worktree_base.clone());
        let (store, id) = (
            self.backend.store.clone(),
            self.thread.as_ref().map(|t| t.id),
        );
        let task = cx.background_spawn(async move {
            let stat = skyelab_git::diff_stat(&cwd, base.as_deref()).ok();
            let sync = skyelab_git::ahead_behind(&cwd).ok().flatten();
            let machine = super::thread::composer::machine_name().map(SharedString::from);
            let remote = id
                .and_then(|id| super::lock(&store).and_then(|s| s.remote_children(id)).ok())
                .unwrap_or_default();
            (stat, sync, editors(), machine, remote)
        });
        // This read covers the stat too: an older watch read must not land after it.
        self.details._stat = None;
        self.details._load = Some(cx.spawn(async move |this, cx| {
            let (stat, sync, editors, machine, remote) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.details.remote = remote;
                this.details.stat = stat;
                this.details.sync = sync;
                this.details.editors = Some(editors);
                this.details.machine = machine;
                cx.notify();
            });
        }));
    }

    /// The folder changed on disk (a folder watch): the footer, the diff size and the upstream counts are read again,
    /// and the view redraws only when one of them differs.
    pub(super) fn folder_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_footer(cx);
        let cwd = self.cwd.clone();
        let base = self.thread.as_ref().and_then(|t| t.worktree_base.clone());
        let task = cx.background_spawn(async move {
            let stat = skyelab_git::diff_stat(&cwd, base.as_deref()).ok();
            (stat, skyelab_git::ahead_behind(&cwd).ok().flatten())
        });
        self.details._stat = Some(cx.spawn(async move |this, cx| {
            let read = task.await;
            let _ = this.update(cx, |this, cx| {
                if (this.details.stat, this.details.sync) != read {
                    (this.details.stat, this.details.sync) = read;
                    cx.notify();
                }
            });
        }));
    }

    fn details_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.details.page = page;
        cx.notify();
    }
}

impl Workspace {
    /// The details card follows the shown thread: its folder's diff size and editors are read again, and every
    /// account's usage too.
    pub(super) fn refresh_details(&mut self, cx: &mut Context<Self>) {
        let view = self.current_thread();
        if let Some(view) = &view {
            view.update(cx, |v, cx| v.load_details(cx));
        }
        self.surfaces
            .update(cx, |s, cx| s.set_thread(view.clone(), cx));
        self.details.update(cx, |d, cx| d.set_thread(view, cx));
        self.refresh_usage(cx);
        self.poll_prs(true, cx);
        self.fetch_shown(cx);
    }

    /// Every account's newest usage readings, read off the UI thread; a newer read replaces an older one.
    pub(super) fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        let (store, accounts) = (self.backend.store.clone(), self.config.accounts.clone());
        let errors = self.usage_poll.errors();
        let task = cx.background_spawn(async move {
            let store = super::lock(&store)?;
            anyhow::Ok(
                accounts
                    .iter()
                    .map(|a| super::details_panel::account_usage(&store, a, &errors))
                    .collect::<Vec<_>>(),
            )
        });
        let details = self.details.downgrade();
        let load = cx.spawn(async move |_, cx| {
            if let Ok(usage) = task.await {
                let _ = details.update(cx, |d, cx| d.set_usage(usage, cx));
            }
        });
        self.details.update(cx, |d, _| d._load = Some(load));
    }

    /// A thread view changed: its parent's Lineage shows its new status.
    pub(super) fn sync_lineage(&mut self, child: &Entity<ThreadView>, cx: &mut Context<Self>) {
        let c = child.read(cx);
        let Some((id, Some(parent))) = c.thread.as_ref().map(|t| (t.id, t.parent_thread_id)) else {
            return;
        };
        let status = c.live_status();
        let Some(parent) = self.open_view(parent, cx) else {
            return;
        };
        parent.update(cx, |p, cx| {
            if let Some(link) = p.children.iter_mut().find(|l| l.id == id)
                && link.status != status
            {
                link.status = status;
                cx.notify();
            }
        });
    }
}
