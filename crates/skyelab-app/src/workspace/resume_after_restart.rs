//! Automatically resuming local threads that were left `starting`/`running` when SkyeLab crashed, was killed, or
//! restarted mid-turn. Resumes through `ThreadView::submit_remote` (the exact `ThreadView::send_prompt` /
//! `launch_env(dir)` path), never spawning a separate process path.

use super::thread::ThreadView;
use super::thread_model::Record;
use super::{Workspace, lock};
use gpui::{App, AppContext as _, Context, Global, Window};
use skyelab_agent::Event;
use skyelab_core::AppSettings;
use skyelab_store::Thread;
use std::collections::HashSet;
use std::path::PathBuf;

/// The continuation prompt sent when SkyeLab resumes a thread interrupted by a restart.
pub(crate) const RESTART_RESUME_PROMPT: &str =
    "SkyeLab restarted and resumed this turn. Continue where you left off.";

const RESTART_RESUME_PREFIX: &str = "SkyeLab restarted and resumed this turn";

/// The restart-resume setting and the `settings.toml` it saves to.
struct ResumePrefs {
    path: Option<PathBuf>,
    enabled: bool,
}

impl Global for ResumePrefs {}

/// Loads `resume_after_restart` from `path` (missing or unreadable: on by default).
pub(crate) fn install(path: Option<PathBuf>, cx: &mut App) {
    let enabled = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .is_none_or(|s| s.resume_after_restart);
    cx.set_global(ResumePrefs { path, enabled });
}

/// Whether interrupted turns auto-resume on restart (Settings → General); on with none installed.
pub(crate) fn enabled(cx: &App) -> bool {
    cx.try_global::<ResumePrefs>().is_none_or(|p| p.enabled)
}

/// Updates `resume_after_restart` in memory and saves it to `settings.toml`.
pub(crate) fn set_enabled(on: bool, cx: &mut App) -> anyhow::Result<()> {
    if !cx.has_global::<ResumePrefs>() {
        install(None, cx);
    }
    let prefs = cx.global_mut::<ResumePrefs>();
    prefs.enabled = on;
    match prefs.path.clone() {
        Some(p) => AppSettings::update(&p, |s| s.resume_after_restart = on).map(drop),
        None => Ok(()),
    }
}

/// Whether `text` is a restart-continuation prompt sent by SkyeLab.
pub(crate) fn is_restart_resume_prompt(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed == RESTART_RESUME_PROMPT || trimmed.starts_with(RESTART_RESUME_PREFIX)
}

/// Whether a thread that `Store::open` recovered from `starting`/`running` should be auto-resumed:
/// - local, non-archived, non-side-chat thread with a non-empty `agent_session_id`,
/// - NOT waiting on a pending permission, question (`AskUserQuestion`), or plan (`ExitPlanMode`) approval,
/// - NOT already auto-resumed for this interrupted turn (continuation chain capped at 1).
pub(crate) fn should_resume(thread: &Thread, records: &[Record]) -> bool {
    if thread.archived_at.is_some() || thread.side_of.is_some() {
        return false;
    }
    let has_session = thread
        .agent_session_id
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_session {
        return false;
    }
    let mut pending_approvals = HashSet::<&str>::new();
    let mut last_prompt: Option<&str> = None;
    let mut completed_after_last_prompt = false;
    for record in records {
        match record {
            Record::Prompt(text) => {
                pending_approvals.clear();
                last_prompt = Some(text.as_str());
                completed_after_last_prompt = false;
            }
            Record::Agent(Event::Permission { id, .. }) => {
                pending_approvals.insert(id.as_str());
            }
            Record::Agent(Event::PermissionResolved { id, .. }) => {
                pending_approvals.remove(id.as_str());
            }
            Record::Agent(Event::Result { is_error, .. }) => {
                pending_approvals.clear();
                if !is_error {
                    completed_after_last_prompt = true;
                }
            }
            _ => {}
        }
    }
    if !pending_approvals.is_empty() {
        return false;
    }
    if let Some(prompt) = last_prompt
        && is_restart_resume_prompt(prompt)
        && !completed_after_last_prompt
    {
        return false;
    }
    true
}

impl Workspace {
    /// Checks the store's startup-recovered threads once and resumes eligible ones when `resume_after_restart` is on.
    pub(super) fn resume_recovered_on_start(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = enabled(cx);
        let store = self.backend.store.clone();
        let check = cx.background_spawn(async move {
            let recovered = lock(&store)?.take_recovered();
            if !on || recovered.is_empty() {
                return anyhow::Ok(Vec::new());
            }
            let guard = lock(&store)?;
            let mut eligible = Vec::new();
            for thread in recovered {
                let rows = guard.events_since(thread.id, 0)?;
                let records: Vec<Record> = rows
                    .iter()
                    .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
                    .collect();
                if should_resume(&thread, &records) {
                    eligible.push(thread);
                }
            }
            anyhow::Ok(eligible)
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(eligible) = check.await else {
                return;
            };
            if eligible.is_empty() {
                return;
            }
            let _ = this.update_in(cx, |ws, window, cx| {
                for row in eligible {
                    ws.resume_restarted(row, window, cx);
                }
            });
        })
        .detach();
    }

    fn resume_restarted(&mut self, row: Thread, window: &mut Window, cx: &mut Context<Self>) {
        let found = self
            .open_view(row.id, cx)
            .or_else(|| self.shared_view(row.id, cx));
        let view = found.unwrap_or_else(|| {
            let (config, backend) = (self.config.clone(), self.backend.clone());
            let view = cx.new(|cx| ThreadView::open(row, config, backend, window, cx));
            super::projects::name_project(&self.projects, &view, cx);
            self.add_thread_tab(view.clone(), false, window, cx);
            view
        });
        if !self.open_threads.contains(&view) {
            self.track_thread(&view, window, cx);
        }
        if view.read(cx).read_only {
            return self.ask_when_ready(&view, RESTART_RESUME_PROMPT.to_owned(), window, cx);
        }
        if let Err(why) = view.update(cx, |v, cx| {
            v.submit_remote(RESTART_RESUME_PROMPT, window, cx)
        }) {
            eprintln!("skyelab: couldn't resume interrupted thread: {why}");
        }
    }
}
