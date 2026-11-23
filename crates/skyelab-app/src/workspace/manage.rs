//! Thread and project management, as T3's thread menu has it: rename, pin, archive and delete (both undoable),
//! rename and remove projects. Everything here is local: in remote mode the handlers do nothing.

use super::thread::ThreadView;
use super::{Workspace, lock};
use crate::ui::Status;
use gpui::{Action, AppContext as _, Context, Entity, SharedString, Window, actions};
use gpui_component::{Sizable as _, WindowExt as _, button::Button, notification::Notification};
use skyelab_store::Store;
use std::time::Duration;

actions!(
    skyelab,
    [
        RenameThread,
        TogglePinThread,
        ArchiveThread,
        DeleteThread,
        RenameProject,
        RemoveProject,
        ToggleArchivedThreads
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadOp {
    Rename,
    Pin,
    Unpin,
    Archive,
    Unarchive,
    Delete,
}

/// A thread row's menu entry (the unit actions above act on the shown thread instead).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ManageThread {
    pub id: i64,
    pub op: ThreadOp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectOp {
    Rename,
    Remove,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ManageProject {
    pub id: i64,
    pub op: ProjectOp,
}

/// How long a deleted thread can be brought back before its row is purged.
pub(super) const UNDO_WINDOW: Duration = Duration::from_secs(8);

/// A store job run off the UI thread.
pub(super) type Job = Box<dyn FnOnce(&mut Store) -> anyhow::Result<()> + Send>;

impl Workspace {
    /// Runs `job` on the store, then `then` on success; a failure shows as the store error.
    pub(super) fn store_then(
        &mut self,
        job: Job,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.backend.store.clone();
        cx.spawn_in(window, async move |this, cx| {
            let done = cx
                .background_spawn(async move { job(&mut *lock(&store)?) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match done {
                Ok(()) => then(this, window, cx),
                Err(e) => {
                    this.store_error = Some(format!("{e:#}").into());
                    cx.notify();
                }
            });
        })
        .detach(); // short; `this` is weak, so a closed window just drops it
    }

    /// The thread's agent is mid-turn (or starting): archiving or deleting it would cut the work off.
    fn working(&self, id: i64, cx: &Context<Self>) -> bool {
        self.open_view(id, cx)
            .is_some_and(|v| matches!(v.read(cx).live_status(), Status::Starting | Status::Running))
    }

    /// Closes the thread's tab if it has one; closing ends its agent.
    pub(super) fn close_thread_tab(
        &mut self,
        view: Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock
            .update(cx, |dock, cx| dock.remove_panel(view, window, cx));
    }

    fn say(&mut self, text: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.into();
        #[cfg(test)]
        self.notices.push(text.clone());
        super::manage::push_note(window, Notification::warning(text), cx);
    }

    /// A notice with an Undo button that runs `undo` on this workspace.
    fn undo_notice(
        &mut self,
        text: String,
        undo: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(test)]
        self.notices.push(text.clone().into());
        let ws = cx.entity().downgrade();
        let undo = std::rc::Rc::new(undo);
        let note = Notification::new()
            .message(text)
            .action(move |_, _, _| {
                let (ws, undo) = (ws.clone(), undo.clone());
                Button::new("undo")
                    .xsmall()
                    .label("Undo")
                    .on_click(move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| undo(ws, window, cx));
                    })
            })
            .autohide(true);
        super::manage::push_note(window, note, cx);
    }

    fn update_row(
        &mut self,
        id: i64,
        f: impl FnOnce(&mut skyelab_store::Thread),
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
            f(t);
        }
        self.refresh_sidebar(cx);
    }

    pub(super) fn manage_thread(
        &mut self,
        a: &ManageThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // these rows are this machine's; another machine's sidebar is shown
        }
        let id = a.id;
        match a.op {
            ThreadOp::Rename => {
                let Some(current) = self.thread_title(id, cx) else {
                    return;
                };
                self.prompt_text(
                    "Rename thread",
                    current,
                    window,
                    cx,
                    move |ws, title, window, cx| ws.rename_thread(id, title, window, cx),
                );
            }
            ThreadOp::Pin | ThreadOp::Unpin => {
                let on = a.op == ThreadOp::Pin;
                let job: Job = Box::new(move |s| s.set_thread_pinned(id, on));
                self.store_then(
                    job,
                    move |ws, _, cx| {
                        ws.update_row(id, |t| t.pinned_at = on.then(crate::ui::now_ms), cx)
                    },
                    window,
                    cx,
                );
            }
            ThreadOp::Archive => self.archive_thread(id, true, window, cx),
            ThreadOp::Unarchive => self.archive_thread(id, false, window, cx),
            ThreadOp::Delete => self.delete_thread(id, window, cx),
        }
    }

    fn thread_title(&self, id: i64, cx: &Context<Self>) -> Option<String> {
        self.threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.title.clone())
            .or_else(|| {
                self.open_view(id, cx)
                    .map(|v| v.read(cx).title().to_string())
            })
    }

    pub(super) fn rename_thread(
        &mut self,
        id: i64,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stored = title.clone();
        let job: Job = Box::new(move |s| s.set_thread_title(id, &stored));
        self.store_then(
            job,
            move |ws, _, cx| {
                if let Some(view) = ws.open_view(id, cx) {
                    view.update(cx, |v, cx| {
                        if let Some(t) = v.thread.as_mut() {
                            t.title = title.clone();
                        }
                        cx.notify();
                    });
                }
                ws.update_row(id, |t| t.title = title, cx);
            },
            window,
            cx,
        );
    }

    pub(super) fn archive_thread(
        &mut self,
        id: i64,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if on && self.working(id, cx) {
            return self.say(
                "This thread is working. Stop it before archiving.",
                window,
                cx,
            );
        }
        if on && let Some(view) = self.open_view(id, cx) {
            self.close_thread_tab(view, window, cx);
        }
        let title = self.thread_title(id, cx).unwrap_or_default();
        let job: Job = Box::new(move |s| s.set_thread_archived(id, on));
        self.store_then(
            job,
            move |ws, window, cx| {
                ws.update_row(id, |t| t.archived_at = on.then(crate::ui::now_ms), cx);
                if on {
                    ws.undo_notice(
                        format!("Archived “{title}”"),
                        move |ws, window, cx| ws.archive_thread(id, false, window, cx),
                        window,
                        cx,
                    );
                }
            },
            window,
            cx,
        );
    }

    /// Hides the thread now and purges it once the undo window has passed (or at the next start).
    pub(super) fn delete_thread(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.working(id, cx) {
            return self.say(
                "This thread is working. Stop it before deleting.",
                window,
                cx,
            );
        }
        // The open tab's row too: the thread may belong to a project the sidebar no longer lists.
        let in_list = self
            .threads
            .iter()
            .any(|t| t.id == id && t.worktree_path.is_some());
        let in_tab = self.open_view(id, cx).is_some_and(|v| {
            v.read(cx)
                .thread
                .as_ref()
                .is_some_and(|t| t.worktree_path.is_some())
        });
        if in_list || in_tab {
            return self.say(
                "This thread has its own worktree. Discard the worktree first.",
                window,
                cx,
            );
        }
        if let Some(view) = self.open_view(id, cx) {
            self.close_thread_tab(view, window, cx);
        }
        let title = self.thread_title(id, cx).unwrap_or_default();
        let job: Job = Box::new(move |s| s.set_thread_deleted(id, true));
        self.store_then(
            job,
            move |ws, window, cx| {
                ws.threads.retain(|t| t.id != id);
                ws.refresh_sidebar(cx);
                ws.undo_notice(
                    format!("Deleted “{title}”"),
                    move |ws, window, cx| ws.undelete_thread(id, window, cx),
                    window,
                    cx,
                );
                let store = ws.backend.store.clone();
                let timer = cx.background_executor().timer(UNDO_WINDOW);
                // Replacing an earlier delete's purge cancels it, so each delete gets its full undo window.
                let purge = cx.background_spawn(async move {
                    timer.await;
                    // Best effort: a failed purge is retried by the next open.
                    let purged = lock(&store).ok().and_then(|s| {
                        let thread = s.purge_thread(id).ok().flatten()?;
                        let project = s.project(thread.project_id).ok().flatten();
                        Some((thread, project.map(|p| p.path)))
                    });
                    // Outside the store lock: git can take a while.
                    if let Some((thread, project)) = purged {
                        super::thread_checkpoints::forget(&thread, project.as_deref());
                    }
                });
                ws.purges.insert(id, purge);
            },
            window,
            cx,
        );
    }

    /// Undo for a delete: the thread is listed again (once purged, the store says there is no such thread).
    pub(super) fn undelete_thread(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.purges.remove(&id);
        let job: Job = Box::new(move |s| s.set_thread_deleted(id, false));
        self.store_then(job, |ws, _, cx| ws.load_threads(cx), window, cx);
    }

    /// The thread shown in the center, for the unit actions (Rename Thread…).
    fn act_on_shown(&mut self, op: ThreadOp, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.shown_thread(cx) else {
            return;
        };
        let op = match op {
            ThreadOp::Pin
                if self
                    .threads
                    .iter()
                    .any(|t| t.id == id && t.pinned_at.is_some()) =>
            {
                ThreadOp::Unpin
            }
            op => op,
        };
        self.manage_thread(&ManageThread { id, op }, window, cx);
    }

    pub(super) fn manage_actions(
        &self,
        el: gpui::Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        el.on_action(cx.listener(Self::manage_thread))
            .on_action(cx.listener(Self::manage_project))
            .on_action(
                cx.listener(|ws, _: &RenameThread, w, cx| ws.act_on_shown(ThreadOp::Rename, w, cx)),
            )
            .on_action(
                cx.listener(|ws, _: &TogglePinThread, w, cx| ws.act_on_shown(ThreadOp::Pin, w, cx)),
            )
            .on_action(
                cx.listener(|ws, _: &ArchiveThread, w, cx| {
                    ws.act_on_shown(ThreadOp::Archive, w, cx)
                }),
            )
            .on_action(
                cx.listener(|ws, _: &DeleteThread, w, cx| ws.act_on_shown(ThreadOp::Delete, w, cx)),
            )
            .on_action(cx.listener(|ws, _: &RenameProject, w, cx| {
                if let Some(id) = ws.project.as_ref().map(|p| p.id) {
                    ws.manage_project(
                        &ManageProject {
                            id,
                            op: ProjectOp::Rename,
                        },
                        w,
                        cx,
                    )
                }
            }))
            .on_action(cx.listener(|ws, _: &RemoveProject, w, cx| {
                if let Some(id) = ws.project.as_ref().map(|p| p.id) {
                    ws.manage_project(
                        &ManageProject {
                            id,
                            op: ProjectOp::Remove,
                        },
                        w,
                        cx,
                    )
                }
            }))
            .on_action(cx.listener(|ws, _: &ToggleArchivedThreads, _, cx| {
                ws.show_archived = !ws.show_archived;
                ws.refresh_sidebar(cx);
            }))
    }
}

/// Shows a toast. A window without gpui-component's `Root` (some tests) has nowhere to show one, so it is skipped.
pub(super) fn push_note(window: &mut Window, note: Notification, cx: &mut gpui::App) {
    if let Some(Some(_)) = window.root::<gpui_component::Root>() {
        window.push_notification(note, cx);
    }
}

/// The menu a thread row offers, by its state.
pub(super) fn thread_menu(t: &skyelab_store::Thread) -> Vec<(SharedString, Box<dyn Action>)> {
    let id = t.id;
    let entry = |label: &'static str, op| {
        (
            SharedString::from(label),
            Box::new(ManageThread { id, op }) as Box<dyn Action>,
        )
    };
    vec![
        entry("Rename", ThreadOp::Rename),
        if t.pinned_at.is_some() {
            entry("Unpin", ThreadOp::Unpin)
        } else {
            entry("Pin", ThreadOp::Pin)
        },
        if t.archived_at.is_some() {
            entry("Unarchive", ThreadOp::Unarchive)
        } else {
            entry("Archive", ThreadOp::Archive)
        },
        entry("Delete", ThreadOp::Delete),
    ]
}

/// The menu a project row offers.
pub(super) fn project_menu(id: i64) -> Vec<(SharedString, Box<dyn Action>)> {
    vec![
        (
            "Rename".into(),
            Box::new(ManageProject {
                id,
                op: ProjectOp::Rename,
            }),
        ),
        (
            "Remove from SkyeLab".into(),
            Box::new(ManageProject {
                id,
                op: ProjectOp::Remove,
            }),
        ),
    ]
}
