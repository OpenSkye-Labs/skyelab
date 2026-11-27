//! gpui-component's dock skin, except that the center panel and side chat panels are drawn uncached. The skin draws
//! every panel as a cached view, and a cached view that misses its cache builds its whole subtree again (GPUI sets
//! `window.refreshing`). The composer's text area is a view, so each caret blink dirtied its panel and rebuilt every
//! timeline row. Uncached, these panels cache what they need themselves: a thread page's timeline is a cached view
//! (`timeline_view.rs`), and the center's other pages are drawn cached by `Center` (docs/gpui/redraw.md).
//!
//! A dock that closes (the sidebar, the tools dock, the terminal) collapses on Rex's spring (`ui::motion::SPRING`)
//! instead of vanishing: the toggle calls [`CenterSkin::start_close`], base asks
//! [`DockAreaRenderer::closed_dock_extent`] how much of a closed dock to draw, and this skin answers with the
//! spring's last value until it settles. A drag that closes a dock stays with the pointer. Opening stays instant, as Rex's lands most of
//! the way on its first frame.
//!
//! The sidebar's edge also hides it (T3's rail): base clamps the drag at the minimum width, and past
//! [`HIDE_BELOW`] the sidebar fades; releasing there hides it through the toggle (`Workspace::hide_sidebar_from_drag`).

use crate::ui::A11yScrollArea as _;
use gpui::{
    AnyElement, AnyView, App, Axis, Bounds, Div, EntityId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Stateful, WeakEntity, Window, canvas, div, prelude::*,
};
use gpui_base::ResizeHandleContext;
use gpui_base::dock::PanelView;
use gpui_component::dock::{
    DockAreaRenderer, DockContext, DockPlacement, DockSkin, DropIndicator, NodeId, PanelState,
    TabGroupContext, TabGroupRenderer,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// T3 hides its sidebar once the pointer is under a quarter of the minimum width (`SIDEBAR_COLLAPSE_DRAG_RATIO`).
pub(super) const HIDE_BELOW: Pixels = gpui::px(25.); // a quarter of gpui-base's PANEL_MIN_SIZE (100 px)
/// The dock's resize handle: its hit area hugs the trailing edge this deep (gpui-base's `HANDLE_SIZE` + padding).
const HANDLE_W: Pixels = gpui::px(5.);

/// A drag of the sidebar's edge: the width it had when pressed, and whether a release now hides it.
#[derive(Clone, Copy, Default)]
pub(super) struct SidebarDrag {
    pub start: Option<Pixels>,
    pub hide: bool,
}

/// [`DockSkin`] with the panel `center` and every side chat panel drawn uncached.
pub(super) struct CenterSkin {
    pub skin: Rc<DockSkin>,
    pub center: EntityId,
    /// Left, right, bottom: the extent each dock was last drawn at, and its size when last open.
    pub drawn: [Cell<Drawn>; 3],
    pub sidebar_drag: Rc<Cell<SidebarDrag>>,
    workspace: WeakEntity<super::Workspace>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Drawn {
    /// Only while the dock closes: an open dock reports base's own closed extent, which base also reads as the
    /// bottom dock's strip during a resize drag.
    extent: Option<Pixels>,
    open: Pixels,
}

/// The spring's key: one per dock.
fn extent_id(at: DockPlacement) -> gpui::ElementId {
    gpui::ElementId::NamedInteger("dock-extent".into(), at as u64)
}

impl CenterSkin {
    pub fn new(
        skin: Rc<DockSkin>,
        center: EntityId,
        workspace: WeakEntity<super::Workspace>,
    ) -> Self {
        Self {
            skin,
            center,
            drawn: Default::default(),
            sidebar_drag: Default::default(),
            workspace,
        }
    }

    /// The open sidebar, watching its edge drag: window listeners (capture phase, so the handle's own
    /// `stop_propagation` can't hide them) arm on a press on the handle and hide on a release past [`HIDE_BELOW`]
    /// that ends a drag (GPUI clears the active drag after the release's listeners).
    fn track_sidebar_drag(&self, width: Pixels, content: AnyElement) -> AnyElement {
        let drag = self.sidebar_drag.clone();
        let workspace = self.workspace.clone();
        let hide = drag.get().hide;
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .size_full()
                    .when(hide, |d| d.opacity(0.35))
                    .child(content),
            )
            .child(
                canvas(
                    // Over base's handle, not blocking it: a press there counts only when nothing (a dialog, a
                    // popover) covers the handle.
                    |bounds: Bounds<Pixels>, window, _| {
                        let strip = Bounds::from_corners(
                            gpui::point(bounds.right() - HANDLE_W, bounds.top()),
                            bounds.bottom_right(),
                        );
                        window.insert_hitbox(strip, gpui::HitboxBehavior::Normal)
                    },
                    move |bounds: Bounds<Pixels>, handle, window, _| {
                        let d = drag.clone();
                        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, _| {
                            let on_handle = handle.is_hovered_at(e.position, window);
                            if phase.capture() && e.button == MouseButton::Left && on_handle {
                                d.set(SidebarDrag {
                                    start: Some(width),
                                    hide: false,
                                });
                            }
                        });
                        let d = drag.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                            let mut now = d.get();
                            let pressed = e.pressed_button == Some(MouseButton::Left);
                            if !phase.capture() || now.start.is_none() || !pressed {
                                return;
                            }
                            let hide = e.position.x - bounds.left() < HIDE_BELOW;
                            if hide != now.hide {
                                now.hide = hide;
                                d.set(now);
                                window.refresh();
                            }
                        });
                        let d = drag;
                        let workspace = workspace.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                            if !phase.capture() {
                                return;
                            }
                            let SidebarDrag { start, hide } = d.take();
                            // Only a real drag of the handle: a press with no drag (under a dialog, say) never hides.
                            if let (Some(width), true, true) = (start, hide, cx.has_active_drag()) {
                                let workspace = workspace.clone();
                                // After base's own release handler, which settles the drag.
                                window.defer(cx, move |window, cx| {
                                    _ = workspace.update(cx, |ws, cx| {
                                        ws.hide_sidebar_from_drag(width, window, cx)
                                    });
                                });
                            }
                        });
                    },
                )
                // `inset_0`: an absolute box with no offsets keeps its place in the flow, below the content.
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }

    /// A toggle is closing the dock at `at`: collapse it from its open size, unless motion is reduced.
    pub fn start_close(&self, at: DockPlacement, cx: &App) {
        if let Some(cell) = self.drawn(at) {
            let mut drawn = cell.get();
            if !cx.reduce_motion() && drawn.open > Pixels::ZERO {
                drawn.extent = Some(drawn.open);
                cell.set(drawn);
            }
        }
    }

    fn drawn(&self, at: DockPlacement) -> Option<&Cell<Drawn>> {
        match at {
            DockPlacement::Left => Some(&self.drawn[0]),
            DockPlacement::Right => Some(&self.drawn[1]),
            DockPlacement::Bottom => Some(&self.drawn[2]),
            DockPlacement::Center => None,
        }
    }

    /// A closing dock: its content held at the size it had open, pinned to the edge beside the center and clipped
    /// by base's frame, so it slides out without laying out (or resizing a terminal) on every frame.
    fn render_closing(
        &self,
        at: DockPlacement,
        cell: &Cell<Drawn>,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mut drawn = cell.get();
        let closed = self.skin.closed_dock_extent(at);
        let now = gpui_base::spring(
            extent_id(at),
            closed,
            crate::ui::motion::SPRING_PX,
            window,
            cx,
        );
        drawn.extent = (now > closed).then_some(now);
        if drawn.extent.is_none() {
            // Base sized this frame from the last value; one more frame draws the closed extent.
            window.request_animation_frame();
        }
        cell.set(drawn);
        let open = drawn.open;
        div()
            .debug_selector(move || format!("dock-closing-{}", at as u8))
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .map(|d| match at {
                        DockPlacement::Left => d.top_0().bottom_0().right_0().w(open),
                        DockPlacement::Right => d.top_0().bottom_0().left_0().w(open),
                        _ => d.left_0().right_0().top_0().h(open),
                    })
                    .child(content),
            )
            .into_any_element()
    }
}

impl DockAreaRenderer for CenterSkin {
    fn frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.skin.frame(window, cx)
    }
    fn split_frame(
        &self,
        node: NodeId,
        axis: Axis,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.skin.split_frame(node, axis, window, cx)
    }
    fn center_frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.skin.center_frame(window, cx)
    }
    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.skin.render_split_handle(handle, window, cx)
    }
    fn closed_dock_extent(&self, placement: DockPlacement) -> Pixels {
        let closed = self.skin.closed_dock_extent(placement);
        self.drawn(placement)
            .and_then(|d| d.get().extent)
            .map_or(closed, |e| e.max(closed))
    }
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let at = dock.placement();
        let Some(cell) = self.drawn(at) else {
            return self.skin.render_dock(dock, content, window, cx);
        };
        if dock.is_open() {
            let size = dock.size();
            // The spring sits on the open size (no travel while open, so a resize drag never lags), and a close
            // starts from it.
            gpui_base::spring(
                extent_id(at),
                size,
                crate::ui::motion::SPRING_PX.with_travel(false),
                window,
                cx,
            );
            cell.set(Drawn {
                extent: None,
                open: size,
            });
            let docked = self.skin.render_dock(dock, content, window, cx);
            if at == DockPlacement::Left {
                return self.track_sidebar_drag(size, docked);
            }
            return docked;
        }
        if cell.get().extent.is_some() {
            return self.render_closing(at, cell, content, window, cx);
        }
        self.skin.render_dock(dock, content, window, cx)
    }
    fn build_placeholder(
        &self,
        state: &PanelState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<dyn PanelView>> {
        self.skin.build_placeholder(state, window, cx)
    }
    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(CenterTabs {
            tabs: self.skin.tab_group_renderer(),
            center: self.center,
        })
    }
}

struct CenterTabs {
    tabs: Rc<dyn TabGroupRenderer>,
    center: EntityId,
}

impl TabGroupRenderer for CenterTabs {
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.tabs.frame(group, window, cx)
    }
    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.tabs.content_frame(group, window, cx)
    }
    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // The hidden terminal drawer keeps a strip above the status bar: draw it as the button it is (T3 shows the
        // drawer's toggle, not a bare tab), opening the drawer on click.
        if group.is_collapsed()
            && group
                .panels()
                .iter()
                .any(|p| matches!(p.panel_name(cx), "Terminal" | "TerminalsEmpty"))
        {
            return collapsed_drawer(window, cx);
        }
        let bar = self.tabs.render_tab_bar(group, window, cx);
        // Windows: the right dock's tab bar is the top row's end there, so it leaves the caption buttons room. Only the
        // right dock holds the Surfaces panel; a zoomed group fills the window, so it reaches them too.
        let bar = if crate::ui::window_controls::drawn()
            && (group.is_zoomed()
                || group
                    .panels()
                    .iter()
                    .any(|p| p.panel_name(cx) == "Surfaces"))
        {
            div()
                .w_full()
                .pr(crate::ui::window_controls::WINDOW_CONTROLS_W)
                .bg(gpui_component::ActiveTheme::theme(cx).tokens.tab_bar)
                .child(bar)
                .into_any_element()
        } else {
            bar
        };
        // macOS: a zoomed group fills the window, so its bar starts under the traffic lights: leave them room.
        let bar = if super::MAC && group.is_zoomed() {
            div()
                .w_full()
                .flex()
                .pl(crate::ui::MAC_TOP_PAD + gpui::px(8.))
                .bg(gpui_component::ActiveTheme::theme(cx).tokens.tab_bar)
                .debug_selector(|| "zoomed-tab-bar".into())
                .child(div().flex_1().min_w_0().child(bar))
                .into_any_element()
        } else {
            bar
        };
        let Some(active) = group
            .active_panel()
            .cloned()
            .filter(|_| !group.is_collapsed())
        else {
            return bar;
        };
        // A press on a tab focuses the tab, and gpui-base's `select_tab` moves focus into the panel only when the tab
        // changes: a click on the tab already shown left the keys (mod+F, Esc) outside the panel. The bar's toolbar
        // occludes, so its menu keeps the focus it takes.
        let panels = group.panels().to_vec();
        div()
            .w_full()
            .flex_shrink_0()
            .child(bar)
            .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                let held = panels.iter().any(|p| {
                    let h = p.focus_handle(cx);
                    h.is_focused(window) || h.contains_focused(window, cx)
                });
                if !held {
                    active.focus_handle(cx).focus(window, cx);
                }
            })
            .into_any_element()
    }
    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // A side chat is a thread page too: its composer blinks under the right dock's panel.
        let thread_page = panel.entity_id() == self.center
            || panel
                .clone()
                .downcast::<super::side_chat::SideChatPanel>()
                .is_ok();
        if !thread_page || group.is_collapsed() {
            return self.tabs.render_active_panel(panel, group, window, cx);
        }
        // The skin's box (`render_active_panel` in gpui-component's tab_panel.rs) around the view, not cached.
        div()
            .id("tab-content")
            .overflow_y_scroll()
            .a11y_scroll_area()
            .overflow_x_hidden()
            .flex_1()
            .child(div().absolute().size_full().child(panel))
            .into_any_element()
    }
    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.tabs.render_drop_indicator(indicator, window, cx)
    }
    fn render_empty(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.tabs.render_empty(group, window, cx)
    }
}

/// The hidden terminal drawer's strip: an icon, "Terminal", its key and a chevron, all one button.
fn collapsed_drawer(window: &Window, cx: &App) -> AnyElement {
    use crate::ui::{self, SkyeIcon};
    use gpui_component::{ActiveTheme as _, Icon, h_flex};
    let theme = cx.theme();
    h_flex()
        .id("drawer-collapsed")
        .role(gpui::accesskit::Role::Button)
        .aria_label("Show terminal")
        .aria_expanded(false)
        .debug_selector(|| "drawer-collapsed".into())
        .w_full()
        .h(gpui::px(30.))
        .flex_shrink_0()
        .px_3()
        .gap_2()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.tokens.tab_bar)
        .text_size(ui::META)
        .text_color(theme.muted_foreground)
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent).text_color(theme.foreground))
        .child(Icon::new(SkyeIcon::SquareTerminal).size_3p5())
        .child("Terminal")
        .children(ui::Kbd::for_action(
            &super::ToggleBottomPanel,
            Some(super::CONTEXT),
            window,
        ))
        .child(div().flex_1())
        .child(Icon::new(SkyeIcon::ChevronUp).size_3p5())
        .tooltip(ui::Tooltip::text("Show terminal"))
        .on_click(|_, window, cx| {
            crate::workspace::dispatch(Box::new(super::ToggleBottomPanel), window, cx)
        })
        .into_any_element()
}
