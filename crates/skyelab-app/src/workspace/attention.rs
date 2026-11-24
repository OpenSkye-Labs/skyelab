//! What draws the user back to a thread, and the keys to move between threads: unread markers and notifications
//! (T3's ThreadNotificationCoordinator), previous/next and 1–9, Open in editor.

use super::thread::ThreadView;
use super::{OpenThread, Workspace, lock};
use gpui::{
    Action, App, AppContext as _, Context, Entity, KeyBinding, KeyBindingContextPredicate,
    NoAction, SharedString, Window, actions,
};
use gpui_component::notification::Notification;
use std::path::{Path, PathBuf};
use std::rc::Rc;

actions!(
    skyelab,
    [
        PreviousThread,
        NextThread,
        OpenInEditor,
        JumpToThread1,
        JumpToThread2,
        JumpToThread3,
        JumpToThread4,
        JumpToThread5,
        JumpToThread6,
        JumpToThread7,
        JumpToThread8,
        JumpToThread9
    ]
);

/// T3's thread keys (`mod+n`, `mod+shift+[ / ]`, `mod+o`, `mod+1..9`), after `secondary-`.
pub(super) const THREAD_KEYS: [&str; 13] = [
    "n", "shift-[", "shift-]", "o", "1", "2", "3", "4", "5", "6", "7", "8", "9",
];

fn key_actions() -> [Box<dyn Action>; 13] {
    [
        Box::new(super::NewThread),
        Box::new(PreviousThread),
        Box::new(NextThread),
        Box::new(OpenInEditor),
        Box::new(JumpToThread1),
        Box::new(JumpToThread2),
        Box::new(JumpToThread3),
        Box::new(JumpToThread4),
        Box::new(JumpToThread5),
        Box::new(JumpToThread6),
        Box::new(JumpToThread7),
        Box::new(JumpToThread8),
        Box::new(JumpToThread9),
    ]
}

/// Binds the thread keys. Off macOS they are ctrl- keys, which a shell uses (readline), so a focused terminal
/// keeps them.
pub fn init(cx: &mut App) {
    let context = |c: &str| KeyBindingContextPredicate::parse(c).ok().map(Rc::new);
    let (workspace, terminal) = (context(super::CONTEXT), context("Terminal"));
    let load = |keys: &str, action, context| {
        KeyBinding::load(keys, action, context, false, None, cx)
            .expect("static keystrokes are valid")
    };
    // A real keyboard reports ⌘⇧[ as `cmd-{` (macOS folds shift into the character); simulated keys say
    // `cmd-shift-[`. Both spellings are bound, the shift one first so the palette shows it.
    let spelled_both = [("shift-[", "{"), ("shift-]", "}")];
    let mut keys = Vec::new();
    for (key, action) in THREAD_KEYS.iter().zip(key_actions()) {
        let alias = spelled_both.iter().find(|(k, _)| k == key).map(|(_, a)| *a);
        for key in std::iter::once(*key).chain(alias) {
            keys.push(load(
                &format!("secondary-{key}"),
                action.boxed_clone(),
                workspace.clone(),
            ));
            if !cfg!(target_os = "macos") {
                keys.push(load(
                    &format!("ctrl-{key}"),
                    Box::new(NoAction),
                    terminal.clone(),
                ));
            }
        }
    }
    cx.bind_keys(keys);
}

/// Tags a thread's notification, so a newer one for the same thread replaces it.
struct ThreadNote;

impl Workspace {
    pub(super) fn shown_thread(&self, cx: &App) -> Option<i64> {
        self.current_thread()?
            .read(cx)
            .thread
            .as_ref()
            .map(|t| t.id)
    }

    /// A thread's turn finished: unless the user is looking at it, mark it unread and tell them.
    pub(super) fn turn_done(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = view.read(cx).thread.clone() else {
            return;
        };
        let here = window.is_window_active() && self.shown_thread(cx) == Some(thread.id);
        if here || self.seen_elsewhere(thread.id, cx) {
            return;
        }
        self.mark_unread(view, cx);
        self.rerun_search(cx); // the turn added message text the filter may match
        if thread.archived_at.is_none() {
            let note = Notification::success(thread.title).title("Thread completed");
            self.notify_attention(thread.id, note, window, cx);
        }
    }

    /// A thread's agent waits on a permission answer: unless the user is looking at it, tell them.
    pub(super) fn approval_needed(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = view.read(cx).thread.clone() else {
            return;
        };
        let here = window.is_window_active() && self.shown_thread(cx) == Some(thread.id);
        if here || self.seen_elsewhere(thread.id, cx) {
            return;
        }
        let note = Notification::warning(thread.title).title("Approval needed");
        self.notify_attention(thread.id, note, window, cx);
    }

    /// Marks the thread unread (stored, in the list and on its view's row) until the user looks at it.
    pub(super) fn mark_unread(&mut self, view: &Entity<ThreadView>, cx: &mut Context<Self>) {
        let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) else {
            return;
        };
        self.write_unread(id, true, cx);
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
            t.unread = true;
        }
        // The view's row too: the thread may be in a project the sidebar isn't showing.
        view.update(cx, |v, _| {
            if let Some(t) = v.thread.as_mut() {
                t.unread = true;
            }
        });
        self.refresh_sidebar(cx);
    }

    /// T3's rule: the window is focused on another thread → an in-app toast; the window is in the background → an
    /// OS notification. Either opens the thread when clicked. `note` carries the headline ("Thread completed";
    /// the permissions work passes "Approval needed") and the thread's title.
    pub(crate) fn notify_attention(
        &mut self,
        id: i64,
        note: Notification,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(test)]
        self.notices.push(format!("thread {id}").into());
        let ws = cx.entity().downgrade();
        let note = note
            .id1::<ThreadNote>(SharedString::from(format!("thread-{id}")))
            .on_click(move |_, window, cx| {
                window.activate_window();
                let _ = ws.update(cx, |ws, cx| ws.open_thread(&OpenThread { id }, window, cx));
            });
        let note = if window.is_window_active() {
            note
        } else {
            note.system()
        };
        super::manage::push_note(window, note, cx);
    }

    /// Clears unread when the window comes back to a thread, and runs the sidebar's thread filter.
    pub(super) fn watch_attention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.mark_seen(window, cx);
            if window.is_window_active() {
                // A thread shown here and there last took its prompt (and the focus) to the other window.
                if window.focused(cx).is_none() {
                    this.apply_page(window, cx);
                } else if let Some(view) = this.current_thread() {
                    // The window in use owns the shared thread it shows: its clicks act here.
                    view.update(cx, |v, cx| v.attach(window, cx));
                }
                this.poll_prs(true, cx);
                // Another window may have added, renamed or removed projects and threads meanwhile.
                if this.has_other_windows(cx) {
                    this.load_projects(None, cx);
                }
            }
        });
        let search = cx.subscribe(
            &self.sidebar.clone(),
            |this, _, e: &super::panels::SidebarSearch, cx| this.search_threads(e.0.clone(), cx),
        );
        self._attention = vec![activation, search];
    }

    /// The shown thread was looked at: clear its unread marker.
    pub(super) fn mark_seen(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            return;
        }
        let Some(view) = self.current_thread() else {
            return;
        };
        let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) else {
            return;
        };
        self.clear_woke(id, cx);
        let in_tab = view.update(cx, |v, _| {
            v.thread
                .as_mut()
                .is_some_and(|t| std::mem::replace(&mut t.unread, false))
        });
        let in_list = self
            .threads
            .iter_mut()
            .find(|t| t.id == id)
            .is_some_and(|t| std::mem::replace(&mut t.unread, false));
        if in_tab || in_list {
            self.write_unread(id, false, cx);
            self.refresh_sidebar(cx);
        }
    }

    /// Stores a thread's unread mark after any earlier write, so a quick mark-then-clear can't land reversed.
    fn write_unread(&mut self, id: i64, unread: bool, cx: &mut Context<Self>) {
        let (prev, store) = (self._unread_write.take(), self.backend.store.clone());
        self._unread_write = Some(cx.background_spawn(async move {
            if let Some(prev) = prev {
                prev.await;
            }
            if let Ok(s) = lock(&store) {
                let _ = s.set_thread_unread(id, unread); // best effort: the dot is a hint, the thread is intact
            }
        }));
    }

    fn step_thread(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // the order is this machine's threads; another machine's sidebar is shown
        }
        let order = self.thread_order(cx);
        if order.is_empty() {
            return;
        }
        let at = self
            .shown_thread(cx)
            .and_then(|id| order.iter().position(|&o| o == id));
        let next = match at {
            Some(i) => (i as isize + delta).rem_euclid(order.len() as isize) as usize,
            None if delta > 0 => 0,
            None => order.len() - 1,
        };
        self.open_thread(&OpenThread { id: order[next] }, window, cx);
    }

    fn jump(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // the order is this machine's threads; another machine's sidebar is shown
        }
        if let Some(&id) = self.thread_order(cx).get(n - 1) {
            self.open_thread(&OpenThread { id }, window, cx);
        }
    }

    fn open_in_editor(&mut self, _: &OpenInEditor, window: &mut Window, cx: &mut Context<Self>) {
        self.open_editor(None, window, cx);
    }

    /// Opens in the editor the details card's chooser picked, and keeps it as the `editor` setting.
    pub(super) fn open_in_editor_with(
        &mut self,
        a: &super::thread_details::OpenInEditorWith,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_editor(Some(a.editor.clone()), window, cx);
    }

    /// Opens the shown thread's folder (its worktree, if it has one), else the project's, in the user's editor
    /// (`pick`, saved to settings first, else the configured one).
    fn open_editor(&mut self, pick: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // the folder is on another machine
        }
        let dir = match self.current_thread() {
            Some(v) => v.read(cx).cwd.clone(),
            None => match &self.project {
                Some(p) => p.path.clone(),
                None => return,
            },
        };
        self.open_dir_in_editor(dir, pick, window, cx);
    }

    /// Opens `dir` (or a file) in the user's editor (`pick`, saved to settings first, else the configured one); a missing
    /// editor shows as a notice.
    pub(super) fn open_dir_in_editor(
        &mut self,
        dir: PathBuf,
        pick: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.layout.path.clone();
        let task = cx.background_spawn(async move {
            if let (Some(pick), Some(path)) = (&pick, &settings) {
                skyelab_core::AppSettings::update(path, |s| s.editor = Some(pick.clone()))?;
            }
            let setting = pick.or_else(|| {
                settings
                    .and_then(|p| skyelab_core::AppSettings::load(&p).ok())
                    .and_then(|s| s.editor)
            });
            let argv = skyelab_core::editor_argv(
                setting.as_deref(),
                |k| std::env::var(k).ok(),
                |n| skyelab_agent::resolve_bin(Path::new(n)).is_ok(),
            )
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No editor found. Set `editor` in settings.toml, or $VISUAL / $EDITOR."
                )
            })?;
            let mut child = skyelab_core::process::spawn(&mut editor_command(&argv, &dir)?)?;
            std::thread::spawn(move || child.wait()); // reap it; the editor outlives nothing of ours
            anyhow::Ok(())
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Err(e) = task.await {
                let _ = this.update_in(cx, |_, window, cx| {
                    super::manage::push_note(window, Notification::error(format!("{e:#}")), cx)
                });
            }
        })
        .detach();
    }

    pub(super) fn attention_actions(
        &self,
        el: gpui::Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        macro_rules! jumps {
            ($el:expr, $($a:ident => $n:literal),*) => {
                $el$(.on_action(cx.listener(|ws, _: &$a, w, cx| ws.jump($n, w, cx))))*
            };
        }
        let el = el
            .on_action(cx.listener(|ws, _: &PreviousThread, w, cx| ws.step_thread(-1, w, cx)))
            .on_action(cx.listener(|ws, _: &NextThread, w, cx| ws.step_thread(1, w, cx)))
            .on_action(cx.listener(Self::open_in_editor));
        jumps!(el, JumpToThread1 => 1, JumpToThread2 => 2, JumpToThread3 => 3, JumpToThread4 => 4,
            JumpToThread5 => 5, JumpToThread6 => 6, JumpToThread7 => 7, JumpToThread8 => 8, JumpToThread9 => 9)
    }
}

/// The editor process: no shell, the folder appended, and only the env a terminal would get (never an account's
/// `CLAUDE_CONFIG_DIR` or `CODEX_HOME` from ours).
pub(super) fn editor_command(argv: &[String], dir: &Path) -> anyhow::Result<std::process::Command> {
    let (bin, args) = argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty editor command"))?;
    let bin = skyelab_agent::resolve_bin(Path::new(bin))?;
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .arg(dir)
        // `dir` may be a file (the picker's mod+Enter): the editor starts in its folder.
        .current_dir(if dir.is_file() {
            dir.parent().unwrap_or(dir)
        } else {
            dir
        })
        .env_clear()
        .envs(skyelab_core::env::terminal_vars(
            std::env::vars_os(),
            cfg!(windows),
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(cmd)
}
