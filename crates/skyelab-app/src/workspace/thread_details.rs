//! The shown thread's details, as a card stack in the right dock's Details tab (T3's `ThreadDetailsPanel`, laid out
//! like Synara's right panel): this machine and the thread's folder, branch and git actions, scripts, the agents it
//! delegated, every account's usage, its pull request and editor. Everything acts on the thread's own folder under
//! its bound account, so the header stays a breadcrumb.
//!
//! Pickers (branch, scripts, editors, finished agents) open as pages inside the card.

use super::Workspace;
use super::thread::ThreadView;
use crate::ui::Status;
use gpui::{Action, AppContext as _, Context, Entity, SharedString, Task};
use std::path::Path;

#[path = "thread_details_card.rs"]
mod card;
#[path = "thread_details_git.rs"]
mod git;
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
    stat: Option<(u64, u64)>,
    editors: Option<Editors>,
    /// Who the Commit & push menu pushes as: the account's GitHub login, or why there is none.
    pub(in crate::workspace) gh_login: Option<SharedString>,
    /// The "Link pull request…" field and what was wrong with the last link.
    pr_input: Option<Entity<gpui_component::input::InputState>>,
    pr_error: Option<SharedString>,
    _load: Option<Task<()>>,
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

struct Editors {
    /// The editor "Open in" runs now (settings, `$VISUAL`, `$EDITOR`, else one on PATH).
    current: Option<String>,
    choices: Vec<String>,
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

impl ThreadView {
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
        let task = cx.background_spawn(async move {
            let stat = skyelab_git::diff_stat(&cwd, base.as_deref()).ok();
            let machine = super::thread::composer::machine_name().map(SharedString::from);
            (stat, editors(), machine)
        });
        self.details._load = Some(cx.spawn(async move |this, cx| {
            let (stat, editors, machine) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.details.stat = stat;
                this.details.editors = Some(editors);
                this.details.machine = machine;
                cx.notify();
            });
        }));
    }

    fn details_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.details.page = page;
        cx.notify();
    }
}

impl Workspace {
    /// The Details tab follows the shown thread: its folder's diff size and editors are read again, and every
    /// account's usage too.
    pub(super) fn refresh_details(&mut self, cx: &mut Context<Self>) {
        let view = self.current_thread();
        if let Some(view) = &view {
            view.update(cx, |v, cx| v.load_details(cx));
        }
        self.details.update(cx, |d, cx| d.set_thread(view, cx));
        self.refresh_usage(cx);
    }

    /// Every account's newest usage readings, read off the UI thread; a newer read replaces an older one.
    pub(super) fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        let (store, accounts) = (self.backend.store.clone(), self.config.accounts.clone());
        let task = cx.background_spawn(async move {
            let store = super::lock(&store)?;
            anyhow::Ok(
                accounts
                    .iter()
                    .map(|a| super::details_panel::account_usage(&store, a))
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
