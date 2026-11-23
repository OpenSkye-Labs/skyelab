//! Renaming and removing projects, and the one-field dialog renames use. Removing never touches the folder.

use super::Workspace;
use super::manage::{Job, ManageProject, ProjectOp};
use super::thread::ThreadView;
use crate::ui;
use gpui::{AppContext as _, Context, Entity, ParentElement as _, Window};
use gpui_component::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    input::{Input, InputEvent, InputState},
    v_flex,
};

impl Workspace {
    pub(super) fn manage_project(
        &mut self,
        a: &ManageProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return;
        }
        let id = a.id;
        let Some(project) = self.projects.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        match a.op {
            ProjectOp::Rename => {
                self.prompt_text(
                    "Rename project",
                    project.name,
                    window,
                    cx,
                    move |ws, name, window, cx| {
                        let stored = name.clone();
                        let job: Job = Box::new(move |s| s.rename_project(id, &stored));
                        ws.store_then(job, move |ws, _, cx| ws.load_projects(None, cx), window, cx);
                    },
                );
            }
            ProjectOp::OpenFolder => self.open_dir_in_editor(project.path, None, window, cx),
            ProjectOp::Accounts => window.dispatch_action(Box::new(super::OpenSettings), cx),
            ProjectOp::ImportSessions => self.open_import(id, window, cx),
            ProjectOp::Remove => {
                let live = self
                    .project_views(id, cx)
                    .iter()
                    .filter(|v| {
                        let v = v.read(cx);
                        v.session.is_some() || v.starting
                    })
                    .count();
                if live == 0 {
                    return self.remove_project_now(id, window, cx);
                }
                self.pending_removal = Some(id);
                let ws = cx.entity().downgrade();
                let text = format!(
                    "{live} thread(s) in {} still have a running agent. Removing the project stops them. \
                     Files on disk are not touched.",
                    project.name
                );
                window.open_dialog(cx, move |dialog, _, _| {
                    let ws = ws.clone();
                    dialog
                        .w(gpui::px(420.))
                        .title("Remove project?")
                        .child(text.clone())
                        .footer(
                            DialogFooter::new()
                                .child(ui::dialog_cancel("remove-project-cancel"))
                                .child(
                                    Button::new("remove-project")
                                        .danger()
                                        .small()
                                        .label("Remove")
                                        .on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            let _ = ws.update(cx, |ws, cx| {
                                                ws.remove_project_now(id, window, cx)
                                            });
                                        }),
                                ),
                        )
                });
            }
        }
    }

    /// Open views of the project's threads.
    fn project_views(&self, project_id: i64, cx: &Context<Self>) -> Vec<Entity<ThreadView>> {
        self.open_threads
            .iter()
            .filter(|v| v.read(cx).project_id == project_id)
            .cloned()
            .collect()
    }

    /// Hides the project and closes its tabs (ending their agents). Its folder and thread history stay.
    pub(super) fn remove_project_now(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_removal = None;
        let job: Job = Box::new(move |s| s.set_project_removed(id, true));
        self.store_then(
            job,
            // Stored first: if that fails, the project stays and so do its agents.
            move |ws, window, cx| {
                for view in ws.project_views(id, cx) {
                    ws.close_thread(&view, window, cx);
                }
                if ws.project.as_ref().is_some_and(|p| p.id == id) {
                    ws.project = None;
                }
                ws.threads.retain(|t| t.project_id != id);
                // Gone now, not at the reload: New thread must not pick it meanwhile.
                ws.projects.retain(|p| p.id != id);
                ws.scope.retain(|&s| s != id);
                ws.load_projects(None, cx);
            },
            window,
            cx,
        );
    }

    /// A one-field dialog (rename), prefilled with `current`; `done` gets the trimmed, non-empty text.
    pub(super) fn prompt_text(
        &mut self,
        title: &'static str,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl Fn(&mut Self, String, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |s, cx| s.set_value(current, window, cx));
        let done = std::rc::Rc::new(done);
        let submit = {
            let (ws, input, done) = (cx.entity().downgrade(), input.clone(), done.clone());
            move |window: &mut Window, cx: &mut gpui::App| {
                let text = input.read(cx).value().trim().to_owned();
                if text.is_empty() {
                    return;
                }
                window.close_dialog(cx);
                let _ = ws.update(cx, |ws, cx| done(ws, text, window, cx));
            }
        };
        let on_enter = done.clone();
        self._dialog_input = Some(cx.subscribe_in(
            &input,
            window,
            move |ws, input, e: &InputEvent, window, cx| {
                let text = input.read(cx).value().trim().to_owned();
                if matches!(e, InputEvent::PressEnter { .. }) && !text.is_empty() {
                    window.close_dialog(cx);
                    on_enter(ws, text, window, cx);
                }
            },
        ));
        let field = input.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let submit = submit.clone();
            dialog
                .w(gpui::px(420.))
                .title(title)
                .child(v_flex().child(Input::new(&field)))
                .footer(
                    DialogFooter::new()
                        .child(ui::dialog_cancel("rename-cancel"))
                        .child(
                            Button::new("rename-save")
                                .primary()
                                .small()
                                .label("Save")
                                .on_click(move |_, window, cx| submit(window, cx)),
                        ),
                )
        });
        input.update(cx, |s, cx| s.focus(window, cx));
    }
}
