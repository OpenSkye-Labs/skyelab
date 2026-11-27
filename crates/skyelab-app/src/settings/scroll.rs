//! Keeps the focused control inside `settings-body` scrolled into view when keyboard focus moves (Tab / Shift-Tab),
//! without making non-interactive rows focusable or inserting extra Tab stops.

use gpui::{
    AnyWindowHandle, App, Bounds, Element, ElementId, FocusHandle, Global, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, RenderOnce, ScrollHandle, Window,
    point, px,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Vertical breathing room above and below a newly focused control so its full setting row stays visible.
const REVEAL_PADDING: Pixels = px(48.);

/// Per-App scratchpad populated during prepaint of a tracked scroll container when focus just changed.
#[derive(Default)]
struct FocusedBounds {
    window: Option<AnyWindowHandle>,
    bounds: Option<Bounds<Pixels>>,
}

impl Global for FocusedBounds {}

/// Records `bounds` as the focused control's window-coordinate rectangle if a scroll container in `window` is
/// currently collecting it on this prepaint pass.
fn record_focused_bounds(bounds: Bounds<Pixels>, window: &Window, cx: &mut App) {
    if !cx.has_global::<FocusedBounds>() {
        return;
    }
    let slot = cx.global_mut::<FocusedBounds>();
    if slot.window == Some(window.window_handle()) {
        slot.bounds = Some(bounds);
    }
}

/// Wraps `child` so that when `focused` is true its prepaint bounds are reported to any enclosing [`BodyScroll`].
/// Transparent to layout, painting, hitboxes, focus order, and accessibility.
pub(crate) fn track_focused<E: IntoElement>(
    focused: bool,
    child: E,
) -> ReportFocusedBounds<E::Element> {
    ReportFocusedBounds {
        focused,
        child: child.into_element(),
    }
}

/// Wraps a `gpui_base::Button` so it reports its bounds while focused without altering its focus handle scope,
/// Tab stop behavior, or click semantics.
pub(crate) fn track_button(id: impl Into<ElementId>, button: gpui_base::Button) -> TrackedButton {
    TrackedButton {
        id: id.into(),
        button,
    }
}

#[derive(IntoElement)]
pub(crate) struct TrackedButton {
    id: ElementId,
    button: gpui_base::Button,
}

impl RenderOnce for TrackedButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id, cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        track_focused(focused, self.button.track_focus(&focus))
    }
}

pub(crate) struct ReportFocusedBounds<E: Element> {
    focused: bool,
    child: E,
}

impl<E: Element> Element for ReportFocusedBounds<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.child.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.child.source_location()
    }

    fn a11y_role(&self) -> Option<gpui::Role> {
        self.child.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.child.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder,
    ) {
        self.child.a11y_synthetic_children(prepaint, builder);
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.child.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if self.focused {
            record_focused_bounds(bounds, window, cx);
        }
        self.child
            .prepaint(id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

/// Scroll state for `settings-body`, auto-scrolling newly focused controls into view.
#[derive(Clone, Default)]
pub(crate) struct BodyScroll {
    handle: ScrollHandle,
    last_focused: Rc<RefCell<Option<FocusHandle>>>,
}

impl BodyScroll {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn handle(&self) -> &ScrollHandle {
        &self.handle
    }

    /// Scrolls back to the top and clears the last-seen focus target (called on section switches).
    pub(crate) fn reset(&self) {
        self.handle.set_offset(Point::default());
        *self.last_focused.borrow_mut() = None;
    }

    /// Wraps `settings-body`'s content so any newly focused descendant outside the viewport scrolls into view.
    pub(crate) fn wrap<E: IntoElement>(&self, child: E) -> ScrollFocusedIntoView<E::Element> {
        ScrollFocusedIntoView {
            handle: self.handle.clone(),
            last_focused: self.last_focused.clone(),
            child: child.into_element(),
        }
    }
}

pub(crate) struct ScrollFocusedIntoView<E: Element> {
    handle: ScrollHandle,
    last_focused: Rc<RefCell<Option<FocusHandle>>>,
    child: E,
}

impl<E: Element> Element for ScrollFocusedIntoView<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.child.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.child.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.child.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let focused = window.focused(cx);
        let focus_changed = *self.last_focused.borrow() != focused;
        let collecting = focus_changed && focused.is_some();
        if collecting {
            let slot = cx.default_global::<FocusedBounds>();
            slot.window = Some(window.window_handle());
            slot.bounds = None;
        }

        let state = self
            .child
            .prepaint(id, inspector_id, bounds, request_layout, window, cx);

        if focus_changed {
            *self.last_focused.borrow_mut() = focused;
            if collecting {
                let captured = if cx.has_global::<FocusedBounds>() {
                    let slot = cx.global_mut::<FocusedBounds>();
                    slot.window = None;
                    slot.bounds.take()
                } else {
                    None
                };
                if let Some(target) = captured {
                    let viewport = self.handle.bounds();
                    let offset = self.handle.offset();
                    let max_y = self.handle.max_offset().y;
                    let new_y =
                        scroll_offset_to_reveal(viewport, target, offset.y, max_y, REVEAL_PADDING);
                    if (new_y - offset.y).abs() > px(0.5) {
                        self.handle.set_offset(point(offset.x, new_y));
                        // `window.refresh()` is a no-op mid-draw, so defer the repaint to right after this frame.
                        let win = window.window_handle();
                        cx.defer(move |cx| {
                            let _ = win.update(cx, |_, window, _| window.refresh());
                        });
                    }
                }
            }
        }
        state
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

/// Computes the vertical scroll offset (`<= 0`, clamped to `-max_y..=0`) that brings `target` inside `viewport`
/// with up to `padding` breathing room on top and bottom.
pub(super) fn scroll_offset_to_reveal(
    viewport: Bounds<Pixels>,
    target: Bounds<Pixels>,
    current_y: Pixels,
    max_y: Pixels,
    padding: Pixels,
) -> Pixels {
    if viewport.size.height <= px(0.) || max_y <= px(0.) {
        return current_y.clamp(-max_y.max(px(0.)), px(0.));
    }
    let slack = (viewport.size.height - target.size.height).max(px(0.));
    let pad = padding.min(slack / 2.);
    let min_top = viewport.top() + pad;
    let max_bottom = viewport.bottom() - pad;

    let next_y = if target.top() < min_top {
        current_y + (min_top - target.top())
    } else if target.bottom() > max_bottom {
        current_y - (target.bottom() - max_bottom)
    } else {
        current_y
    };
    next_y.clamp(-max_y, px(0.))
}

#[cfg(test)]
mod tests {
    use super::scroll_offset_to_reveal;
    use gpui::{Bounds, point, px, size};

    fn rect(top: f32, height: f32) -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(0.), px(top)), size(px(400.), px(height)))
    }

    #[test]
    fn keeps_offset_when_target_is_already_comfortably_visible() {
        let viewport = rect(50., 300.); // 50..350
        let target = rect(150., 24.);
        assert_eq!(
            scroll_offset_to_reveal(viewport, target, px(-40.), px(500.), px(48.)),
            px(-40.)
        );
    }

    #[test]
    fn scrolls_down_for_below_fold_target_with_padding_and_clamps() {
        let viewport = rect(50., 300.); // 50..350, max_bottom = 302
        let target = rect(400., 20.); // bottom = 420, needs -118
        assert_eq!(
            scroll_offset_to_reveal(viewport, target, px(0.), px(500.), px(48.)),
            px(-118.)
        );
        // Past max_y clamps cleanly to -max_y.
        let far = rect(900., 20.);
        assert_eq!(
            scroll_offset_to_reveal(viewport, far, px(0.), px(500.), px(48.)),
            px(-500.)
        );
    }

    #[test]
    fn scrolls_up_for_above_fold_target_and_clamps_at_zero() {
        let viewport = rect(50., 300.); // 50..350, min_top = 98
        let target = rect(-100., 24.); // needs +198
        assert_eq!(
            scroll_offset_to_reveal(viewport, target, px(-300.), px(500.), px(48.)),
            px(-102.)
        );
        // Near top clamps to 0.
        let near_top = rect(10., 24.);
        assert_eq!(
            scroll_offset_to_reveal(viewport, near_top, px(-40.), px(500.), px(48.)),
            px(0.)
        );
    }

    #[test]
    fn clamps_padding_in_short_viewports() {
        let viewport = rect(0., 60.);
        let target = rect(80., 40.); // slack = 20 -> pad = 10 -> max_bottom = 50, bottom = 120 -> -70
        assert_eq!(
            scroll_offset_to_reveal(viewport, target, px(0.), px(200.), px(48.)),
            px(-70.)
        );
    }
}
