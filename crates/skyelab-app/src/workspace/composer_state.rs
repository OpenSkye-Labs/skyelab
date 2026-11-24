//! What the composer holds and does: attachments, the `@` `/` `$` menus, the model pick, the queue and the footer's
//! facts. How it looks is `composer.rs`; sending is `thread.rs` and `thread_session.rs`.

use super::composer_menu::{self, MenuItem, MenuKind};
use super::composer_queue::Queue;
use super::lock;
use super::thread::ThreadView;
use gpui::{App, AppContext as _, Context, KeyBinding, SharedString, Window, actions};
use skyelab_agent::{Attachment, AttachmentKind, CLAUDE_MODELS, ModelInfo};
use skyelab_core::{Account, Provider};
use skyelab_store::ModelChoice;
use std::path::PathBuf;
use std::sync::Arc;

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

const INTERRUPT_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-."
} else {
    "ctrl-."
};
const STOP_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-alt-."
} else {
    "ctrl-alt-."
};

/// Set on the composer while a menu is open, so its keys reach the menu before the text field.
pub(in crate::workspace) const MENU_CONTEXT: &str = "ComposerMenu";
const STEER_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-shift-enter"
} else {
    "ctrl-shift-enter"
};

/// Binds the composer's keys. The menu's override the text field's own up/down/enter/tab/escape: same depth, bound later.
pub(in crate::workspace) fn bind_keys(cx: &mut App) {
    let menu = Some("ComposerMenu > Input");
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, menu),
        KeyBinding::new("down", MenuDown, menu),
        KeyBinding::new("enter", MenuAccept, menu),
        KeyBinding::new("tab", MenuAccept, menu),
        KeyBinding::new("escape", MenuClose, menu),
        KeyBinding::new(STEER_KEY, SteerPrompt, Some(super::thread::CONTEXT)),
        // Stop has no button (T3 has none either): the palette and this key.
        KeyBinding::new(
            INTERRUPT_KEY,
            super::thread::Interrupt,
            Some(super::thread::CONTEXT),
        ),
        KeyBinding::new(STOP_KEY, super::thread::Stop, Some(super::thread::CONTEXT)),
    ]);
    super::composer_draft::bind_keys(cx);
}

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
#[derive(Default)]
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
    pub menu: Option<Menu>,
    pub picker_open: bool,
    pub pick: ModelChoice,
    /// "Send to several models": the ticked models (`compare.rs`).
    pub compare: Option<crate::workspace::compare::Compare>,
    /// The user picked in this tab, so a late store read doesn't overwrite it.
    pub picked: bool,
    /// Codex's models, once listed (or why they couldn't be).
    pub codex_models: Option<Result<Vec<ModelInfo>, SharedString>>,
    /// The model this thread's agent last reported running (its init line).
    pub reported_model: Option<String>,
    /// The pick the run that reported it had (`None` inside: the account's default); `None`: replayed, unknown.
    pub reported_pick: Option<Option<String>>,
    /// The account's own default model, from its config dir.
    pub account_model: Option<String>,
    pub footer: Footer,
    /// Why the last attach or send was refused.
    pub notice: Option<SharedString>,
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

/// Why `account`'s agent can't take these files, if it can't: Codex reads images and text, not PDFs.
pub(in crate::workspace) fn unfit(account: &Account, attachments: &[Attachment]) -> Option<String> {
    let pdf = attachments
        .iter()
        .find(|a| account.provider == Provider::Codex && a.kind == AttachmentKind::Pdf)?;
    Some(format!(
        "Codex can't read PDFs ({}). Remove it, or attach an image or a text file.",
        pdf.name
    ))
}

/// A model pick `provider`'s CLI can take: Claude's aliases and `claude-…` ids for Claude, anything else for Codex.
/// A project default saved under the other provider is dropped, not sent.
pub(in crate::workspace) fn fits(provider: Provider, model: &str) -> bool {
    let claude =
        CLAUDE_MODELS.iter().any(|(alias, _)| *alias == model) || model.starts_with("claude");
    claude == (provider == Provider::Claude)
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
            let changes = skyelab_git::status(&cwd).ok().map(|s| {
                let mut paths: Vec<&std::path::Path> = s
                    .staged
                    .iter()
                    .chain(&s.unstaged)
                    .map(|f| f.path.as_path())
                    .chain(s.untracked.iter().map(PathBuf::as_path))
                    .collect();
                paths.sort();
                paths.dedup();
                paths.len()
            });
            let branch = skyelab_git::current_branch(&cwd).ok().flatten();
            (super::composer_machine::machine_name(), branch, changes)
        });
        cx.spawn(async move |this, cx| {
            let (machine, branch, changes) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.composer.footer = Footer {
                    machine: machine.map(Into::into),
                    branch: branch.map(Into::into),
                    changes,
                };
                cx.notify();
            });
        })
        .detach();
    }

    // The model picker.

    pub(in crate::workspace) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.composer.picker_open = !self.composer.picker_open;
        self.composer.menu = None;
        if self.composer.picker_open {
            self.list_codex_models(cx);
        }
        cx.notify();
    }

    /// Picks a model and effort: kept on the thread, remembered as the project's default for new threads, and
    /// handed to a running agent.
    pub(in crate::workspace) fn pick_model(&mut self, pick: ModelChoice, cx: &mut Context<Self>) {
        self.composer.pick = pick.clone();
        self.composer.picked = true;
        self.composer.picker_open = false;
        let (store, thread, project) = (
            self.backend.store.clone(),
            self.thread.as_ref().map(|t| t.id),
            self.project_id,
        );
        let session = self.session.clone();
        self.in_background(cx, move || {
            {
                let s = lock(&store)?;
                if let Some(id) = thread {
                    s.set_thread_model(id, &pick)?;
                }
                s.set_project_model(project, &pick)?;
            }
            if let Some(session) = session {
                lock(&session)?.set_model(pick.model.as_deref(), pick.effort.as_deref())?;
            }
            Ok(())
        });
        cx.notify();
    }

    // The `@` `/` `$` `#` menus.

    /// The prompt changed: open, narrow or close the menu its cursor's word asks for.
    pub(in crate::workspace) fn prompt_changed(&mut self, cx: &mut Context<Self>) {
        let (text, cursor) = {
            let p = self.prompt.read(cx);
            (p.value(), p.cursor())
        };
        cx.notify(); // Send turns into Interrupt and back as the prompt empties and fills
        let Some(t) = composer_menu::trigger(&text, cursor) else {
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
        self.composer.picker_open = false;
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

    fn load_menu_items(&mut self, kind: MenuKind, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        let account = self.composer_account();
        let contexts = (kind == MenuKind::Context).then(|| self.context_menu_items());
        let side_commands = !self.is_side_chat();
        let task = cx.background_spawn(async move {
            match (kind, account) {
                // Its own check: only threads of the composer's account (none when unbound).
                (MenuKind::Context, _) => contexts
                    .map_or(Ok(Vec::new()), |read| read())
                    .unwrap_or_default(),
                (MenuKind::Files, _) => skyelab_git::tracked_files(&cwd)
                    .map(|f| composer_menu::file_items(&f))
                    .unwrap_or_default(),
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_menu(-1, cx);
    }

    pub(in crate::workspace) fn menu_down(
        &mut self,
        _: &MenuDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_menu(1, cx);
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.menu = None;
        cx.notify();
    }

    pub(in crate::workspace) fn menu_accept(
        &mut self,
        _: &MenuAccept,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
