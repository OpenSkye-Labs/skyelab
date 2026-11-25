//! What the composer holds and does: attachments, the `@` `/` `$` menus, the model pick, the queue and the footer's
//! facts. How it looks is `composer.rs`; sending is `thread.rs` and `thread_session.rs`.

use super::composer_menu::{self, MenuItem, MenuKind};
use super::composer_queue::Queue;
use super::composer_stash::StashKey;
use super::lock;
use super::thread::ThreadView;
use gpui::{AppContext as _, Context, SharedString, Window, actions};
use skyelab_agent::{Attachment, ModelInfo};
use skyelab_core::{Account, Provider};
use skyelab_store::ModelChoice;
use std::sync::Arc;

#[path = "composer_fit.rs"]
mod composer_fit;
pub(in crate::workspace) use composer_fit::{fits, unfit};

actions!(
    skyelab,
    [
        MenuUp,
        MenuDown,
        MenuAccept,
        MenuClose,
        SteerPrompt,
        AttachFiles
    ]
);

/// An open menu: its rows (already ranked) and the highlighted one.
pub(crate) struct Menu {
    pub kind: MenuKind,
    /// Where the typed token starts (its trigger character), a byte offset in the prompt.
    pub start: usize,
    pub query: String,
    /// Every candidate; `None` while they load.
    pub items: Option<Arc<Vec<MenuItem>>>,
    pub rows: Vec<usize>,
    pub selected: usize,
    /// The query `rows` were ranked for; behind `query` while a ranking runs.
    pub ranked: Option<String>,
    /// Ranks `items` for `query` off the UI thread; a newer query (or closing the menu) drops it.
    pub _rank: Option<gpui::Task<()>>,
}

/// The footer strip's facts, read off the UI thread.
#[derive(Default, PartialEq)]
pub(crate) struct Footer {
    pub machine: Option<SharedString>,
    pub branch: Option<SharedString>,
    /// Changed files (staged, unstaged, untracked); `None` outside a git repo.
    pub changes: Option<usize>,
}

/// How a send was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Submit {
    /// Send (⌘⏎): queues while a turn runs.
    Send,
    /// ⌘⇧⏎: into the running turn.
    Steer,
    /// From another device or an agent: the text alone, sent now.
    External,
}

#[derive(Default)]
pub(crate) struct Composer {
    pub attachments: Vec<Attachment>,
    /// Terminal output added with "Add to prompt", sent after the text.
    pub contexts: Vec<super::composer_context::ContextChip>,
    /// The next excerpt chip's id: never reused, so no chip inherits another's element state.
    pub next_context: u64,
    /// Comments on the Changes tab's diff, until they are sent here as a chip.
    pub review: crate::workspace::diff_comments::DiffComments,
    pub queue: Queue,
    /// The prompt stash's list is open, with this row highlighted (`composer_stash.rs`).
    pub stash_open: Option<usize>,
    /// Text selected in the timeline and where the mouse came up: the floating Quote button (`composer_quote.rs`).
    pub quote: Option<(gpui::Point<gpui::Pixels>, String)>,
    pub menu: Option<Menu>,
    /// The model picker, while open (`composer_picker_keys.rs`).
    pub picker: Option<super::composer_picker_keys::PickerState>,
    pub pick: ModelChoice,
    /// "Send to several models": the ticked models (`compare.rs`).
    pub compare: Option<crate::workspace::compare::Compare>,
    /// Counts model picks, so a pick's save that lands after a newer one's is dropped.
    pub pick_gen: Arc<std::sync::atomic::AtomicU64>,
    /// The user picked in this tab, so a late store read doesn't overwrite it.
    pub picked: bool,
    /// Codex's models, once listed (or why they couldn't be).
    pub codex_models: Option<Result<Vec<ModelInfo>, SharedString>>,
    /// Cursor's models, once listed (or why they couldn't be).
    pub cursor_models: Option<Result<Vec<ModelInfo>, SharedString>>,
    /// Antigravity's models, once listed (or why they couldn't be).
    pub antigravity_models: Option<Result<Vec<ModelInfo>, SharedString>>,
    /// The model this thread's agent last reported running (its init line).
    pub reported_model: Option<String>,
    /// The pick the run that reported it had (`None` inside: the account's default); `None`: replayed, unknown.
    pub reported_pick: Option<Option<String>>,
    /// The account's own default model, from its config dir.
    pub account_model: Option<String>,
    pub footer: Footer,
    /// The footer read in flight: a newer one drops it, so an older read never lands last.
    pub footer_read: Option<gpui::Task<()>>,
    /// Why the last attach or send was refused.
    pub notice: Option<SharedString>,
    /// The docked ask (`ask_dock.rs`) the user folded away, and the placeholder last set for the dock.
    pub dock_collapsed: Option<String>,
    pub dock_placeholder: Option<&'static str>,
    /// The docked question whose answers were sent (a second Submit waits).
    pub dock_sent: Option<String>,
    /// The docked question panel's focus: 1–9 pick while it has it (T3 leaves digits typed in a field alone).
    pub dock_focus: Option<gpui::FocusHandle>,
    /// The docked question the focus last moved to on its arrival, so it moves once per question.
    pub dock_focused: Option<String>,
    /// The last long paste made into a chip (its name and text), for "Paste as text".
    pub pasted: Option<(String, String)>,
    /// An interrupt was sent and the turn hasn't ended yet ("Stopping…"); its number, so a late timer can't clear a newer one.
    pub interrupting: Option<u64>,
    /// The last interrupt went unanswered for [`INTERRUPT_WAIT`]: Force stop shows.
    pub stalled: bool,
    /// A message the agent refused after it left the composer: put back once the composer is empty.
    pub restore: Option<(String, Vec<Attachment>)>,
    /// The draft's saving, prompt history and a fork's source (`composer_draft.rs`).
    pub extras: super::composer_draft::ComposerExtras,
    /// Dictation's state (`composer_voice.rs`) and its running recognizer.
    pub dictation: skyelab_core::dictation::Dictation,
    pub voice: super::composer_voice::VoiceRun,
    /// The card's width at the last paint (0 before the first), for the compact control row.
    pub width: std::rc::Rc<std::cell::Cell<gpui::Pixels>>,
}

/// The text a prompt is stored and shown as: what the agent got, plus the names of its files.
pub(in crate::workspace) fn shown_prompt(text: &str, attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return text.to_owned();
    }
    let names: Vec<&str> = attachments.iter().map(|a| a.name.as_str()).collect();
    let files = format!("Attached: {}", names.join(", "));
    if text.is_empty() {
        files
    } else {
        format!("{text}\n\n{files}")
    }
}

impl ThreadView {
    /// The account the composer acts for: the thread's own, or for a new thread the folder's.
    pub(in crate::workspace) fn composer_account(&self) -> Option<Account> {
        match &self.thread {
            Some(t) => self
                .config
                .accounts
                .iter()
                .find(|a| a.id == t.account_id)
                .cloned(),
            None => self.folder_account.clone(),
        }
    }

    /// Loads the model pick (the thread's; a new thread's as `ThreadSettings::resolve_model` layers it) and the footer's facts.
    pub(in crate::workspace) fn load_composer(&mut self, cx: &mut Context<Self>) {
        let (store, thread, project) = (
            self.backend.store.clone(),
            self.thread.as_ref().map(|t| t.id),
            self.project_id,
        );
        let task = cx.background_spawn(async move {
            let s = lock(&store)?;
            match thread {
                Some(id) => s.thread_model(id),
                None => s.project_model(project),
            }
        });
        cx.spawn(async move |this, cx| {
            if let Ok(mut pick) = task.await {
                let _ = this.update(cx, |this, cx| {
                    // A pick made meanwhile wins; a default from the other provider is dropped.
                    if this.composer.picked {
                        return;
                    }
                    let provider = this.composer_account().map(|a| a.provider);
                    if let (Some(p), Some(m)) = (provider, pick.model.as_deref())
                        && !fits(p, m)
                    {
                        pick = ModelChoice::default();
                    }
                    if provider == Some(Provider::Cursor) {
                        pick.effort = None;
                    }
                    // A new thread: the project's own default, else its remembered pick, else Settings' default.
                    if let (None, Some(p)) = (&this.thread, provider) {
                        let (model, effort) = crate::workspace::thread_extras::prefs::get(cx)
                            .resolve_model(&this.cwd, p, (None, None), (pick.model, pick.effort));
                        pick = ModelChoice { model, effort };
                    }
                    this.composer.pick = pick;
                    cx.notify();
                });
            }
        })
        .detach();
        self.load_account_model(cx);
        self.refresh_footer(cx);
    }

    /// Reads the branch, the changed-file count and the machine name again (after each turn).
    pub(in crate::workspace) fn refresh_footer(&mut self, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        let task = cx.background_spawn(async move {
            let changes = skyelab_git::status(&cwd)
                .ok()
                .map(|s| s.changed_paths().len());
            let branch = skyelab_git::current_branch(&cwd).ok().flatten();
            (super::composer_machine::machine_name(), branch, changes)
        });
        self.composer.footer_read = Some(cx.spawn(async move |this, cx| {
            let (machine, branch, changes) = task.await;
            let _ = this.update(cx, |this, cx| {
                let footer = Footer {
                    machine: machine.map(Into::into),
                    branch: branch.map(Into::into),
                    changes,
                };
                // Most reads (a folder watch firing on an edit that changes no count) find what is shown.
                if this.composer.footer != footer {
                    this.composer.footer = footer;
                    cx.notify();
                }
            });
        }));
    }

    // The `@` `/` `$` `#` menus.

    /// The prompt changed: open, narrow or close the menu its cursor's word asks for.
    pub(in crate::workspace) fn prompt_changed(&mut self, cx: &mut Context<Self>) {
        self.sync_chips(cx);
        let (text, cursor, chips) = {
            let p = self.prompt.read(cx);
            (p.value(), p.cursor(), p.tokens().to_vec())
        };
        cx.notify(); // Send turns into Interrupt and back as the prompt empties and fills
        // A chip is a finished mention: the caret right after it opens no menu.
        let Some(t) = composer_menu::trigger(&text, cursor)
            .filter(|t| !chips.iter().any(|c| c.range().start == t.start))
        else {
            self.composer.menu = None;
            return;
        };
        if let Some(menu) = self
            .composer
            .menu
            .as_mut()
            .filter(|m| m.kind == t.kind && m.start == t.start)
        {
            menu.query = t.query;
            return self.rank_menu(cx);
        }
        self.close_picker(cx);
        self.composer.menu = Some(Menu {
            kind: t.kind,
            start: t.start,
            query: t.query,
            rows: Vec::new(),
            selected: 0,
            items: None,
            ranked: None,
            _rank: None,
        });
        // Read each time a menu opens: the agent adds and removes files between turns.
        self.load_menu_items(t.kind, cx);
        cx.notify();
    }

    pub(in crate::workspace) fn load_menu_items(&mut self, kind: MenuKind, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        let account = self.composer_account();
        let contexts = (kind == MenuKind::Context).then(|| self.context_menu_items());
        if kind == MenuKind::Files {
            self.list_codex_models(cx); // the menu reloads when they land
            self.list_cursor_models(cx);
            self.list_antigravity_models(cx);
        }
        let models = (kind == MenuKind::Files).then(|| self.mention_models());
        let side_commands = !self.is_side_chat();
        let task = cx.background_spawn(async move {
            match (kind, account) {
                // Its own check: only threads of the composer's account (none when unbound).
                (MenuKind::Context, _) => contexts
                    .map_or(Ok(Vec::new()), |read| read())
                    .unwrap_or_default(),
                (MenuKind::Files, _) => composer_menu::at_items(models.unwrap_or_default(), &cwd),
                // Unbound: no account dir to read, so nothing (never the parent's).
                (_, None) => Vec::new(),
                (MenuKind::Commands, Some(a)) => {
                    composer_menu::command_items(a.provider, &a.config_dir, &cwd, side_commands)
                }
                (MenuKind::Skills, Some(a)) => {
                    composer_menu::skill_items(a.provider, &a.config_dir)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let items = Arc::new(task.await);
            let _ = this.update(cx, |this, cx| this.set_menu_items(kind, items, cx));
        })
        .detach();
    }

    fn set_menu_items(
        &mut self,
        kind: MenuKind,
        items: Arc<Vec<MenuItem>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = self.composer.menu.as_mut().filter(|m| m.kind == kind) {
            menu.items = Some(items);
            self.rank_menu(cx);
        }
    }

    /// Ranks the open menu's items for its query on the background executor (a repo can track 100k files).
    fn rank_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.composer.menu.as_mut() else {
            return;
        };
        let Some(items) = menu.items.clone() else {
            return;
        };
        let (kind, start, query) = (menu.kind, menu.start, menu.query.clone());
        let ranked = cx.background_spawn({
            let query = query.clone();
            async move { composer_menu::rank(&items, &query) }
        });
        menu._rank = Some(cx.spawn(async move |this, cx| {
            let rows = ranked.await;
            let _ = this.update(cx, |this, cx| {
                let shown = this.composer.menu.as_mut();
                if let Some(menu) =
                    shown.filter(|m| m.kind == kind && m.start == start && m.query == query)
                {
                    menu.rows = rows;
                    menu.ranked = Some(query);
                    menu.selected = 0;
                    cx.notify();
                }
            });
        }));
    }

    pub(in crate::workspace) fn menu_up(
        &mut self,
        _: &MenuUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.stash_key(StashKey::Move(-1), window, cx) {
            self.move_menu(-1, cx);
        }
    }

    pub(in crate::workspace) fn menu_down(
        &mut self,
        _: &MenuDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.stash_key(StashKey::Move(1), window, cx) {
            self.move_menu(1, cx);
        }
    }

    fn move_menu(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(m) = self.composer.menu.as_mut().filter(|m| !m.rows.is_empty()) {
            let n = m.rows.len() as isize;
            m.selected = (m.selected as isize + by).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    pub(in crate::workspace) fn menu_close(
        &mut self,
        _: &MenuClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stash_key(StashKey::Close, window, cx) {
            return;
        }
        self.composer.menu = None;
        cx.notify();
    }

    pub(in crate::workspace) fn menu_accept(
        &mut self,
        _: &MenuAccept,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stash_key(StashKey::Accept, window, cx) {
            return;
        }
        let selected = self.composer.menu.as_ref().map(|m| m.selected);
        if let Some(ix) = selected {
            self.accept_row(ix, window, cx);
        }
    }

    /// Replaces the typed token with row `ix`'s text.
    pub(in crate::workspace) fn accept_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut menu) = self.composer.menu.take() else {
            return;
        };
        // Accepted before the latest ranking landed: rank now, so the pick matches what was typed.
        if let Some(items) = &menu.items
            && menu.ranked.as_ref() != Some(&menu.query)
        {
            menu.rows = composer_menu::rank(items, &menu.query);
        }
        let Some(item) = menu
            .rows
            .get(ix)
            .and_then(|&i| menu.items.as_ref()?.get(i).cloned())
        else {
            cx.notify();
            return;
        };
        // The caret may have moved since the menu opened: replace only the token it is still in.
        let (text, cursor) = {
            let p = self.prompt.read(cx);
            (p.value(), p.cursor())
        };
        if composer_menu::trigger(&text, cursor).is_none_or(|t| t.start != menu.start) {
            cx.notify();
            return;
        }
        self.prompt.update(cx, |p, cx| {
            p.set_selected_range(menu.start..cursor, cx);
            p.replace(item.insert, window, cx);
        });
        if let Some(pick) = item.context {
            self.add_context_pick(pick, cx);
        }
        self.focus_prompt(window, cx);
        cx.notify();
    }
}
