//! Right-click context menus for terminal tabs (bottom dock) and right-panel dock tabs.
//!
//! Each dock panel wraps its title in [`tab_title`], which records direct right-clicks and painted bounds so
//! [`attach_tab_bar`] in `center_skin.rs` can resolve any right-click inside a tab (including its padding and close
//! button) to the exact [`PanelId`] without duplicating gpui-component's `TabGroupSkin`.

use super::files::PreviewPanel;
use super::terminal_panel::TerminalPanel;
use super::{NewTerminal, Workspace};
use crate::ui::dock::{DockPlacement, PanelId, TabGroupContext};
use crate::ui::{PopupMenu, PopupMenuItem};
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, Pixels, Point, Subscription, WeakEntity,
    Window, anchored, deferred, div, prelude::*, px,
};
use skyelab_core::terminal_layout::Axis;

#[path = "tab_hits.rs"]
mod tab_hits;
use tab_hits::tab_at;
pub(in crate::workspace) use tab_hits::tab_title;

/// Attaches right-click context menu handling to a dock tab bar wrapper (`center_skin.rs`).
pub(super) fn attach_tab_bar(
    el: gpui::Div,
    group: &TabGroupContext,
    workspace: &WeakEntity<Workspace>,
    cx: &App,
) -> gpui::Div {
    let is_terminal_bar = group
        .panels()
        .iter()
        .any(|p| p.panel_name(cx) == "Terminal");
    let is_right_bar = group
        .panels()
        .iter()
        .any(|p| p.panel_name(cx) == "Surfaces");
    if !is_terminal_bar && !is_right_bar {
        return el;
    }
    let group = group.clone();
    let workspace = workspace.clone();
    el.on_mouse_down(MouseButton::Right, move |e, window, cx| {
        let Some(panel_id) = tab_at(&group, e.position, window, cx) else {
            return;
        };
        cx.stop_propagation();
        let at = e.position;
        let group = group.clone();
        let _ = workspace.update(cx, |ws, cx| {
            if is_terminal_bar {
                ws.open_terminal_tab_menu(panel_id, at, window, cx);
            } else {
                ws.open_right_tab_menu(panel_id, group, at, window, cx);
            }
        });
    })
}

struct OpenTabMenu {
    at: Point<Pixels>,
    menu: Entity<PopupMenu>,
    return_focus: Option<FocusHandle>,
    _dismiss: Subscription,
}

/// The workspace's open dock-tab right-click menu.
#[derive(Default)]
pub(super) struct TabContextMenu {
    open: Option<OpenTabMenu>,
}

impl TabContextMenu {
    #[cfg(all(test, unix))]
    pub(super) fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

impl Workspace {
    /// Closes any open dock-tab context menu, restoring focus if it was still inside the menu.
    pub(super) fn close_tab_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.tab_menu.open.take() else {
            return false;
        };
        if open.menu.focus_handle(cx).contains_focused(window, cx)
            && let Some(prev) = open.return_focus
        {
            prev.focus(window, cx);
        }
        cx.notify();
        true
    }

    fn show_tab_menu(
        &mut self,
        at: Point<Pixels>,
        menu: Entity<PopupMenu>,
        return_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        menu.focus_handle(cx).focus(window, cx);
        let _dismiss = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            if this.tab_menu.open.take().is_some() {
                cx.notify();
            }
        });
        self.tab_menu.open = Some(OpenTabMenu {
            at,
            menu,
            return_focus,
            _dismiss,
        });
        cx.notify();
    }

    /// Renders the open dock-tab context menu over the workspace with an occluding backdrop.
    pub(super) fn render_tab_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.tab_menu.open.as_ref()?;
        let at = open.at;
        let menu = open.menu.clone();
        Some(
            deferred(
                div()
                    .id("tab-menu-backdrop")
                    // A click outside closes the menu; named so agent-desktop can too.
                    .role(gpui::accesskit::Role::Button)
                    .aria_label("Close menu")
                    .on_a11y_action(gpui::accesskit::Action::Click, {
                        let view = cx.entity().downgrade();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.close_tab_menu(window, cx));
                        }
                    })
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab_menu(window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab_menu(window, cx);
                        }),
                    )
                    .child(
                        anchored()
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .debug_selector(|| "tab-context-menu".into())
                                    .occlude()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_mouse_down(MouseButton::Right, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(menu),
                            ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Right-click menu on a terminal tab.
    fn open_terminal_tab_menu(
        &mut self,
        panel_id: PanelId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .terminals
            .iter()
            .find(|p| PanelId::from(p.entity_id()) == panel_id)
            .cloned()
        else {
            return;
        };
        let has_others = self
            .terminals
            .iter()
            .any(|p| p != &target && p.read(cx).docked);
        let return_focus = window.focused(cx);
        let action_ctx = return_focus.clone().unwrap_or_else(|| self.focus.clone());
        let ws = cx.weak_entity();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu
                .action_context(action_ctx)
                .item(PopupMenuItem::new("Rename").on_click({
                    let (ws, target) = (ws.clone(), target.clone());
                    move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.dock.update(cx, |d, cx| {
                                d.select_panel(target.entity_id().into(), window, cx)
                            });
                            target.update(cx, |p, cx| p.start_rename(window, cx));
                        });
                    }
                }))
                .item(PopupMenuItem::new("Split right").on_click({
                    let (ws, target) = (ws.clone(), target.clone());
                    move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.dock.update(cx, |d, cx| {
                                d.select_panel(target.entity_id().into(), window, cx)
                            });
                            let active = target.read(cx).active;
                            ws.split_pane(target.clone(), active, Axis::Row, window, cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new("Split down").on_click({
                    let (ws, target) = (ws.clone(), target.clone());
                    move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.dock.update(cx, |d, cx| {
                                d.select_panel(target.entity_id().into(), window, cx)
                            });
                            let active = target.read(cx).active;
                            ws.split_pane(target.clone(), active, Axis::Column, window, cx);
                        });
                    }
                }))
                .item(PopupMenuItem::new("New terminal").on_click({
                    let ws = ws.clone();
                    move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.new_terminal(&NewTerminal, window, cx);
                        });
                    }
                }))
                .separator()
                .item(PopupMenuItem::new("Close tab").on_click({
                    let (ws, target) = (ws.clone(), target.clone());
                    move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.close_terminal_tab(&target, window, cx);
                        });
                    }
                }));
            if has_others {
                menu = menu.item(PopupMenuItem::new("Close other tabs").on_click({
                    let (ws, target) = (ws.clone(), target.clone());
                    move |_, window, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.close_other_terminal_tabs(&target, window, cx);
                        });
                    }
                }));
            }
            menu
        });
        self.show_tab_menu(at, menu, return_focus, window, cx);
    }

    fn close_other_terminal_tabs(
        &mut self,
        keep: &Entity<TerminalPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let others: Vec<Entity<TerminalPanel>> = self
            .terminals
            .iter()
            .filter(|p| *p != keep && p.read(cx).docked)
            .cloned()
            .collect();
        self.close_terminal_tabs(others, window, cx);
    }

    /// Returns the preview panel entity if `id` names it and it holds unsaved edits.
    fn unsaved_preview(&self, id: PanelId, cx: &App) -> Option<Entity<PreviewPanel>> {
        let preview = self.files.preview.as_ref()?.upgrade()?;
        (PanelId::from(preview.entity_id()) == id && preview.read(cx).unsaved()).then_some(preview)
    }

    /// Whether a right-panel tab can be closed (either directly closable, or an unsaved Preview that asks first).
    fn right_panel_can_close(
        &self,
        panel: &std::sync::Arc<dyn crate::ui::dock::BasePanelView>,
        cx: &App,
    ) -> bool {
        panel.closable(cx) || self.unsaved_preview(panel.panel_id(cx), cx).is_some()
    }

    /// Right-click menu on a right-panel dock tab.
    fn open_right_tab_menu(
        &mut self,
        panel_id: PanelId,
        group: TabGroupContext,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let can_close_self = group
            .panels()
            .iter()
            .find(|p| p.panel_id(cx) == panel_id)
            .is_some_and(|p| self.right_panel_can_close(p, cx));
        let other_ids: Vec<PanelId> = group
            .panels()
            .iter()
            .filter(|p| p.panel_id(cx) != panel_id && self.right_panel_can_close(p, cx))
            .map(|p| p.panel_id(cx))
            .collect();
        let all_ids: Vec<PanelId> = group
            .panels()
            .iter()
            .filter(|p| self.right_panel_can_close(p, cx))
            .map(|p| p.panel_id(cx))
            .collect();
        let has_others = !other_ids.is_empty();
        let has_any = !all_ids.is_empty();
        let return_focus = window.focused(cx);
        let action_ctx = return_focus.clone().unwrap_or_else(|| self.focus.clone());
        let ws = cx.weak_entity();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            menu.action_context(action_ctx)
                .item(
                    PopupMenuItem::new("Close tab")
                        .disabled(!can_close_self)
                        .on_click({
                            let (ws, group) = (ws.clone(), group.clone());
                            move |_, window, cx| {
                                let _ = ws.update(cx, |ws, cx| {
                                    ws.close_right_panels(
                                        vec![panel_id],
                                        group.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new("Close other tabs")
                        .disabled(!has_others)
                        .on_click({
                            let (ws, group, ids) = (ws.clone(), group.clone(), other_ids.clone());
                            move |_, window, cx| {
                                let _ = ws.update(cx, |ws, cx| {
                                    ws.close_right_panels(ids.clone(), group.clone(), window, cx);
                                });
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new("Close all tabs")
                        .disabled(!has_any)
                        .on_click({
                            let (ws, group, ids) = (ws.clone(), group.clone(), all_ids.clone());
                            move |_, window, cx| {
                                let _ = ws.update(cx, |ws, cx| {
                                    ws.close_right_panels(ids.clone(), group.clone(), window, cx);
                                });
                            }
                        }),
                )
        });
        self.show_tab_menu(at, menu, return_focus, window, cx);
    }

    /// Closes right-panel tabs `ids`, respecting `closable()` and prompting before closing an unsaved Preview tab.
    fn close_right_panels(
        &mut self,
        ids: Vec<PanelId>,
        group: TabGroupContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let mut immediate = Vec::new();
        let mut dirty_preview = None;
        for id in ids {
            if let Some(preview) = self.unsaved_preview(id, cx) {
                dirty_preview = Some(preview);
            } else if self.dock.read(cx).panel(id).is_some_and(|p| p.closable(cx)) {
                immediate.push(id);
            }
        }
        let dock = self.dock.clone();
        // Deferred: closing tabs emits dock events this workspace handles.
        window.defer(cx, move |window, cx| {
            if !immediate.is_empty() {
                // A locked or zoomed group refuses closes.
                dock.update(cx, |d, cx| {
                    if d.is_zoomed() {
                        d.set_zoomed_out(window, cx);
                    }
                    d.set_locked(false, window, cx);
                });
                for id in immediate {
                    group.close(id, window, cx);
                }
                dock.update(cx, |d, cx| d.set_locked(true, window, cx));
            }
            if let Some(preview) = dirty_preview {
                let preview_id: PanelId = preview.entity_id().into();
                dock.update(cx, |d, cx| {
                    if !d.is_dock_open(DockPlacement::Right) {
                        d.toggle_dock(DockPlacement::Right, window, cx);
                    }
                    d.select_panel(preview_id, window, cx);
                });
                let dock_for_close = dock.clone();
                let preview_for_close = preview.clone();
                preview.update(cx, |p, cx| {
                    p.leave(
                        Box::new(move |_, window, cx| {
                            let dock = dock_for_close.clone();
                            let panel = preview_for_close.clone();
                            // Deferred: `then` runs inside `preview.update`, and `remove_panel` reads/notifies it.
                            window.defer(cx, move |window, cx| {
                                dock.update(cx, |d, cx| d.remove_panel(panel, window, cx));
                            });
                        }),
                        window,
                        cx,
                    );
                });
            }
        });
    }
}
