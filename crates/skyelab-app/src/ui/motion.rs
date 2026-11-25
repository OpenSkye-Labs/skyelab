//! SkyeLab's motion, measured from Rex at 60 fps (the table in docs/gpui/animation.md). Layout that moves (a dock
//! closing, a tab bar's indicator) rides one critically damped spring; something that appears fades in; the palette,
//! tabs, selection, hover and anything that goes away change at once. GPUI has no blur, so where Rex blurs we use
//! opacity and size only. Reduce Motion ([`follow_reduce_motion`], read at launch and on every window activation) draws every end state.

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

/// Rex's fades: the biggest change in the first frame, its speed then falling off evenly (an ease-out quad; a cubic
/// front-loads too much next to Rex's frames). The one easing for `Animation::with_easing` app-wide.
pub fn ease_out_quad(t: f32) -> f32 {
    1. - (1. - t).powi(2)
}

/// How long [`SPRING`] takes to come to rest: Rex's split and sidebar settle in about this.
pub const SETTLE: Duration = Duration::from_millis(250);

/// [`SPRING`] as a one-shot easing over [`SETTLE`], for `with_animation` where nothing retargets mid-flight (a new
/// pane): a critically damped step, `1 − (1 + ωt)·e^(−ωt)` with ω = 2π / 153 ms, scaled to end exactly at 1.
pub fn spring_ease(t: f32) -> f32 {
    let x = |t: f32| {
        let wt = std::f32::consts::TAU / 0.153 * SETTLE.as_secs_f32() * t;
        1. - (1. + wt) * (-wt).exp()
    };
    x(t) / x(1.)
}

/// Rex's list insertion: a row that really just appeared (a new thread or message, never one scrolled into view)
/// fades in over [`FADE_FAST`] and grows from about 0.97 of its width. Only horizontal inset changes, so the row's
/// height, and the list around it, never move. `id` must be unique to this appearance.
pub fn appear_row(row: gpui::AnyElement, id: impl Into<gpui::ElementId>) -> gpui::AnyElement {
    use gpui::{Animation, AnimationExt as _, IntoElement as _, ParentElement as _, Styled as _};
    gpui::div()
        .child(row)
        .with_animation(
            id,
            Animation::new(FADE_FAST).with_easing(ease_out_quad),
            |el, t| el.opacity(t).mx(gpui::px(4. * (1. - t))),
        )
        .into_any_element()
}

/// [`appear_row`] while the row's appearance at `since` is recent; else the row as it is. The fade gets 3x its length
/// of room, so a late first draw still finishes it; a row scrolled into view later just shows.
pub fn appear_since(
    row: gpui::AnyElement,
    since: Option<std::time::Instant>,
    id: impl Into<gpui::ElementId>,
) -> gpui::AnyElement {
    match since {
        Some(at) if at.elapsed() < FADE_FAST * 3 => appear_row(row, id),
        _ => row,
    }
}

/// [`ease_out_quad`] as a bezier, for gpui-component's tokens.
fn ease_out_quad_bezier() -> Easing {
    Easing::cubic_bezier(0.25, 0.46, 0.45, 0.94).expect("static ease-out quad is valid")
}

/// gpui-component's motion tokens set to Rex's values, so its own pieces (dialog fades, dock tab bars, resize handles)
/// move like ours.
pub fn tokens() -> MotionTokens {
    let mut m = MotionTokens::default();
    m.duration_fast = FADE_FAST;
    m.duration_normal = FADE;
    m.duration_slow = Duration::from_millis(250);
    m.easing_enter = ease_out_quad_bezier();
    m.easing_move = ease_out_quad_bezier();
    m.spring_control = SPRING;
    m.spring_move = SPRING.with_epsilon(0.1);
    m
}

/// macOS Reduce Motion (System Settings → Accessibility → Display) into GPUI, so animations draw their end state.
/// Reads it off the UI thread; called at launch and whenever a workspace window becomes active, so a change made in
/// System Settings applies when the user comes back (GPUI has no notification for it). `set_reduce_motion` redraws
/// only when the value changed. Elsewhere GPUI's default (motion on) stays.
pub fn follow_reduce_motion(cx: &mut gpui::App) {
    // Tests set it themselves: the machine's setting must not leak in on a window activation.
    if !cfg!(target_os = "macos") || cfg!(test) {
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
