//! The thread details card, opened from the header's top right (T3's `ThreadDetailsPanel`): this machine and the
//! thread's folder, its editor and scripts, branch and git actions, the agents it delegated, and the bound account's
//! usage. Everything here acts on the thread's own folder under its bound account, so the header stays a breadcrumb.
//!
//! Pickers (branch, scripts, editors, finished agents) open as pages inside the card rather than as popovers of their
//! own, so a click in one never counts as a click outside the card.

use super::Workspace;
use super::thread::ThreadView;
use crate::ui::Status;
use gpui::{Action, AppContext as _, Context, Entity, SharedString, Task, Window, actions};
use std::path::Path;

#[path = "thread_details_card.rs"]
mod card;
#[path = "thread_details_git.rs"]
mod git;

actions!(skyelab, [ToggleThreadDetails]);

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
    pub open: bool,
    page: Page,
    machine: Option<SharedString>,
    /// Lines added and removed in the thread's folder since its base (`None` until loaded, or not a repo).
    stat: Option<(u64, u64)>,
    editors: Option<Editors>,
    /// The bound account's usage windows, as its agent last reported them.
    pub limits: Vec<(&'static str, f32)>,
    _load: Option<Task<()>>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Page {
    #[default]
    Main,
    Editors,
    Previous,
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

/// The host name without its domain (`studio.local` → `studio`).
fn machine_name() -> SharedString {
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    match host.split('.').next() {
        Some(name) if !name.is_empty() => name.to_owned().into(),
        _ => "This machine".into(),
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
        if self.starting {
            Status::Starting
        } else if self.session.is_some() {
            if self.turns.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                Status::Running
            } else {
                Status::Idle
            }
        } else {
            match self.thread.as_ref().map(|t| Status::parse(&t.status)) {
                Some(Status::Starting | Status::Running) | None => Status::Stopped,
                Some(other) => other,
            }
        }
    }

    pub(super) fn toggle_details(
        &mut self,
        _: &ToggleThreadDetails,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_details(!self.details.open, cx);
    }

    pub(super) fn set_details(&mut self, open: bool, cx: &mut Context<Self>) {
        self.details.open = open;
        self.details.page = Page::Main;
        // Its pages close with it.
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
        if open {
            self.load_details(cx);
        }
        cx.notify();
    }

    /// Reads what the card shows off the UI thread: the diff's size, the editors, this machine's name.
    fn load_details(&mut self, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        let base = self.thread.as_ref().and_then(|t| t.worktree_base.clone());
        let task = cx.background_spawn(async move {
            let stat = skyelab_git::diff_stat(&cwd, base.as_deref()).ok();
            (stat, editors(), machine_name())
        });
        self.details._load = Some(cx.spawn(async move |this, cx| {
            let (stat, editors, machine) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.details.stat = stat;
                this.details.editors = Some(editors);
                this.details.machine = Some(machine);
                cx.notify();
            });
        }));
    }

    /// Keeps the newest reading of each usage window the agent reports.
    pub(super) fn note_rate_limit(&mut self, event: &skyelab_agent::Event) {
        let Some((label, used)) = event.rate_limit() else {
            return;
        };
        match self.details.limits.iter_mut().find(|(l, _)| *l == label) {
            Some(slot) => slot.1 = used,
            None => self.details.limits.push((label, used)),
        }
    }

    fn details_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.details.page = page;
        cx.notify();
    }
}

impl Workspace {
    pub(super) fn toggle_thread_details(
        &mut self,
        a: &ToggleThreadDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.current_thread() {
            view.update(cx, |v, cx| v.toggle_details(a, window, cx));
        }
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
