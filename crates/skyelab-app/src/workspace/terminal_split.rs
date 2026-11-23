//! Drawing a terminal tab's split tree: nested rows and columns sized by their shares, with a divider between
//! neighbours that resizes them when dragged.

use super::terminal_panel::{TabEvent, TerminalPanel};
use gpui::{
    AnyElement, Context, DragMoveEvent, EntityId, IntoElement, Pixels, Render, Window, div,
    prelude::*, px, relative,
};
use gpui_component::ActiveTheme as _;
use skyelab_core::terminal_layout::{Axis, PaneTree, WHOLE};

/// The divider being dragged: which tab, the split's path from the root, and the divider's index in it.
#[derive(Clone)]
pub(super) struct DividerDrag {
    tab: EntityId,
    path: Vec<usize>,
    divider: usize,
}

/// What follows the mouse while a divider is dragged: nothing (the panes themselves move).
struct NoPreview;

impl Render for NoPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The grab area on each side of a divider's 1px line.
const GRAB: Pixels = px(3.);

impl TerminalPanel {
    fn node(
        &self,
        node: &PaneTree<EntityId>,
        path: Vec<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (axis, children, sizes) = match node {
            PaneTree::Leaf(id) => {
                let view = self.panes.iter().find(|p| p.view.entity_id() == *id);
                return div()
                    .id(("pane", *id))
                    .size_full()
                    .children(view.map(|p| p.view.clone()))
                    .into_any_element();
            }
            PaneTree::Split {
                axis,
                children,
                sizes,
            } => (*axis, children, sizes),
        };
        let tab = cx.entity_id();
        let (line, background) = (cx.theme().border, cx.theme().background);
        let mut items = Vec::with_capacity(children.len() * 2);
        for (i, (child, share)) in children.iter().zip(sizes).enumerate() {
            if i > 0 {
                items.push(
                    divider(tab, axis, path.clone(), i - 1, line, background).into_any_element(),
                );
            }
            let mut path = path.clone();
            path.push(i);
            let mut cell = div().relative().overflow_hidden().min_w_0().min_h_0();
            // Shares as flex weights over a zero basis: the 1px dividers come out of the space first.
            cell.style().flex_grow = Some(*share as f32 / WHOLE as f32);
            cell.style().flex_shrink = Some(1.);
            cell.style().flex_basis = Some(relative(0.).into());
            items.push(cell.child(self.node(child, path, cx)).into_any_element());
        }
        let split_path = path.clone();
        div()
            .id(gpui::ElementId::Name(format!("split-{path:?}").into()))
            .flex()
            .map(|d| match axis {
                Axis::Row => d.flex_row(),
                Axis::Column => d.flex_col(),
            })
            .size_full()
            .on_drag_move(
                cx.listener(move |this, e: &DragMoveEvent<DividerDrag>, _, cx| {
                    let drag = e.drag(cx);
                    if drag.tab != cx.entity_id() || drag.path != split_path {
                        return;
                    }
                    let (at, start, len) = match axis {
                        Axis::Row => (e.event.position.x, e.bounds.origin.x, e.bounds.size.width),
                        Axis::Column => {
                            (e.event.position.y, e.bounds.origin.y, e.bounds.size.height)
                        }
                    };
                    if len <= px(0.) {
                        return;
                    }
                    let divider = drag.divider;
                    if this.tree.resize(&split_path, divider, (at - start) / len) {
                        cx.emit(TabEvent::Changed);
                        cx.notify();
                    }
                }),
            )
            .children(items)
            .into_any_element()
    }
}

/// A 1px line between two children, inside a strip a few pixels wide in the terminal's own background that takes
/// the drag (an overhanging grab area would sit under the next pane, which is drawn later and wins the hit test).
fn divider(
    tab: EntityId,
    axis: Axis,
    path: Vec<usize>,
    i: usize,
    line: gpui::Hsla,
    background: gpui::Hsla,
) -> impl IntoElement {
    let id = gpui::ElementId::Name(format!("divider-{path:?}-{i}").into());
    let drag = DividerDrag {
        tab,
        path,
        divider: i,
    };
    div()
        .id(id)
        .debug_selector(move || format!("terminal-divider-{i}"))
        .flex_shrink_0()
        .flex()
        .justify_center()
        .items_center()
        .bg(background)
        .map(|d| match axis {
            Axis::Row => d.w(GRAB * 2. + px(1.)).h_full().cursor_col_resize(),
            Axis::Column => d
                .h(GRAB * 2. + px(1.))
                .w_full()
                .flex_col()
                .cursor_row_resize(),
        })
        .child(div().bg(line).map(|d| match axis {
            Axis::Row => d.w(px(1.)).h_full(),
            Axis::Column => d.h(px(1.)).w_full(),
        }))
        .on_drag(drag, |_, _, _, cx| cx.new(|_| NoPreview))
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::debug_renders::count("TerminalPanel");
        if let Some(at) = self.focused_pane(window, cx) {
            self.active = at; // a click moved focus; the tab's title and account follow it
        }
        let tree = self.tree.clone();
        div()
            .size_full()
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .child(self.node(&tree, Vec::new(), cx))
    }
}
