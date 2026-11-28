//! Places a popup's `anchor` corner at `corner` (relative to where this element is laid out: right below the trigger), flipped to the other
//! side when it would leave the window and kept `margin` inside it. The final offset is rounded to whole points, as
//! gpui-base's positioner did, so popups land on the same pixels they always have.

use gpui::{
    Anchor, AnyElement, App, Axis, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, Position, Style, Window, point,
};

pub(super) struct PopupPlace {
    anchor: Anchor,
    corner: Point<Pixels>,
    margin: Pixels,
    child: AnyElement,
}

pub(super) fn popup_place(
    anchor: Anchor,
    corner: Point<Pixels>,
    margin: Pixels,
    child: impl IntoElement,
) -> PopupPlace {
    PopupPlace {
        anchor,
        corner,
        margin,
        child: child.into_any_element(),
    }
}

/// Where a popup of `size` goes: `anchor` at `at`, flipped along an axis it overflows on, then kept `margin` inside
/// `viewport`.
fn place(
    anchor: Anchor,
    at: Point<Pixels>,
    size: gpui::Size<Pixels>,
    viewport: gpui::Size<Pixels>,
    margin: Pixels,
) -> Bounds<Pixels> {
    let mut b = Bounds::from_anchor_and_size(anchor, at, size);
    let limit = viewport.height - margin;
    if b.bottom() > limit || b.top() < margin {
        let flipped =
            Bounds::from_anchor_and_size(anchor.other_side_along(Axis::Vertical), at, size);
        if flipped.top() >= margin && flipped.bottom() <= limit {
            b = flipped;
        }
    }
    let x_max = (viewport.width - margin - size.width).max(margin);
    let y_max = (viewport.height - margin - size.height).max(margin);
    b.origin = point(
        b.origin.x.clamp(margin, x_max),
        b.origin.y.clamp(margin, y_max),
    );
    b
}

impl Element for PopupPlace {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, LayoutId) {
        let child = self.child.request_layout(window, cx);
        // No insets: like gpui-base's positioner, it sits where it would flow, just below the trigger.
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, [child], cx), child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut LayoutId,
        window: &mut Window,
        cx: &mut App,
    ) {
        let size = window.layout_bounds(*child).size;
        let at = bounds.origin + self.corner;
        let placed = place(self.anchor, at, size, window.viewport_size(), self.margin);
        let offset = placed.origin - bounds.origin;
        window.with_element_offset(point(offset.x.round(), offset.y.round()), |window| {
            self.child.prepaint(window, cx)
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut LayoutId,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    #[test]
    fn a_popup_that_fits_sits_at_its_corner_and_one_that_overflows_flips_or_stays_inside() {
        let view = size(px(800.), px(600.));
        let menu = size(px(200.), px(100.));
        let at = point(px(300.), px(50.));
        assert_eq!(
            place(Anchor::TopRight, at, menu, view, px(8.)).origin,
            point(px(100.), px(50.))
        );
        let low = point(px(300.), px(560.));
        assert_eq!(
            place(Anchor::TopLeft, low, menu, view, px(8.)).origin,
            point(px(300.), px(460.)),
            "flips above when there is no room below"
        );
        let edge = point(px(750.), px(50.));
        assert_eq!(
            place(Anchor::TopLeft, edge, menu, view, px(8.)).origin.x,
            px(592.),
            "kept the margin inside the right edge"
        );
    }
}
