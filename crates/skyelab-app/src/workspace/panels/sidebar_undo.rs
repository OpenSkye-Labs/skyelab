//! The last thread action's Undo in the sidebar footer (T3's `SidebarThreadUndoNotice`): "Settled “x” · Undo" sits
//! below the list instead of a toast over the right panel, and never moves or covers a row.

use super::sidebar::Sidebar;
use super::*;
use crate::ui::SkyeIcon;
use std::rc::Rc;
use std::time::Duration;

/// How long the note stays when the pointer isn't on it.
const SHOWN: Duration = Duration::from_secs(6);

/// What a note's Undo runs.
type Undo = Rc<dyn Fn(&mut Window, &mut App)>;

pub(in crate::workspace) struct UndoNote {
    pub text: SharedString,
    undo: Undo,
    hovered: bool,
    _hide: gpui::Task<()>,
}

impl Sidebar {
    /// Shows `text` with an Undo that runs `undo`, replacing any earlier note.
    pub(in crate::workspace) fn show_undo(
        &mut self,
        text: SharedString,
        undo: impl Fn(&mut Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let hide = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SHOWN).await;
            // The pointer is on it: wait until it leaves, but not past the action's own undo window (a delete is
            // purged after `manage::UNDO_WINDOW`, 8 s), or the Undo shown would do nothing.
            for _ in 0..2 {
                let hovered = this
                    .update(cx, |s, _| s.undo.as_ref().is_some_and(|u| u.hovered))
                    .unwrap_or(false);
                if !hovered {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
            let _ = this.update(cx, |s, cx| {
                s.undo = None;
                cx.notify();
            });
        });
        self.undo = Some(UndoNote {
            text,
            undo: Rc::new(undo),
            hovered: false,
            _hide: hide,
        });
        cx.notify();
    }

    /// The note, filling the footer's free space; `None` when there is none.
    pub(super) fn render_undo(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let note = self.undo.as_ref()?;
        let theme = cx.theme();
        Some(
            h_flex()
                .id("sidebar-undo-note")
                .flex_1()
                .min_w_0()
                .gap_1()
                .pl_1()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .on_hover(cx.listener(|this, on: &bool, _, _| {
                    if let Some(u) = this.undo.as_mut() {
                        u.hovered = *on;
                    }
                }))
                .child(div().flex_1().min_w_0().truncate().child(note.text.clone()))
                .child(
                    div().debug_selector(|| "sidebar-undo".into()).child(
                        Button::new("sidebar-undo")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(SkyeIcon::Undo2).size_3())
                            .label("Undo")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(u) = this.undo.take() {
                                    // After this update: the undo may touch the sidebar again.
                                    window.defer(cx, move |window, cx| (u.undo)(window, cx));
                                }
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element(),
        )
    }
}
