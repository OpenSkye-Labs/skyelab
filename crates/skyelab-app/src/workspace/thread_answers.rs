//! Answering the agent's questions (AskUserQuestion) and its proposed plan (ExitPlanMode), and opening the files
//! its cards name.

use super::{ThreadView, ThreadViewEvent, lock};
use crate::workspace::ask_cards::{OTHER, QuestionDraft, questions};
use crate::workspace::thread_items::ItemsView;
use gpui::{App, AppContext as _, Context, Entity, Window};
use gpui_component::input::InputState;
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
        let Some(session) = self.session.clone() else {
            return Err("the agent isn't running".into());
        };
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

    fn draft_mut(&mut self, id: &str) -> &mut QuestionDraft {
        let n = self
            .items
            .iter()
            .find_map(|i| match i {
                super::Item::Approval(a) if a.id == id => Some(questions(&a.raw).len()),
                _ => None,
            })
            .unwrap_or_default();
        let d = self.drafts.entry(id.to_owned()).or_default();
        d.picked
            .resize_with(n.max(d.picked.len()), Default::default);
        d.other.resize_with(n.max(d.other.len()), Default::default);
        d
    }

    /// Toggles a pick. A single-choice question keeps one pick; "Other" is a pick too.
    pub fn pick(&mut self, id: &str, q: usize, option: usize, multi: bool, cx: &mut Context<Self>) {
        let d = self.draft_mut(id);
        let Some(set) = d.picked.get_mut(q) else {
            return;
        };
        if !set.remove(&option) {
            if !multi {
                set.clear();
            }
            set.insert(option);
        }
        self.redraw(id, cx);
    }

    /// Question `q`'s "Other" text field, made the first time it shows.
    pub fn other_input(
        &mut self,
        id: &str,
        q: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let existing = self.draft_mut(id).other.get(q).cloned().flatten();
        if let Some(input) = existing {
            return input;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Your answer"));
        if let Some(slot) = self.draft_mut(id).other.get_mut(q) {
            *slot = Some(input.clone());
        }
        input
    }

    /// Each question's picks as labels (a typed "Other" answer is its text), or why they can't be sent yet.
    pub(super) fn answers(&self, id: &str, cx: &gpui::App) -> Result<Vec<Vec<String>>, String> {
        let qs = self
            .items
            .iter()
            .find_map(|i| match i {
                super::Item::Approval(a) if a.id == id => Some(questions(&a.raw)),
                _ => None,
            })
            .ok_or("that question is gone")?;
        let draft = self.drafts.get(id);
        qs.iter()
            .enumerate()
            .map(|(qi, q)| {
                let picked = draft.and_then(|d| d.picked.get(qi));
                let mut labels: Vec<String> = picked
                    .into_iter()
                    .flatten()
                    .filter_map(|&o| q.options.get(o).map(|(l, _)| l.clone()))
                    .collect();
                if picked.is_some_and(|p| p.contains(&OTHER)) {
                    let typed = draft
                        .and_then(|d| d.other.get(qi).cloned().flatten())
                        .map(|i| i.read(cx).value().trim().to_owned())
                        .unwrap_or_default();
                    if typed.is_empty() {
                        return Err(format!("Type your answer to \"{}\".", q.question));
                    }
                    labels.push(typed);
                }
                if labels.is_empty() {
                    return Err(format!("Pick an answer to \"{}\".", q.question));
                }
                Ok(labels)
            })
            .collect()
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
        let Some(session) = self.session.clone() else {
            return self.show_error(anyhow::anyhow!("the agent isn't running"), cx);
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
        if !approve {
            return self.answer(id, Decision::Deny, cx);
        }
        let Some(session) = self.session.clone() else {
            return self.show_error(anyhow::anyhow!("the agent isn't running"), cx);
        };
        let id = id.to_owned();
        let sent =
            cx.background_spawn(async move { lock(&session)?.answer(&id, Decision::AllowOnce) });
        cx.spawn(async move |this, cx| {
            let sent = sent.await;
            let _ = this.update(cx, |this, cx| match sent {
                Err(e) => this.show_error(e, cx),
                // Only once the agent has the approval, so it reads that before the mode change. Whether `-p` claude
                // leaves plan mode by itself is unconfirmed, so the thread's mode moves explicitly.
                Ok(()) if this.mode == Mode::Plan => this.set_mode(Mode::Default, cx),
                Ok(()) => {}
            });
        })
        .detach();
    }

    fn redraw(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .items
            .iter()
            .position(|i| matches!(i, super::Item::Approval(a) if a.id == id))
        {
            self.list.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }
}

impl ItemsView for ThreadView {
    fn items(&self) -> &[super::Item] {
        &self.items
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
    fn interrupt_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.interrupt(&super::Interrupt, window, cx)
    }
    fn open_path(&mut self, path: &str, cx: &mut Context<Self>) {
        ThreadView::open_path(self, path, cx)
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
    fn find_mark(&self, ix: usize) -> Option<bool> {
        self.timeline.find.mark(ix)
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
    ) -> Option<gpui::Entity<gpui_component::input::InputState>> {
        Some(ThreadView::other_input(self, id, q, window, cx))
    }
    fn submit_answers(&mut self, id: &str, _: &mut Window, cx: &mut Context<Self>) {
        ThreadView::submit_answers(self, id, cx)
    }
    fn answer_plan(&mut self, id: &str, approve: bool, _: &mut Window, cx: &mut Context<Self>) {
        ThreadView::answer_plan(self, id, approve, cx)
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

    /// The provider of the account the thread was started with (`None` before the thread has a row,
    /// or when that account is gone).
    #[cfg(all(test, unix))]
    pub fn provider(&self) -> Option<skyelab_core::Provider> {
        let id = &self.thread.as_ref()?.account_id;
        let account = self.config.accounts.iter().find(|a| &a.id == id)?;
        Some(account.provider)
    }
}
