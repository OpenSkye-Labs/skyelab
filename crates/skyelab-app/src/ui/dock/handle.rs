//! What a dock's resize handle and a split's divider paint: a hairline at rest, and a pill that grows and firms up
//! as the pointer hovers, presses and drags. gpui-base owns the band, the cursor and the drag.

use crate::ui::ActiveTheme as _;
use crate::ui::motion::{FADE_FAST, ease_out_quad_bezier};
use gpui::{
    AnyElement, App, Axis, IntoElement, ParentElement as _, Pixels, Styled as _, Window, deferred,
    div, prelude::FluentBuilder as _, px,
};
use gpui_base::{ResizeHandleContext, ResizeHandleState, Transition, transition};

/// How thick the pill is across the divider.
const PILL: Pixels = px(3.);

/// The pill's length and opacity at each level of engagement; at rest only the hairline shows.
fn pill(state: ResizeHandleState) -> (Pixels, f32) {
    match state {
        ResizeHandleState::Idle => (px(0.), 0.),
        ResizeHandleState::Hovered => (px(20.), 0.35),
        ResizeHandleState::Pressed => (px(28.), 0.6),
        ResizeHandleState::Dragging => (px(44.), 0.9),
    }
}

/// The hairline and its pill.
pub(super) fn render(
    handle: &ResizeHandleContext,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let axis = handle.axis();
    let (to_len, to_opacity) = pill(handle.state());
    let policy = Transition::new(FADE_FAST).easing(ease_out_quad_bezier());
    // Sampled every frame, moving or not, so the next hover starts from where the pill really is.
    let len = transition(
        "dock-handle-pill-length",
        to_len,
        policy.clone(),
        window,
        cx,
    );
    let opacity =
        transition("dock-handle-pill-opacity", to_opacity, policy, window, cx).clamp(0., 1.);
    let theme = cx.theme();
    div()
        .flex_none()
        .flex()
        .bg(theme.border)
        .map(|d| match axis {
            Axis::Horizontal => d.w(px(1.)).h_full().items_center(),
            _ => d.h(px(1.)).w_full().items_start().justify_center(),
        })
        .when(len > px(0.5), |d| {
            let pill = div()
                .flex_none()
                .rounded_full()
                .bg(theme.muted_foreground)
                .opacity(opacity)
                // Centered across the hairline by hand: flex can't center an item that overflows it.
                .map(|p| match axis {
                    Axis::Horizontal => p.w(PILL).h(len).ml((PILL - px(1.)) * -0.5),
                    _ => p.h(PILL).w(len).mt((PILL - px(1.)) * -0.5),
                });
            // A dock edge's pill overhangs the dock's clip: deferred, it keeps its outer pixel.
            d.child(match handle.edge() {
                Some(_) => deferred(pill).into_any_element(),
                None => pill.into_any_element(),
            })
        })
        .into_any_element()
}
