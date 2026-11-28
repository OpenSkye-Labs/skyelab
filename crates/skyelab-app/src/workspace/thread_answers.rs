//! Answering the agent's questions (AskUserQuestion) and its proposed plan (ExitPlanMode), and opening the files
//! its cards name.

use super::{ThreadView, ThreadViewEvent, lock};
use crate::ui::TextInput;
use crate::workspace::ask_cards::QuestionDraft;
use crate::workspace::question_drafts;
use crate::workspace::thread_items::ItemsView;
use gpui::{App, AppContext as _, Context, Entity, Window};
use skyelab_agent::{Decision, Mode};

impl ThreadView {
    /// Answers the agent's permission request `id` (a click on its row).
    pub fn answer(&mut self, id: &str, decision: Decision, cx: &mut Context<Self>) {
        if let Err(e) = self.answer_then(id, decision, |_| {}, cx) {
            self.show_error(anyhow::anyhow!(e), cx);
        }
    }

    /// Sends the answer off the UI thread; `done` gets whether the agent took it (it can't once the request was
    /// withdrawn). Refused at once when no agent runs.
    pub(in crate::workspace) fn answer_then(
        &mut self,
        id: &str,
        decision: Decision,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        // A side chat's agent may only be told no (Codex's Plan sandbox lets it ask to run outside).
        if self.is_side_chat() && !matches!(decision, Decision::Deny | Decision::Withdrawn) {
            return Err(crate::workspace::side_chat::PLAN_ONLY.into());
        }
        // A no can't set the agent working, so only an allow re-checks the folder's binding.
        let session = if matches!(decision, Decision::Deny | Decision::Withdrawn) {
            self.session.clone().ok_or("the agent isn't running")?
        } else {
            self.answer_session(cx)?
        };
        self.error = None; // a refusal left about this ask (no rule can cover it) is settled with it
        let id = id.to_owned();
        self.in_background(cx, move || {
            let r = lock(&session).and_then(|mut s| s.answer(&id, decision));
            done(r.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
            r
        });
        Ok(())
    }

    /// A file a card or link names, relative to the thread's folder unless absolute: shown in the Changes tab.
    pub fn open_path(&mut self, path: &str, cx: &mut Context<Self>) {
        let path = std::path::Path::new(path);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.cwd.join(path)
        };
        cx.emit(ThreadViewEvent::OpenPath(path));
    }

    /// Toggles a pick. A single-choice question keeps one pick; "Other" is a pick too.
    pub fn pick(&mut self, id: &str, q: usize, option: usize, multi: bool, cx: &mut Context<Self>) {
        question_drafts::pick(&self.items, &mut self.drafts, id, q, option, multi);
        self.redraw(id, cx);
    }

    /// Picks files for question `q`'s answer with the system file dialog.
    pub fn attach_answer_files(&mut self, id: &str, q: usize, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return; // cancelled, or the platform has no picker
            };
            let _ = this.update(cx, |this, cx| this.attach_answer_paths(&id, q, paths, cx));
        })
        .detach();
    }

    /// Adds `paths` to question `q`'s answer.
    pub(in crate::workspace) fn attach_answer_paths(
        &mut self,
        id: &str,
        q: usize,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        question_drafts::attach(&self.items, &mut self.drafts, id, q, paths);
        self.redraw(id, cx);
    }

    /// Question `q`'s "Other" text field, made the first time it shows.
    pub fn other_input(
        &mut self,
        id: &str,
        q: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        question_drafts::other_input(&self.items, &mut self.drafts, id, q, window, cx)
    }

    /// Each question's picks as labels (a typed "Other" answer is its text), or why they can't be sent yet.
    pub(super) fn answers(&self, id: &str, cx: &gpui::App) -> Result<Vec<Vec<String>>, String> {
        question_drafts::answers(&self.items, &self.drafts, id, cx)
    }

    /// Sends every question's answer to the agent, off the UI thread.
    pub fn submit_answers(&mut self, id: &str, cx: &mut Context<Self>) {
        let answers = match self.answers(id, cx) {
            Ok(a) => a,
            Err(why) => {
                self.error = Some(why.into());
                return cx.notify();
            }
        };
        let session = match self.answer_session(cx) {
            Ok(s) => s,
            Err(why) => return self.show_error(anyhow::anyhow!(why), cx),
        };
        self.error = None;
        let id = id.to_owned();
        let sent = {
            let id = id.clone();
            cx.background_spawn(async move { lock(&session)?.answer_questions(&id, &answers) })
        };
        // The picks stay until the agent has them, so a refused send can be tried again.
        cx.spawn(async move |this, cx| {
            let sent = sent.await;
            let _ = this.update(cx, |this, cx| match sent {
                Ok(()) => {
                    this.drafts.remove(&id);
                }
                Err(e) => this.show_error(e, cx),
            });
        })
        .detach();
    }

    /// Approve lets the agent leave plan mode and start (the thread goes to Ask, so edits ask first); Keep planning
    /// tells it no.
    pub fn answer_plan(&mut self, id: &str, approve: bool, cx: &mut Context<Self>) {
        if let Err(e) = self.answer_plan_then(id, approve, |_| {}, cx) {
            self.show_error(anyhow::anyhow!(e), cx);
        }
    }

    /// [`Self::answer_plan`], from here or another device; `done` gets whether the agent took the answer.
    pub(in crate::workspace) fn answer_plan_then(
        &mut self,
        id: &str,
        approve: bool,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !approve {
            return self.answer_then(id, Decision::Deny, done, cx);
        }
        if self.is_side_chat() {
            return Err(crate::workspace::side_chat::PLAN_ONLY.into());
        }
        let session = self.answer_session(cx)?;
        let id = id.to_owned();
        let sent =
            cx.background_spawn(async move { lock(&session)?.answer(&id, Decision::AllowOnce) });
        cx.spawn(async move |this, cx| {
            let sent = sent.await;
            done(sent.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
            let _ = this.update(cx, |this, cx| match sent {
                Err(e) => this.show_error(e, cx),
                // Only once the agent has the approval, so it reads that before the mode change. Whether `-p` claude
                // leaves plan mode by itself is unconfirmed, so the thread's mode moves explicitly.
                Ok(()) if this.mode == Mode::Plan => this.set_mode(Mode::Default, cx),
                Ok(()) => {}
            });
        })
        .detach();
        Ok(())
    }

    /// Whether `id` is an ExitPlanMode request: answering it allows or denies the plan, and an approval leaves Plan.
    pub(in crate::workspace) fn is_plan(&self, id: &str) -> bool {
        self.items.iter().any(
            |i| matches!(i, super::Item::Approval(a) if a.id == id && a.tool == skyelab_agent::PLAN_TOOL),
        )
    }

    /// Another device's answers to AskUserQuestion `id` (one list of labels per question), sent as a local Submit
    /// sends them; `done` gets whether the agent took them (it refuses a count that doesn't match).
    pub(in crate::workspace) fn answer_questions_then(
        &mut self,
        id: &str,
        answers: Vec<Vec<String>>,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let session = self.answer_session(cx)?;
        let id = id.to_owned();
        self.in_background(cx, move || {
            let r = lock(&session).and_then(|mut s| s.answer_questions(&id, &answers));
            done(r.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
            r
        });
        Ok(())
    }

    /// The running agent an answer may go to: an answer lets it carry on, so it is a send like any other, and a folder
    /// rebound (or unbound) since it started ends it instead (`end_refused`).
    pub(in crate::workspace) fn answer_session(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<std::sync::Arc<std::sync::Mutex<skyelab_agent::Session>>, String> {
        let session = self.session.clone().ok_or("the agent isn't running")?;
        if let Err(why) = self.send_account() {
            self.end_refused(why.clone(), cx);
            return Err(why.to_string());
        }
        Ok(session)
    }

    fn redraw(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .items
            .iter()
            .position(|i| matches!(i, super::Item::Approval(a) if a.id == id))
        {
            self.lists.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }
}

impl ItemsView for ThreadView {
    fn items(&self) -> &super::super::thread_model::Items {
        &self.items
    }
    fn sent_images(&self, text: &str) -> Option<&crate::workspace::image_viewer::Sent> {
        self.sent_images.get(text)
    }
    fn fork(&self, ix: usize) -> Option<crate::workspace::thread_extras::fork::ForkThread> {
        let id = self.thread.as_ref()?.id;
        let done = crate::workspace::timeline_fold::reply_of(&self.items, ix)?;
        let turns = self.items[..=done]
            .iter()
            .filter(|i| matches!(i, super::Item::Done { .. }))
            .count();
        Some(crate::workspace::thread_extras::fork::ForkThread { id, turns })
    }
    fn is_expanded(&self, tool_id: &str) -> bool {
        ThreadView::is_expanded(self, tool_id)
    }
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        ThreadView::toggle_tool(self, tool_id, cx)
    }
    fn can_answer(&self, _: &App) -> bool {
        self.session.is_some()
    }
    fn answer(&mut self, id: &str, decision: Decision, _: &mut Window, cx: &mut Context<Self>) {
        ThreadView::answer(self, id, decision, cx)
    }
    fn offers_project_rule(&self, tool: &str) -> bool {
        self.takes_project_rules() && skyelab_core::project_rules::offers(tool)
    }
    fn allow_for_project(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_rule_dialog(id, window, cx)
    }
    fn thread_prefix(&self, a: &crate::workspace::thread_model::ApprovalItem) -> Option<String> {
        use skyelab_core::project_rules as rules;
        // Codex's shell only: Claude's thread allow is the exact command.
        let local = self.session.is_some() && !self.is_side_chat() && self.mode != Mode::Plan;
        (local && a.tool == "shell" && rules::propose(&a.tool, &a.raw, &self.cwd).is_some())
            .then(|| a.raw["command"].as_str().and_then(rules::suggested_prefix))
            .flatten()
            // One word (`git`) or a script the agent can rewrite (`./build.sh`) allows far more than it looks.
            .filter(|p| p.split_whitespace().count() == 2 && !p.contains('/'))
    }
    fn allow_thread_prefix(
        &mut self,
        id: &str,
        prefix: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = match self.answer_session(cx) {
            Ok(s) => s,
            Err(why) => return self.show_error(anyhow::anyhow!(why), cx),
        };
        let (id, prefix) = (id.to_owned(), prefix.to_owned());
        self.in_background(cx, move || {
            lock(&session)?.allow_thread_prefix(&id, &prefix)
        });
    }
    fn interrupt_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.interrupt(&super::Interrupt, window, cx)
    }
    fn open_path(&mut self, path: &str, cx: &mut Context<Self>) {
        ThreadView::open_path(self, path, cx)
    }
    fn files_root(&self) -> Option<&std::path::Path> {
        Some(&self.cwd)
    }
    fn preview_path(&mut self, path: &str, line: Option<u32>, cx: &mut Context<Self>) {
        cx.emit(ThreadViewEvent::Preview(path.into(), line));
    }
    fn turn_files(&self, done: usize) -> Option<(u32, &[super::super::turn_files::FileStat])> {
        let f = &self.timeline.files;
        let turn =
            super::super::turn_files::turn_of(&self.items, done, f.turns, f.dividers_at_load)?;
        Some((turn, f.by_turn.get(&turn)?))
    }
    fn open_turn_file(&mut self, turn: u32, path: &std::path::Path, cx: &mut Context<Self>) {
        cx.emit(ThreadViewEvent::OpenTurnPath(turn, path.to_owned()));
    }
    fn find_words(&self, ix: usize) -> Option<(&str, Option<usize>)> {
        self.timeline.find.words(ix)
    }
    fn tool_live(&self, id: &str) -> bool {
        // Only the running turn's calls can still finish: one in an earlier turn, or with no turn running, was cut off.
        self.turn_running()
            && self
                .items
                .iter()
                .rev()
                .take_while(|i| !matches!(i, crate::workspace::thread_model::Item::Prompt(_)))
                .any(|i| matches!(i, crate::workspace::thread_model::Item::Tool(t) if t.id == id))
    }
    fn agent_alive(&self) -> bool {
        self.session.is_some()
    }
    fn docks_asks(&self, id: &str) -> bool {
        self.dock().is_some_and(|a| a.id == id)
    }
    fn can_answer_questions(&self, _: &App) -> bool {
        self.session.is_some()
    }
    fn draft(&self, id: &str) -> Option<&QuestionDraft> {
        self.drafts.get(id)
    }
    fn pick(&mut self, id: &str, q: usize, option: usize, multi: bool, cx: &mut Context<Self>) {
        ThreadView::pick(self, id, q, option, multi, cx)
    }
    fn other_input(
        &mut self,
        id: &str,
        q: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<crate::ui::TextInput>> {
        Some(ThreadView::other_input(self, id, q, window, cx))
    }
    fn can_attach_answer_files(&self, cx: &App) -> bool {
        self.can_answer_questions(cx)
    }
    fn attach_answer_files(&mut self, id: &str, q: usize, cx: &mut Context<Self>) {
        ThreadView::attach_answer_files(self, id, q, cx)
    }
    fn remove_answer_file(&mut self, id: &str, q: usize, ix: usize, cx: &mut Context<Self>) {
        question_drafts::detach(&mut self.drafts, id, q, ix);
        self.redraw(id, cx);
    }
    fn submit_answers(&mut self, id: &str, _: &mut Window, cx: &mut Context<Self>) {
        ThreadView::submit_answers(self, id, cx)
    }
    fn answer_plan(
        &mut self,
        id: &str,
        approve: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review_plan(id).is_some() {
            return match self.answer_review_plan(id, approve, window, cx) {
                Err(why) => self.show_error(anyhow::anyhow!(why), cx),
                Ok(()) if !approve => self.focus_prompt(window, cx), // the composer is theirs again
                Ok(()) => {}
            };
        }
        ThreadView::answer_plan(self, id, approve, cx)
    }
    fn reveal_plan(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        ThreadView::reveal_plan(self, path, cx)
    }
    fn message_menu(
        &mut self,
        is_markdown: bool,
        text: String,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_message_menu(is_markdown, text, at, window, cx);
    }
}

impl ThreadView {
    /// "Label · id" of the account the thread was started with, from its store row.
    #[cfg(all(test, unix))] // only the unix-only thread tests read it
    pub fn badge(&self) -> Option<gpui::SharedString> {
        let id = &self.thread.as_ref()?.account_id;
        let label = self
            .config
            .accounts
            .iter()
            .find(|a| &a.id == id)
            .map_or("Unknown account", |a| a.label.as_str());
        Some(format!("{label} · {id}").into())
    }
}
