//! The composer's draft, kept in the store per thread (and per project for a "New thread" tab), so it survives switching
//! threads and restarts; prompt history (ArrowUp/ArrowDown walk this thread's earlier prompts); and what a new thread
//! takes from Settings → Threads.

use crate::workspace::thread::ThreadView;
use crate::workspace::thread_model::Item;
use crate::workspace::{SharedStore, lock};
use gpui::{
    App, AppContext as _, Context, Focusable as _, KeyBinding, SharedString, Window, actions,
};
use skyelab_agent::{Attachment, Mode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

actions!(skyelab, [HistoryUp, HistoryDown]);

/// Up and Down in the prompt. When the history doesn't take the key, it goes on to the menu or the text field.
pub(in crate::workspace) fn bind_keys(cx: &mut App) {
    let input = Some("Thread > Input");
    cx.bind_keys([
        KeyBinding::new("up", HistoryUp, input),
        KeyBinding::new("down", HistoryDown, input),
    ]);
}

type Files = Vec<(String, Vec<u8>)>;

/// Draft writes, latest first: each write stores the newest pending text (and files) of every owner, so writes that
/// finish out of order can't leave an older draft behind, and a new thread's switch from its project's draft to its
/// own can't drop the project's pending clear.
#[derive(Default, Clone)]
struct Saver {
    text: Arc<Mutex<HashMap<String, String>>>,
    files: Arc<Mutex<HashMap<String, Files>>>,
    write: Arc<Mutex<()>>,
}

impl Saver {
    fn flush(&self, store: &SharedStore) -> anyhow::Result<()> {
        let _held = lock(&self.write)?;
        let text = std::mem::take(&mut *lock(&self.text)?);
        for (owner, text) in text {
            lock(store)?.set_draft_text(&owner, &text)?;
        }
        let files = std::mem::take(&mut *lock(&self.files)?);
        for (owner, files) in files {
            lock(store)?.set_draft_files(&owner, &files)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct ComposerExtras {
    saver: Saver,
    /// The text and files (name, size) as last saved, to notice a change.
    saved_text: String,
    saved_files: Vec<(String, usize)>,
    /// The history entry shown: its index among this thread's prompts, and its text.
    history: Option<(usize, String)>,
    /// The thread this one was forked or handed off from: its id, and the link's label ("Forked from …" or
    /// "Continued from …").
    pub forked_from: Option<(i64, SharedString)>,
    /// A draft started from the Pull requests page: the PR its row links once the first message saves it.
    pub pending_pr: Option<String>,
}

/// A shown prompt as the composer can hold it again: without its "Attached: …" line (`None` for files alone).
fn recallable(shown: &str) -> Option<&str> {
    if shown.starts_with("Attached: ") {
        return None;
    }
    let text = shown.split("\n\nAttached: ").next().unwrap_or(shown).trim();
    (!text.is_empty()).then_some(text)
}

impl ThreadView {
    fn draft_owner(&self) -> String {
        match &self.thread {
            Some(t) => format!("thread:{}", t.id),
            None => format!("project:{}", self.project_id),
        }
    }

    /// Set up once the view exists: a new thread's defaults, the stored draft, the files' saver and a fork's source.
    pub(in crate::workspace) fn init_extras(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.thread.is_none() {
            // A new thread's cwd is its project's folder, the key of that project's own defaults.
            let prefs = crate::workspace::thread_extras::prefs::get(cx).for_project(&self.cwd);
            // Full access is never a default: it is confirmed per thread.
            self.mode = match Mode::parse(prefs.permission_mode.as_deref().unwrap_or_default()) {
                Mode::Bypass => Mode::Default,
                m => m,
            };
            self.wt.use_worktree = prefs.new_worktree;
        }
        // The text and files change in many places (typing, sending, a refused send put back): one check after any
        // change saves them.
        self._subs
            .push(cx.observe_self(|this, cx| this.save_draft(cx)));
        self.init_stash(cx);
        let (store, owner) = (self.backend.store.clone(), self.draft_owner());
        let source = self
            .thread
            .as_ref()
            .and_then(|t| Some((t.forked_from?, t.account_id.clone())));
        let task = cx.background_spawn(async move {
            let s = lock(&store)?;
            let title = match source {
                Some((id, account)) => s.thread(id)?.map(|t| {
                    // A fork keeps its source's account; a handoff runs under the other provider's.
                    let handoff = t.account_id != account;
                    (
                        id,
                        crate::workspace::thread_extras::fork::forked_label(&t.title, handoff),
                    )
                }),
                None => None,
            };
            anyhow::Ok((s.draft(&owner)?, title))
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok((draft, source)) = task.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.composer.extras.forked_from = source;
                // Typed meanwhile: what the user has now wins.
                let empty =
                    this.prompt.read(cx).value().is_empty() && this.composer.attachments.is_empty();
                if !empty || draft.is_empty() {
                    return cx.notify();
                }
                // Read back through the same checks as an attached file.
                this.composer.attachments = draft
                    .files
                    .into_iter()
                    .filter_map(|(name, data)| Attachment::new(name, data).ok())
                    .collect();
                this.composer.extras.saved_files = this.file_marks();
                this.composer.extras.saved_text = draft.text.clone();
                this.prompt
                    .update(cx, |p, cx| p.set_value(draft.text, window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    fn file_marks(&self) -> Vec<(String, usize)> {
        let files = &self.composer.attachments;
        files
            .iter()
            .map(|a| (a.name.clone(), a.data.len()))
            .collect()
    }

    /// Stores what changed since the last save (sending empties the composer, which removes the draft).
    fn save_draft(&mut self, cx: &mut Context<Self>) {
        // A review draft from the Pull requests page isn't the project's new-thread draft: saving it would replace that.
        if self.thread.is_none() && self.composer.extras.pending_pr.is_some() {
            return;
        }
        let text = self.prompt.read(cx).value();
        if *text != *self.composer.extras.saved_text {
            self.composer.extras.saved_text = text.to_string();
            let pending = &self.composer.extras.saver.text;
            let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.insert(self.draft_owner(), text.to_string());
            drop(pending);
            self.write_draft(cx);
        }
        let marks = self.file_marks();
        if marks == self.composer.extras.saved_files {
            return;
        }
        self.composer.extras.saved_files = marks;
        let files = self.composer.attachments.iter();
        let files = files.map(|a| (a.name.clone(), a.data.clone())).collect();
        let pending = &self.composer.extras.saver.files;
        let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.insert(self.draft_owner(), files);
        drop(pending);
        self.write_draft(cx);
    }

    fn write_draft(&mut self, cx: &mut Context<Self>) {
        let (saver, store) = (
            self.composer.extras.saver.clone(),
            self.backend.store.clone(),
        );
        self.in_background(cx, move || saver.flush(&store));
    }

    /// Writes what is still pending (the app is quitting).
    pub(in crate::workspace) fn draft_flush(&self) -> impl FnOnce() + Send + use<> {
        let (saver, store) = (
            self.composer.extras.saver.clone(),
            self.backend.store.clone(),
        );
        move || drop(saver.flush(&store))
    }

    /// This thread's earlier prompts, oldest first, repeats folded.
    fn history_entries(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for item in &self.items {
            if let Item::Prompt(t) | Item::Steer(t) = item
                && let Some(t) = recallable(t)
                && out.last().is_none_or(|l| l != t)
            {
                out.push(t.to_owned());
            }
        }
        out
    }

    /// The history entry the prompt shows, if it still shows it unedited.
    fn shown_entry(&self, cx: &App) -> Option<usize> {
        let now = self.prompt.read(cx).value();
        let (i, text) = self.composer.extras.history.as_ref()?;
        (*text == *now).then_some(*i)
    }

    fn show_entry(
        &mut self,
        entry: Option<(usize, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = entry.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
        self.composer.extras.history = entry;
        self.prompt
            .update(cx, |p, cx| p.set_value(text, window, cx));
        cx.notify();
    }

    /// Whether the key is ours: the prompt has focus and no menu (nor the stash list) is open.
    fn history_key(&self, window: &Window, cx: &App) -> bool {
        self.composer.menu.is_none()
            && self.composer.stash_open.is_none()
            && self.prompt.read(cx).focus_handle(cx).is_focused(window)
    }

    /// ArrowUp: in an empty prompt, the newest earlier prompt; on a shown one's first line, the one before it.
    pub(in crate::workspace) fn history_up(
        &mut self,
        _: &HistoryUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, cursor) = {
            let p = self.prompt.read(cx);
            (p.value(), p.cursor())
        };
        let entries = self.history_entries();
        let target = match self.shown_entry(cx) {
            Some(i) if !text.get(..cursor).unwrap_or(&text).contains('\n') => i.checked_sub(1),
            None if text.is_empty() => entries.len().checked_sub(1),
            _ => None,
        };
        match target.filter(|_| self.history_key(window, cx)) {
            Some(i) => self.show_entry(Some((i, entries[i].clone())), window, cx),
            None => cx.propagate(),
        }
    }

    /// ArrowDown on a shown prompt's last line: the next newer one, then back to an empty prompt.
    pub(in crate::workspace) fn history_down(
        &mut self,
        _: &HistoryDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, cursor) = {
            let p = self.prompt.read(cx);
            (p.value(), p.cursor())
        };
        let on_last_line = !text.get(cursor..).unwrap_or_default().contains('\n');
        match self
            .shown_entry(cx)
            .filter(|_| on_last_line && self.history_key(window, cx))
        {
            Some(i) => {
                let next = self
                    .history_entries()
                    .get(i + 1)
                    .cloned()
                    .map(|t| (i + 1, t));
                self.show_entry(next, window, cx)
            }
            None => cx.propagate(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn recallable_drops_the_files_line() {
        assert_eq!(
            super::recallable("fix it\n\nAttached: a.png"),
            Some("fix it")
        );
        assert_eq!(super::recallable("Attached: a.png"), None);
        assert_eq!(super::recallable("plain"), Some("plain"));
    }
}
