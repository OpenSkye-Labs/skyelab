//! Drawing the dock: the area's structure (what makes a dock a column beside the center), each group's, and the
//! renderer traits a look fills in.

use super::{DockArea, DockContext, DockPlacement, PanelId, PanelView};
use gpui::{
    AnyElement, AnyView, App, Context, Div, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, RenderOnce, Stateful, Styled as _, WeakEntity, Window,
    canvas, div, prelude::FluentBuilder as _, px,
};
use std::{rc::Rc, sync::Arc};

/// How docks look.
pub trait DockAreaRenderer: 'static {
    /// How much of a closed dock stays drawn.
    fn closed_dock_extent(&self, at: DockPlacement) -> Pixels;

    /// What goes inside a dock's box, around its group (`content`).
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;

    /// Called once per group.
    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer>;
}

/// How one group looks.
pub trait TabGroupRenderer: 'static {
    /// The group's outer box; the engine sizes it and tracks its focus.
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div>;

    /// The box under the bar; the engine sizes it.
    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div>;

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}

/// A group as its renderer draws it, and what the renderer may ask of it.
#[derive(Clone)]
pub struct TabGroupContext {
    area: WeakEntity<DockArea>,
    placement: DockPlacement,
    panels: Vec<Arc<dyn PanelView>>,
    active_ix: usize,
    zoomed: bool,
    collapsed: bool,
    closable: bool,
}

impl TabGroupContext {
    pub fn placement(&self) -> DockPlacement {
        self.placement
    }

    pub fn panels(&self) -> &[Arc<dyn PanelView>] {
        &self.panels
    }

    pub fn active_ix(&self) -> usize {
        self.active_ix
    }

    pub fn active_panel(&self) -> Option<&Arc<dyn PanelView>> {
        self.panels.get(self.active_ix)
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoomed
    }

    /// Whether its dock is closed.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// Whether the shown tab may close.
    pub fn is_closable(&self) -> bool {
        self.closable
    }

    /// Shows tab `ix` and focuses its panel.
    pub fn select_tab(&self, ix: usize, window: &mut Window, cx: &mut App) {
        _ = self
            .area
            .update(cx, |a, cx| a.select_tab(self.placement, ix, window, cx));
    }

    /// Closes `panel`'s tab, if the group and the panel allow it.
    pub fn close(&self, panel: PanelId, window: &mut Window, cx: &mut App) {
        _ = self
            .area
            .update(cx, |a, cx| a.close(self.placement, panel, window, cx));
    }

    pub fn toggle_zoom(&self, window: &mut Window, cx: &mut App) {
        _ = self
            .area
            .update(cx, |a, cx| a.toggle_zoom(self.placement, window, cx));
    }
}

/// One group, drawn after `DockArea::render` returns, so a title or a toolbar may read the dock.
#[derive(IntoElement)]
struct GroupElement {
    context: TabGroupContext,
    renderer: Rc<dyn TabGroupRenderer>,
    focus: FocusHandle,
}

impl RenderOnce for GroupElement {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            context,
            renderer,
            focus,
        } = self;
        let active = context.active_panel().map(|p| p.view());
        renderer
            .frame(&context, window, cx)
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .track_focus(&focus)
            .tab_group()
            .child(renderer.render_tab_bar(&context, window, cx))
            .child(
                renderer
                    .content_frame(&context, window, cx)
                    .flex()
                    .flex_col()
                    .when(!context.collapsed, |d| d.flex_1())
                    // Or its content's wanted height floors it, and a virtualized list builds every row.
                    .min_h(px(0.))
                    .overflow_hidden()
                    .children(
                        active
                            .map(|panel| renderer.render_active_panel(panel, &context, window, cx)),
                    ),
            )
    }
}

impl DockArea {
    fn render_group(&self, at: DockPlacement, cx: &Context<Self>) -> AnyElement {
        let group = self.group(at);
        let active = group.active_panel();
        GroupElement {
            focus: active.map_or_else(|| group.focus.clone(), |p| p.focus_handle(cx)),
            renderer: group.renderer.clone(),
            context: TabGroupContext {
                area: cx.weak_entity(),
                placement: at,
                panels: group.panels.clone(),
                active_ix: group.active_ix,
                zoomed: self.zoomed == Some(at),
                collapsed: self.is_collapsed(at),
                closable: self.may_close(at) && active.is_some_and(|p| p.closable(cx)),
            },
        }
        .into_any_element()
    }

    fn render_dock(
        &self,
        at: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dock = self.dock_context(at, cx)?;
        let size = if dock.is_open() {
            dock.size()
        } else {
            self.renderer.closed_dock_extent(at)
        };
        if size <= px(0.) {
            return None;
        }
        let content = self.render_group(at, cx);
        let chrome = self.renderer.render_dock(&dock, content, window, cx);
        Some(
            div()
                .flex()
                .flex_none()
                .relative()
                .overflow_hidden()
                .map(|d| match at {
                    DockPlacement::Bottom => d.w_full().h(size),
                    _ => d.flex_row().h_full().w(size),
                })
                .child(chrome)
                .into_any_element(),
        )
    }
}

impl Render for DockArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area = cx.entity();
        div()
            .id(self.id.clone())
            .relative()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_row()
            .child(
                canvas(
                    move |bounds, _, cx| area.update(cx, |area, _| area.bounds = bounds),
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .track_focus(&self.focus)
            .map(|frame| match self.zoomed {
                Some(at) => frame.child(self.render_group(at, cx)),
                None => frame
                    .children(self.render_dock(DockPlacement::Left, window, cx))
                    .child(
                        div()
                            .id("dock-area-center")
                            .flex()
                            .flex_1()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .size_full()
                                    .min_h(px(0.))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_1()
                                            .size_full()
                                            .min_w(super::PANEL_MIN_SIZE)
                                            .child(self.render_group(DockPlacement::Center, cx)),
                                    ),
                            )
                            .children(self.render_dock(DockPlacement::Bottom, window, cx)),
                    )
                    .children(self.render_dock(DockPlacement::Right, window, cx)),
            })
    }
}
