//! Onboarding's motion: a page slides in from the side it came from and fades up, its
//! rows enter one after another, and the progress dot of the current step widens. Every animation is one-shot, so
//! a settled page draws no frames, and GPUI draws each at its end state when `App::reduce_motion` is on.

use gpui::{Animation, AnimationExt as _, ElementId, IntoElement, Styled, ease_out_quint, px};
use std::time::Duration;

/// A page's slide and fade.
pub(super) const PAGE: Duration = Duration::from_millis(220);
/// Each row starts this much after the one above it.
const STAGGER_MS: u64 = 35;
/// One row's own entrance.
const ROW_MS: u64 = 200;
/// The most rows that wait their turn; later ones enter with the last of them.
const MAX_STAGGER: usize = 8;
/// How far a page slides, in px.
const SLIDE: f32 = 28.;

/// A page entering from the right (`forward`) or the left. `id` must change with the page, so each change starts a
/// new animation.
pub(super) fn page<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
    forward: bool,
) -> impl IntoElement {
    let from = if forward { SLIDE } else { -SLIDE };
    el.with_animation(
        id,
        Animation::new(PAGE).with_easing(ease_out_quint()),
        move |el, t| el.opacity(t).left(px(from * (1. - t))),
    )
}

/// Row `i` of a list fading up after the rows above it.
pub(super) fn row<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
    i: usize,
) -> impl IntoElement {
    let delay = (i.min(MAX_STAGGER) as u64) * STAGGER_MS;
    let total = delay + ROW_MS;
    let start = delay as f32 / total as f32;
    let ease = ease_out_quint();
    el.with_animation(
        id,
        Animation::new(Duration::from_millis(total))
            .with_easing(move |t| ease(((t - start) / (1. - start)).clamp(0., 1.))),
        |el, t| el.opacity(t).top(px(6. * (1. - t))),
    )
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
