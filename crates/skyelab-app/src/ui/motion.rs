//! SkyeLab's motion, measured from Rex at 60 fps (the table in docs/gpui/animation.md). Layout that moves (a dock
//! closing, a tab bar's indicator) rides one critically damped spring; something that appears fades in; the palette,
//! tabs, selection, hover and anything that goes away change at once. GPUI has no blur, so where Rex blurs we use
//! opacity and size only. Reduce Motion (read at launch, `onboarding::motion::reduce_motion`) draws every end state.

use gpui_base::{Easing, Spring};
use gpui_component::dialog::DialogEntrance;
use gpui_component::theme::MotionTokens;
use std::time::Duration;

/// Rex's pane and sidebar spring: no overshoot, ω ≈ 41 rad/s (response 2π/ω). 93% of the way in 117 ms, at rest by
/// about 250 ms.
pub const SPRING: Spring = Spring::new(Duration::from_millis(153));
/// The same spring over pixels: stops within half a pixel instead of a fraction of one, so it stops asking for frames.
pub const SPRING_PX: Spring = SPRING.with_epsilon(0.5);
/// Rex's fade for something that appears (its toast, a new pane).
pub const FADE: Duration = Duration::from_millis(150);
/// A small appearance (a new row): Rex's fade there is shorter.
pub const FADE_FAST: Duration = Duration::from_millis(100);
/// A dialog fades in place over [`FADE`]; Rex has no in-window dialog that slides.
pub const DIALOG: DialogEntrance = DialogEntrance::Fade;

/// Rex's fades: speed falls off evenly from the first frame (ease-out quad).
fn ease_out_quad() -> Easing {
    Easing::cubic_bezier(0.25, 0.46, 0.45, 0.94).expect("static ease-out quad is valid")
}

/// gpui-component's motion tokens set to Rex's values, so its own pieces (dialog fades, dock tab bars, resize handles)
/// move like ours.
pub fn tokens() -> MotionTokens {
    let mut m = MotionTokens::default();
    m.duration_fast = FADE_FAST;
    m.duration_normal = FADE;
    m.duration_slow = Duration::from_millis(250);
    m.easing_enter = ease_out_quad();
    m.easing_move = ease_out_quad();
    m.spring_control = SPRING;
    m.spring_move = SPRING.with_epsilon(0.1);
    m
}
