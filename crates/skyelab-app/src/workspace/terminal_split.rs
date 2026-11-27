//! Drawing a terminal tab's split tree: nested rows and columns sized by their shares, with a divider between
//! neighbours that resizes them when dragged.

use super::terminal_panel::{TabEvent, TerminalPanel};
use crate::ui::ActiveTheme as _;
use gpui::{
    AnyElement, Context, DragMoveEvent, EntityId, IntoElement, Pixels, Render, Window, div,
    prelude::*, px, relative,
};
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
                let leaf = div()
                    .id(("pane", *id))
                    .relative()
                    .size_full()
                    .children(view.map(|p| p.view.clone()));
                if self.appearing != Some(*id) {
                    return leaf.into_any_element();
                }
                return appear(leaf, *id, cx.theme().background);
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
        let (split_path, count) = (path.clone(), children.len());
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
                    // Flex shares out the space left after the dividers, so measure in that space: divider d
                    // sits after d dividers and its own middle.
                    let divider = drag.divider;
                    let width = GRAB * 2. + px(1.);
                    let len = len - width * (count - 1) as f32;
                    if len <= px(0.) {
                        return;
                    }
                    let at = at - start - width * divider as f32 - width / 2.;
                    if this.tree.resize(&split_path, divider, at / len) {
                        cx.emit(TabEvent::Changed);
                        cx.notify();
                    }
                }),
            )
            .children(items)
            .into_any_element()
    }
}

/// Rex's new pane: it fades in over [`motion::FADE_FAST`] and grows from [`APPEAR_FROM`] of its slot on the spring.
/// The terminal grid never changes size mid-way: the pane has its final size from the first frame, and a frame in
/// the terminal's background, [`APPEAR_FROM`] of the slot inside it, shrinks to nothing (GPUI has no transform).
fn appear(leaf: gpui::Stateful<gpui::Div>, id: EntityId, background: gpui::Hsla) -> AnyElement {
    use crate::ui::motion;
    use gpui::{Animation, AnimationExt as _};
    let fade_share = motion::FADE_FAST.as_secs_f32() / motion::SETTLE.as_secs_f32();
    leaf.with_animation(
        ("pane-appear", id),
        Animation::new(motion::SETTLE),
        move |el, t| {
            let edge = (1. - motion::spring_ease(t)) * (1. - APPEAR_FROM) / 2.;
            let bar = || div().absolute().bg(background);
            el.opacity(motion::ease_out_quad((t / fade_share).min(1.)))
                .child(bar().top_0().left_0().w_full().h(relative(edge)))
                .child(bar().bottom_0().left_0().w_full().h(relative(edge)))
                .child(bar().top_0().left_0().h_full().w(relative(edge)))
                .child(bar().top_0().right_0().h_full().w(relative(edge)))
        },
    )
    .into_any_element()
}

/// Rex's new pane starts at about 0.88 of its slot.
const APPEAR_FROM: f32 = 0.88;

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
        let _render = crate::debug_renders::count("TerminalPanel");
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
