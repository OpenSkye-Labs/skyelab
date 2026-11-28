//! Undo notices: a toast with Undo, and a thread row's Undo in the sidebar footer.

use super::super::Workspace;
use crate::ui::toast::Toast;
use gpui::{Context, Window};

impl Workspace {
    /// A notice with an Undo button that runs `undo` on this workspace.
    pub(in crate::workspace) fn undo_notice(
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
        let note = Toast::new(text)
            .action("Undo", move |window, cx| {
                let _ = ws.update(cx, |ws, cx| undo(ws, window, cx));
            })
            .autohide(true);
        crate::ui::toast::push(window, note, cx);
    }

    /// A thread row's Undo (settle, snooze, archive, delete): in the sidebar footer under the list it changed (T3's
    /// `SidebarThreadUndoNotice`), or a toast while the sidebar is hidden.
    pub(in crate::workspace) fn thread_undo_notice(
        &mut self,
        text: String,
        undo: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::ui::dock::DockPlacement;
        if !self.dock.read(cx).is_dock_open(DockPlacement::Left) {
            return self.undo_notice(text, undo, window, cx);
        }
        #[cfg(test)]
        self.notices.push(text.clone().into());
        let ws = cx.entity().downgrade();
        self.sidebar.update(cx, |s, cx| {
            s.show_undo(
                text.into(),
                move |window, cx| {
                    let _ = ws.update(cx, |ws, cx| undo(ws, window, cx));
                },
                cx,
            )
        });
    }
}
