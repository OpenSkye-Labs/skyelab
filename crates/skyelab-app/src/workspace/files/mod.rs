//! The thread folder's files: the mod+P picker, the Search tab and the Preview tab, and file links in replies.
//! Everything reads only inside the folder (`skyelab_core::files`); a workspace showing another machine opens none.

mod links;
#[cfg(test)]
mod links_test;
mod picker;
#[cfg(test)]
mod picker_test;
mod preview;
#[cfg(test)]
mod remote_test;
mod search;

pub(super) use links::{link_line, linkify};
pub(super) use picker::bind_keys;

use super::Workspace;
use gpui::{AppContext as _, Context, Subscription, WeakEntity, Window, actions};
use preview::{OpenInEditor, PreviewPanel};
use search::{PreviewAt, SearchPanel};
use std::path::PathBuf;
use std::rc::Rc;

actions!(skyelab, [OpenFilePicker, SearchFiles]);

/// The window's one Search tab and one Preview tab, while open.
#[derive(Default)]
pub(super) struct FileTabs {
    search: Option<WeakEntity<SearchPanel>>,
    preview: Option<WeakEntity<PreviewPanel>>,
    _events: Vec<Subscription>,
}

impl Workspace {
    /// Where files come from: the shown thread's folder (its worktree, if it has one), else the project's. `None`
    /// when the workspace shows another machine: its files aren't here.
    pub(super) fn files_root(&self, cx: &gpui::App) -> Option<PathBuf> {
        if self.is_remote() {
            return None;
        }
        match self.current_thread() {
            Some(v) => Some(v.read(cx).cwd.clone()),
            None => self.project.as_ref().map(|p| p.path.clone()),
        }
    }

    fn open_file_picker(
        &mut self,
        _: &OpenFilePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.files_root(cx) else {
            return;
        };
        let ws = cx.entity().downgrade();
        let at = root.clone();
        let on_pick: picker::OnPick = Rc::new(move |path, editor, window, cx| {
            let at = at.clone();
            let _ = ws.update(cx, |ws, cx| {
                if editor {
                    ws.open_file_in_editor(&at, &path, window, cx)
                } else {
                    ws.open_preview(at, path, None, window, cx)
                }
            });
        });
        let prev = window.focused(cx).unwrap_or(self.focus.clone());
        picker::open(root, prev, on_pick, window, cx);
    }

    fn search_files(&mut self, _: &SearchFiles, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root(cx) else {
            return;
        };
        let tab = match self.files.search.as_ref().and_then(|t| t.upgrade()) {
            Some(tab) => {
                tab.update(cx, |t, cx| t.set_root(root, cx));
                self.show_right_panel(tab.entity_id().into(), window, cx);
                tab
            }
            None => {
                let tab = cx.new(|cx| SearchPanel::new(root, window, cx));
                let sub = cx.subscribe_in(&tab, window, |ws, _, e: &PreviewAt, window, cx| {
                    ws.open_preview(e.root.clone(), e.path.clone(), Some(e.line), window, cx)
                });
                self.files._events.push(sub);
                self.add_right_panel(tab.clone(), window, cx);
                self.files.search = Some(tab.downgrade());
                tab
            }
        };
        tab.update(cx, |t, cx| t.focus_input(window, cx));
    }

    /// Shows `path` (relative to `root`, or absolute) in the Preview tab at `line`; the read refuses anything
    /// outside `root`.
    pub(super) fn open_preview(
        &mut self,
        root: PathBuf,
        path: PathBuf,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return;
        }
        let tab = match self.files.preview.as_ref().and_then(|t| t.upgrade()) {
            Some(tab) => {
                self.show_right_panel(tab.entity_id().into(), window, cx);
                tab
            }
            None => {
                let tab = cx.new(|cx| PreviewPanel::new(window, cx));
                let sub = cx.subscribe_in(&tab, window, |ws, _, e: &OpenInEditor, window, cx| {
                    ws.open_file_in_editor(&e.root, &e.path, window, cx)
                });
                self.files._events.push(sub);
                self.add_right_panel(tab.clone(), window, cx);
                self.files.preview = Some(tab.downgrade());
                tab
            }
        };
        tab.update(cx, |t, cx| t.show(root, path, line, window, cx));
    }

    /// Opens a file of `root` in the user's editor, only when it resolves inside `root`.
    fn open_file_in_editor(
        &mut self,
        root: &std::path::Path,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = skyelab_core::files::inside(root, path) {
            self.open_dir_in_editor(file, None, window, cx);
        }
    }

    pub(super) fn file_actions(
        &self,
        el: gpui::Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        el.on_action(cx.listener(Self::open_file_picker))
            .on_action(cx.listener(Self::search_files))
    }
}
