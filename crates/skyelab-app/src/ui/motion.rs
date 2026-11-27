//! SkyeLab's motion, measured from Rex at 60 fps (the table in docs/gpui/animation.md). Layout that moves (a dock
//! closing, a tab bar's indicator) rides one critically damped spring; something that appears fades in; the palette,
//! tabs, selection, hover and anything that goes away change at once. GPUI has no blur, so where Rex blurs we use
//! opacity and size only. Reduce Motion ([`follow_reduce_motion`], read at launch and on every window activation) draws every end state.

use gpui::{BoxShadow, hsla, point, px};
use gpui_base::{Easing, Spring};
use std::time::Duration;

/// Rex's pane and sidebar spring: no overshoot, ω ≈ 41 rad/s (response 2π/ω). 93% of the way in 117 ms, at rest by
/// about 250 ms.
pub const SPRING: Spring = Spring::new(Duration::from_millis(153));
/// The same spring over pixels: stops within half a pixel instead of a fraction of one, so it stops asking for frames.
pub const SPRING_PX: Spring = SPRING.with_epsilon(0.5);
/// Rex's fade for something that appears (its toast, a new pane, a floating overlay).
pub const FADE: Duration = Duration::from_millis(150);
/// A small appearance (a new row, compact popover): Rex's fade there is shorter.
pub const FADE_FAST: Duration = Duration::from_millis(100);
/// A focal entrance (onboarding step change and hero settle).
pub const FOCAL_FADE: Duration = Duration::from_millis(300);
/// A leaving crossfade copy; dropped once this finishes.
pub const FADE_OUT: Duration = Duration::from_millis(150);

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

/// A floating card/toast shadow scaled by `ink` (0..=1). GPUI has no group compositing, so fading cards pass `t * t * t`
/// to prevent a half-faded card from showing its shadow through as a dark slab.
pub fn overlay_shadow(ink: f32, dark: bool) -> Vec<BoxShadow> {
    let alpha = if dark { 0.45 } else { 0.22 } * ink;
    vec![BoxShadow {
        color: hsla(0., 0., 0., alpha),
        offset: point(px(0.), px(10.)),
        blur_radius: px(24.),
        spread_radius: px(-6.),
        inset: false,
    }]
}

/// Rex's list insertion: a row that really just appeared (a new thread or message, never one scrolled into view)
/// fades in over [`FADE_FAST`] with a 4 px relative rise. Only a relative offset changes (never horizontal margin),
/// so multi-line rows never re-wrap or shift list height mid-fade. `id` must be unique to this appearance.
pub fn appear_row(row: gpui::AnyElement, id: impl Into<gpui::ElementId>) -> gpui::AnyElement {
    use gpui::{Animation, AnimationExt as _, IntoElement as _, ParentElement as _, Styled as _};
    gpui::div()
        .relative()
        .child(row)
        .with_animation(
            id,
            Animation::new(FADE_FAST).with_easing(ease_out_quad),
            |el, t| el.opacity(t).top(px(4. * (1. - t))),
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
pub(super) fn ease_out_quad_bezier() -> Easing {
    Easing::cubic_bezier(0.25, 0.46, 0.45, 0.94).expect("static ease-out quad is valid")
}

/// Last OS-level Reduce Motion reading, combined with Settings → Appearance → Motion by [`apply_reduce_motion`].
#[derive(Default)]
struct OsReduceMotion(bool);
impl gpui::Global for OsReduceMotion {}

/// Applies the effective Reduce Motion state (`Settings → Appearance → Motion == Reduced` OR the OS preference) to GPUI.
pub fn apply_reduce_motion(cx: &mut gpui::App) {
    let pref = super::themes::get(cx).motion == skyelab_core::MotionMode::Reduced;
    let os = cx.try_global::<OsReduceMotion>().is_some_and(|r| r.0);
    cx.set_reduce_motion(pref || os);
}

/// Parses `gsettings get org.gnome.desktop.interface enable-animations` output: `"false"` means Reduce Motion is on.
pub(crate) fn linux_animations_disabled(stdout: &str) -> bool {
    stdout.trim().eq_ignore_ascii_case("false")
}

/// Reads the OS Reduce Motion setting into GPUI (macOS `defaults read`, Linux `gsettings`), combined with Settings →
/// Appearance → Motion. Runs off the UI thread at launch and on every workspace window activation so OS changes apply
/// when returning from System Settings.
pub fn follow_reduce_motion(cx: &mut gpui::App) {
    // Tests set reduce_motion explicitly: the host machine's setting must not leak in on a window activation.
    if cfg!(test) {
        return;
    }
    if cfg!(target_os = "macos") {
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
            cx.update(|cx| {
                cx.set_global(OsReduceMotion(on));
                apply_reduce_motion(cx);
            });
        })
        .detach();
    } else if cfg!(target_os = "linux") {
        let read = gpui::AppContext::background_spawn(cx, async {
            let mut cmd = std::process::Command::new("gsettings");
            cmd.env_clear()
                .envs(skyelab_core::env::inherited_agent_env(false))
                .args(["get", "org.gnome.desktop.interface", "enable-animations"]);
            skyelab_core::process::output(&mut cmd, None)
                .map(|o| linux_animations_disabled(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or(false)
        });
        cx.spawn(async move |cx| {
            let on = read.await;
            cx.update(|cx| {
                cx.set_global(OsReduceMotion(on));
                apply_reduce_motion(cx);
            });
        })
        .detach();
    } else {
        apply_reduce_motion(cx);
    }
}
