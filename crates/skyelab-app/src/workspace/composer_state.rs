//! What the composer holds and does: attachments, the `@` `/` `$` menus, the model pick, the queue and the footer's
//! facts. How it looks is `composer.rs`; sending is `thread.rs` and `thread_session.rs`.

use super::composer_menu::{self, MenuItem, MenuKind};
use super::composer_queue::{self, Queue, SendMode};
use super::thread::ThreadView;
use super::{lock, thread_model::Record};
use gpui::{App, AppContext as _, Context, KeyBinding, SharedString, Window, actions};
use skyelab_agent::{Attachment, ModelInfo};
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

/// Set on the composer while a menu is open, so its keys reach the menu before the text field.
pub(super) const MENU_CONTEXT: &str = "ComposerMenu";
const STEER_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-shift-enter"
} else {
    "ctrl-shift-enter"
};

/// Binds the composer's keys. The menu's override the text field's own up/down/enter/tab/escape: same depth, bound later.
pub(super) fn bind_keys(cx: &mut App) {
    let menu = Some("ComposerMenu > Input");
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, menu),
        KeyBinding::new("down", MenuDown, menu),
        KeyBinding::new("enter", MenuAccept, menu),
        KeyBinding::new("tab", MenuAccept, menu),
        KeyBinding::new("escape", MenuClose, menu),
        KeyBinding::new(STEER_KEY, SteerPrompt, Some(super::thread::CONTEXT)),
    ]);
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
    /// From another device or an agent: the text alone, sent now, as before the queue existed.
    External,
}

#[derive(Default)]
pub(crate) struct Composer {
    pub attachments: Vec<Attachment>,
    pub queue: Queue,
    pub menu: Option<Menu>,
    pub picker_open: bool,
    pub pick: ModelChoice,
    /// Codex's models, once listed (or why they couldn't be).
    pub codex_models: Option<Result<Vec<ModelInfo>, SharedString>>,
    pub footer: Footer,
    /// Why the last attach or send was refused.
    pub notice: Option<SharedString>,
    files: Option<Arc<Vec<MenuItem>>>,
}

/// The text a prompt is stored and shown as: what the agent got, plus the names of its files.
pub(super) fn shown_prompt(text: &str, attachments: &[Attachment]) -> String {
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

/// This machine's name as people know it (macOS's Computer Name), read once.
fn machine_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            // Absolute path and an empty env: nothing of ours leaks into the child.
            let out = std::process::Command::new("/usr/sbin/scutil")
                .args(["--get", "ComputerName"])
                .env_clear()
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()?;
            let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            (out.status.success() && !name.is_empty()).then_some(name)
        }
        #[cfg(windows)]
        {
            std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let name = std::fs::read_to_string("/proc/sys/kernel/hostname")
                .or_else(|_| std::fs::read_to_string("/etc/hostname"))
                .ok()?;
            Some(name.trim().to_owned()).filter(|n| !n.is_empty())
        }
    })
    .clone()
}

impl ThreadView {
    /// The account the composer acts for: the thread's own, or for a new thread the folder's.
    pub(super) fn composer_account(&self) -> Option<Account> {
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

    /// Loads the model pick (the thread's, else the project's default) and the footer's facts.
    pub(super) fn load_composer(&mut self, cx: &mut Context<Self>) {
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
            if let Ok(pick) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.composer.pick = pick;
                    cx.notify();
                });
            }
        })
        .detach();
        self.refresh_footer(cx);
    }

    /// Reads the branch, the changed-file count and the machine name again (after each turn).
    pub(super) fn refresh_footer(&mut self, cx: &mut Context<Self>) {
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
            (machine_name(), branch, changes)
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

    /// A turn ended: refresh the footer, and when no turn is left, send the next queued message.
    pub(super) fn after_turn(&mut self, cx: &mut Context<Self>) {
        self.refresh_footer(cx);
        if self.turn_running() || self.session.is_none() {
            return;
        }
        let (Some(next), Some(account)) = (self.composer.queue.pop(), self.composer_account())
        else {
            return;
        };
        if !self.follow_up(
            next.text.clone(),
            next.attachments.clone(),
            false,
            &account,
            cx,
        ) {
            self.composer.queue.put_back(next); // refused here (a rebound folder): it stays queued
        }
        cx.notify();
    }

    pub(super) fn turn_running(&self) -> bool {
        self.session.is_some() && self.turns.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    /// What a send of `text` does now.
    pub(super) fn send_mode(&self, how: Submit, text: &str) -> SendMode {
        match how {
            Submit::External => SendMode::Now,
            _ => composer_queue::mode(self.turn_running(), how == Submit::Steer, text),
        }
    }

    /// Sends queued message `id` into the running turn now; with no turn running, sends it as a new one. With no agent
    /// at all (it ended), the message goes back into an empty composer, where Send resumes the session.
    pub(super) fn steer_queued(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let account = self.composer_account();
        if self.session.is_none() || account.is_none() {
            let draft = !self.prompt.read(cx).value().trim().is_empty()
                || !self.composer.attachments.is_empty();
            if draft {
                self.composer.notice =
                    Some("Send or clear your draft first; the agent isn't running.".into());
            } else if let Some(q) = self.composer.queue.take(id) {
                self.prompt
                    .update(cx, |p, cx| p.set_value(q.text.clone(), window, cx));
                self.composer.attachments = q.attachments;
            }
            cx.notify();
            return;
        }
        let (Some(q), Some(account)) = (self.composer.queue.take(id), account) else {
            return;
        };
        let steer = self.turn_running() && !q.text.trim_start().starts_with('/');
        if !self.follow_up(q.text.clone(), q.attachments.clone(), steer, &account, cx) {
            self.composer.queue.put_back(q);
        }
        cx.notify();
    }

    pub(super) fn remove_queued(&mut self, id: u64, cx: &mut Context<Self>) {
        self.composer.queue.take(id);
        cx.notify();
    }

    /// Holds `text` and the attachments for after the running turn, shown in the queue above the composer.
    pub(super) fn queue_prompt(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let attachments = std::mem::take(&mut self.composer.attachments);
        self.composer.queue.push(text, attachments);
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        cx.notify();
    }

    /// The record a prompt is stored as.
    pub(super) fn prompt_record(text: &str, attachments: &[Attachment]) -> Record {
        Record::Prompt(shown_prompt(text, attachments))
    }

    // The model picker.

    pub(super) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.composer.picker_open = !self.composer.picker_open;
        self.composer.menu = None;
        let codex = self
            .composer_account()
            .filter(|a| a.provider == Provider::Codex);
        if let (true, None, Some(account)) = (
            self.composer.picker_open,
            &self.composer.codex_models,
            codex,
        ) {
            self.composer.codex_models = Some(Ok(Vec::new())); // loading; listed once per tab
            let (bin, config, cwd) = (
                super::github::codex_bin(cx),
                self.config.clone(),
                self.cwd.clone(),
            );
            let task = cx.background_spawn(async move {
                skyelab_agent::list_codex_models(&bin, &config, &account, &cwd)
                    .map_err(|e| SharedString::from(format!("{e:#}")))
            });
            cx.spawn(async move |this, cx| {
                let listed = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.composer.codex_models = Some(listed);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Picks a model and effort: kept on the thread, remembered as the project's default for new threads, and
    /// handed to a running agent.
    pub(super) fn pick_model(&mut self, pick: ModelChoice, cx: &mut Context<Self>) {
        self.composer.pick = pick.clone();
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

    // The `@` `/` `$` menus.

    /// The prompt changed: open, narrow or close the menu its cursor's word asks for.
    pub(super) fn prompt_changed(&mut self, cx: &mut Context<Self>) {
        let (text, cursor) = {
            let p = self.prompt.read(cx);
            (p.value(), p.cursor())
        };
        let Some(t) = composer_menu::trigger(&text, cursor) else {
            if self.composer.menu.take().is_some() {
                cx.notify();
            }
            return;
        };
        if let Some(menu) = self
            .composer
            .menu
            .as_mut()
            .filter(|m| m.kind == t.kind && m.start == t.start)
        {
            menu.query = t.query;
            if let Some(items) = &menu.items {
                menu.rows = composer_menu::rank(items, &menu.query);
                menu.selected = 0;
            }
            cx.notify();
            return;
        }
        self.composer.picker_open = false;
        let cached = (t.kind == MenuKind::Files)
            .then(|| self.composer.files.clone())
            .flatten();
        self.composer.menu = Some(Menu {
            kind: t.kind,
            start: t.start,
            query: t.query,
            rows: Vec::new(),
            selected: 0,
            items: None,
        });
        match cached {
            Some(items) => self.set_menu_items(t.kind, items, cx),
            None => self.load_menu_items(t.kind, cx),
        }
        cx.notify();
    }

    fn load_menu_items(&mut self, kind: MenuKind, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        let account = self.composer_account();
        let task = cx.background_spawn(async move {
            match (kind, account) {
                (MenuKind::Files, _) => skyelab_git::tracked_files(&cwd)
                    .map(|f| composer_menu::file_items(&f))
                    .unwrap_or_default(),
                // Unbound: no account dir to read, so nothing (never the parent's).
                (_, None) => Vec::new(),
                (MenuKind::Commands, Some(a)) => {
                    composer_menu::command_items(a.provider, &a.config_dir, &cwd)
                }
                (MenuKind::Skills, Some(a)) => {
                    composer_menu::skill_items(a.provider, &a.config_dir)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let items = Arc::new(task.await);
            let _ = this.update(cx, |this, cx| {
                if kind == MenuKind::Files {
                    this.composer.files = Some(items.clone());
                }
                this.set_menu_items(kind, items, cx);
            });
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
            menu.rows = composer_menu::rank(&items, &menu.query);
            menu.items = Some(items);
            menu.selected = 0;
            cx.notify();
        }
    }

    pub(super) fn menu_up(&mut self, _: &MenuUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_menu(-1, cx);
    }

    pub(super) fn menu_down(&mut self, _: &MenuDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_menu(1, cx);
    }

    fn move_menu(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(m) = self.composer.menu.as_mut().filter(|m| !m.rows.is_empty()) {
            let n = m.rows.len() as isize;
            m.selected = (m.selected as isize + by).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    pub(super) fn menu_close(&mut self, _: &MenuClose, _: &mut Window, cx: &mut Context<Self>) {
        self.composer.menu = None;
        cx.notify();
    }

    pub(super) fn menu_accept(
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
    pub(super) fn accept_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.composer.menu.take() else {
            return;
        };
        let Some(item) = menu
            .rows
            .get(ix)
            .and_then(|&i| menu.items.as_ref()?.get(i).cloned())
        else {
            cx.notify();
            return;
        };
        self.prompt.update(cx, |p, cx| {
            let cursor = p.cursor();
            p.set_selected_range(menu.start..cursor.max(menu.start), cx);
            p.replace(item.insert, window, cx);
        });
        self.focus_prompt(window, cx);
        cx.notify();
    }
}
