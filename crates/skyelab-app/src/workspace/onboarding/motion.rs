//! Onboarding's motion, after Rex's: a step change is a crossfade in place (the new page fades in while the old one
//! fades out over the same spot, no slide), and the step's hero tile zooms in from 1.35x. Measured from Rex at 60 fps:
//! most of the change lands in the first ~120 ms, all of it by ~300 ms, on an ease-out curve. GPUI has no element blur,
//! so the hero's settle is scale and opacity only. Every animation is one-shot, so a settled page draws no frames, and
//! GPUI draws each at its end state when `App::reduce_motion` is on. The progress dot of the current step widens.

use gpui::{
    Animation, AnimationExt as _, ElementId, IntoElement, ParentElement as _, Pixels, Styled, px,
};
use std::time::Duration;

/// The new page's fade in.
const FADE_IN: Duration = Duration::from_millis(300);
/// The old page's fade out; it is dropped once this is over.
pub(super) const FADE_OUT: Duration = Duration::from_millis(150);
/// The hero tile's zoom from `ZOOM` to 1.
const HERO: Duration = Duration::from_millis(300);
const ZOOM: f32 = 1.35;

/// Rex's curve: the biggest change in the first frame, its speed then falling off evenly (an ease-out quad; a cubic
/// front-loads too much next to Rex's frames).
fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(2)
}

/// A page fading in where it stands. `id` must change with the page, so each change starts a new animation.
pub(super) fn fade_in<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
) -> impl IntoElement {
    el.with_animation(
        id,
        Animation::new(FADE_IN).with_easing(ease_out),
        |el, t| el.opacity(t),
    )
}

/// The page that was there fading out over the new one.
pub(super) fn fade_out<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
) -> impl IntoElement {
    el.with_animation(
        id,
        Animation::new(FADE_OUT).with_easing(ease_out),
        |el, t| el.opacity(1. - t),
    )
}

/// The hero tile settling from `ZOOM` times its size to `size`: `draw` builds the tile at a given edge length (its
/// glyph scales with it), inside a box that keeps `size`, so nothing around it moves. `still` (the page fading out)
/// draws it settled: that copy has a new element path, so an animation there would start the zoom over.
pub(super) fn hero<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    size: Pixels,
    still: bool,
    draw: impl Fn(Pixels) -> E + 'static,
) -> impl IntoElement {
    let tile = if still {
        draw(size).into_any_element()
    } else {
        gpui::div()
            .with_animation(
                id,
                Animation::new(HERO).with_easing(ease_out),
                move |el, t| {
                    let edge = size * (ZOOM + (1. - ZOOM) * t);
                    el.flex_shrink_0().opacity(0.4 + 0.6 * t).child(draw(edge))
                },
            )
            .into_any_element()
    };
    gpui::div()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .child(tile)
}

/// A progress dot springing from `from` to `to` px wide.
pub(super) fn dot<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
    from: f32,
    to: f32,
) -> impl IntoElement {
    // A spring, sampled into a one-shot easing: it overshoots a touch and settles, then asks for no more frames.
    let (duration, spring) = gpui::sampled_easing(gpui::SpringConfig::new(420., 26., 1.), 0.01);
    el.with_animation(
        id,
        Animation::new(duration).with_easing(spring),
        move |el, t| el.w(px(from + (to - from) * t)),
    )
}

/// macOS Reduce Motion (System Settings → Accessibility → Display) into GPUI, so animations draw their end state.
/// Read once at launch, off the UI thread; elsewhere GPUI's default (motion on) stays.
pub(crate) fn reduce_motion(cx: &mut gpui::App) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let read = gpui::AppContext::background_spawn(cx, async {
        let mut cmd = std::process::Command::new("/usr/bin/defaults");
        cmd.env_clear()
            .envs(skyelab_core::env::inherited_agent_env(false))
            .args(["read", "com.apple.universalaccess", "reduceMotion"]);
        skyelab_core::process::output(&mut cmd, None)
            .map(|o| {
                skyelab_core::onboarding::reduce_motion_on(&String::from_utf8_lossy(&o.stdout))
            })
            .unwrap_or(false)
    });
    cx.spawn(async move |cx| {
        let on = read.await;
        cx.update(|cx| cx.set_reduce_motion(on));
    })
    .detach();
}
