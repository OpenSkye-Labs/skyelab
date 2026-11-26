//! Settings → Scheduled tasks (T3's scheduled tasks settings): the list, its controls and the editor. It reads and
//! writes the store directly; runs go through the one scheduler (`run_now`).

use super::super::{SharedStore, lock};
use super::Scheduler;
use super::form::{Draft, Kind};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, SharedString, Subscription,
    Window,
};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use skyelab_agent::Mode;
use skyelab_core::AccountsConfig;
use skyelab_store::Project;
use skyelab_store::scheduled::ScheduledTask;
use std::path::PathBuf;

/// A choice from one of the editor's menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Project(i64),
    Kind(Kind),
    Mode(Mode),
    Worktree(bool),
}

/// One listed task: what the store has, plus its project's name and its newest thread.
pub(crate) struct Row {
    pub task: ScheduledTask,
    pub project: SharedString,
    pub thread: Option<(i64, SharedString)>,
}

/// The open editor: its text fields, and the rest of the draft.
pub(crate) struct Editor {
    pub draft: Draft,
    pub title: Entity<InputState>,
    pub prompt: Entity<TextareaState>,
    pub value: Entity<InputState>,
    pub model: Entity<InputState>,
    pub error: Option<SharedString>,
    /// Typing in any field clears `error`: it was about what was there before.
    _edits: Vec<Subscription>,
}

pub(crate) struct ScheduledPage {
    store: SharedStore,
    accounts_path: Option<PathBuf>,
    pub(crate) rows: Vec<Row>,
    pub(crate) projects: Vec<Project>,
    pub(crate) editor: Option<Editor>,
    /// Why the last action failed (a store write, Run now refused).
    pub(crate) error: Option<SharedString>,
    /// The task whose Delete was clicked once: the next click deletes.
    pub(crate) deleting: Option<i64>,
    seen: u64,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl ScheduledPage {
    pub(crate) fn new(
        store: SharedStore,
        accounts_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        // A tick or a run changed a task: read the list again.
        let sub = cx.observe_global::<Scheduler>(|this: &mut Self, cx| {
            let changes = cx.global::<Scheduler>().changes;
            if changes != this.seen {
                this.reload(cx);
            }
        });
        let mut this = Self {
            store,
            accounts_path,
            rows: Vec::new(),
            projects: Vec::new(),
            editor: None,
            error: None,
            deleting: None,
            seen: 0,
            focus: cx.focus_handle(),
            _subs: vec![sub],
        };
        this.reload(cx);
        this
    }

    /// Reads the tasks, their projects and newest threads from the store.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        self.seen = cx.try_global::<Scheduler>().map_or(0, |s| s.changes);
        let read = lock(&self.store).and_then(|s| {
            let (tasks, projects, runs) = (
                s.scheduled_tasks()?,
                s.list_projects()?,
                s.last_scheduled_runs()?,
            );
            let mut rows = Vec::new();
            for task in tasks {
                let project = projects
                    .iter()
                    .find(|p| p.id == task.project_id)
                    .map_or_else(|| "Removed project".into(), |p| p.name.clone().into());
                let thread = match runs.iter().find(|r| r.0 == task.id) {
                    Some(r) => s.thread(r.1)?.map(|t| (t.id, t.title.into())),
                    None => None,
                };
                rows.push(Row {
                    task,
                    project,
                    thread,
                });
            }
            Ok((rows, projects))
        });
        match read {
            Ok((rows, projects)) => (self.rows, self.projects) = (rows, projects),
            Err(e) => self.error = Some(format!("{e:#}").into()),
        }
        cx.notify();
    }

    /// Opens the editor on `draft` (a new task, an edit, or a prompt from the composer).
    pub(crate) fn open_editor(
        &mut self,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input =
            |value: &str, placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
                let (value, placeholder) = (value.to_owned(), placeholder.to_owned());
                cx.new(|cx| {
                    let mut state = InputState::new(window, cx).placeholder(placeholder);
                    state.set_value(value, window, cx);
                    state
                })
            };
        let prompt = draft.prompt.clone();
        let prompt = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("What should the agent do each time this runs?");
            state.set_value(prompt, window, cx);
            state
        });
        let title = input(&draft.title, "e.g. Morning triage", window, cx);
        let value = input(&draft.value, draft.kind.hint(), window, cx);
        let model = input(&draft.model, "Agent default", window, cx);
        let clear = |this: &mut Self, e: &InputEvent, cx: &mut Context<Self>| {
            if matches!(e, InputEvent::Change)
                && let Some(editor) = this.editor.as_mut()
                && editor.error.take().is_some()
            {
                cx.notify();
            }
        };
        let _edits = vec![
            cx.subscribe(&title, move |this, _, e, cx| clear(this, e, cx)),
            cx.subscribe(&prompt, move |this, _, e, cx| clear(this, e, cx)),
            cx.subscribe(&value, move |this, _, e, cx| clear(this, e, cx)),
            cx.subscribe(&model, move |this, _, e, cx| clear(this, e, cx)),
        ];
        self.editor = Some(Editor {
            title,
            prompt,
            value,
            model,
            draft,
            error: None,
            _edits,
        });
        cx.notify();
    }

    /// A new task, prefilled with a prompt and project when it comes from a composer.
    pub(crate) fn new_task(
        &mut self,
        prompt: Option<String>,
        project: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = Draft {
            prompt: prompt.unwrap_or_default(),
            project: project.or_else(|| self.projects.first().map(|p| p.id)),
            ..Draft::default()
        };
        self.open_editor(draft, window, cx);
    }

    pub(crate) fn edit(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.iter().find(|r| r.task.id == id) {
            let draft = Draft::of(&row.task);
            self.open_editor(draft, window, cx);
        }
    }

    pub(crate) fn set_kind(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(e) = &mut self.editor {
            e.draft.kind = kind;
            e.error = None; // set_value below emits no Change
            e.value.update(cx, |v, cx| {
                v.set_value("", window, cx);
                v.set_placeholder(kind.hint(), window, cx);
            });
            cx.notify();
        }
    }

    pub(crate) fn pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            Pick::Kind(kind) => self.set_kind(kind, window, cx),
            Pick::Project(id) => self.update_draft(|d| d.project = Some(id), cx),
            Pick::Mode(mode) => self.update_draft(|d| d.mode = mode, cx),
            Pick::Worktree(on) => self.update_draft(|d| d.worktree = on, cx),
        }
    }

    pub(crate) fn update_draft(&mut self, change: impl FnOnce(&mut Draft), cx: &mut Context<Self>) {
        if let Some(e) = &mut self.editor {
            change(&mut e.draft);
            e.error = None;
            cx.notify();
        }
    }

    /// Saves the editor's task, or shows why it can't be saved.
    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let mut draft = editor.draft.clone();
        draft.title = editor.title.read(cx).value().to_string();
        draft.prompt = editor.prompt.read(cx).value().to_string();
        draft.value = editor.value.read(cx).value().to_string();
        draft.model = editor.model.read(cx).value().to_string();
        // Read now: a save names the account the folder is bound to at this moment.
        let config = self
            .accounts_path
            .as_deref()
            .and_then(|p| AccountsConfig::load(p).ok())
            .unwrap_or_default();
        let now = super::now_ms(cx);
        let saved = draft.to_task(&self.projects, &config, now).and_then(|t| {
            lock(&self.store)
                .and_then(|s| s.save_scheduled_task(&t))
                .map_err(|e| format!("{e:#}"))
        });
        match saved {
            Ok(_) => {
                self.editor = None;
                self.reload(cx);
            }
            Err(why) => {
                editor.draft = draft;
                editor.error = Some(why.into());
                cx.notify();
            }
        }
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    /// Turns a task on (counting its next run on from now) or pauses it.
    pub(crate) fn set_enabled(&mut self, id: i64, on: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|r| r.task.id == id) else {
            return;
        };
        let next = super::next_run(&row.task.schedule, super::now_ms(cx));
        let saved = lock(&self.store).and_then(|s| {
            if on {
                s.set_scheduled_next_run(id, next)?;
            }
            s.set_scheduled_task_enabled(id, on)
        });
        self.finish(saved.map_err(|e| format!("{e:#}")), cx);
    }

    /// Deferred: the run starts on the hub window, which may be the one whose click is still being handled.
    pub(crate) fn run_now(&mut self, id: i64, cx: &mut Context<Self>) {
        let page = cx.entity().downgrade();
        cx.defer(move |cx| {
            let ran = super::run_now(id, cx);
            let _ = page.update(cx, |this, cx| this.finish(ran, cx));
        });
    }

    /// The first click arms Delete; the second deletes.
    pub(crate) fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.deleting != Some(id) {
            self.deleting = Some(id);
            return cx.notify();
        }
        self.deleting = None;
        let deleted = lock(&self.store).and_then(|s| s.delete_scheduled_task(id));
        self.finish(deleted.map_err(|e| format!("{e:#}")), cx);
    }

    fn finish(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        self.error = result.err().map(Into::into);
        self.reload(cx);
    }

    pub(crate) fn mode_label(mode: Mode) -> &'static str {
        match mode {
            Mode::Default => "Ask",
            Mode::Plan => "Plan",
            Mode::AcceptEdits => "Accept edits",
            Mode::Bypass => "Full access",
        }
    }
}

impl Focusable for ScheduledPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
