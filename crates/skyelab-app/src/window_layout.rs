//! Where the main window opens: its last frame when that still shows on a connected display, else a large share of
//! the primary display.

use gpui::{Bounds, Pixels, point, px, size};
use skyelab_core::WindowLayout;

/// Share of the primary display's visible area a first launch takes.
const FIRST_LAUNCH_SHARE: f32 = 0.88;
/// Smallest frame the window opens at (it is clamped to the display if that is smaller).
const MIN_W: f32 = 1100.;
const MIN_H: f32 = 720.;
/// How much of a saved frame must still be on some display for it to be reused.
const MIN_VISIBLE: f32 = 120.;

/// The frame to open with. `saved` is used when at least a corner of it (`MIN_VISIBLE` square) is still on one of
/// `displays`; otherwise it is `FIRST_LAUNCH_SHARE` of `visible` (the primary display minus menu bar and dock),
/// centered, never below `MIN_W` x `MIN_H` unless the display itself is smaller.
pub fn initial_bounds(
    saved: Option<&WindowLayout>,
    displays: &[Bounds<Pixels>],
    visible: Bounds<Pixels>,
) -> Bounds<Pixels> {
    if let Some(s) = saved.filter(|s| s.width > 0 && s.height > 0) {
        let frame = Bounds::new(
            point(px(s.x as f32), px(s.y as f32)),
            size(px(s.width as f32), px(s.height as f32)),
        );
        let shows = displays.iter().any(|d| {
            let i = d.intersect(&frame);
            i.size.width >= px(MIN_VISIBLE) && i.size.height >= px(MIN_VISIBLE)
        });
        if shows {
            return frame;
        }
    }
    let fit = |share: Pixels, min: f32, max: Pixels| share.max(px(min)).min(max);
    let w = fit(
        visible.size.width * FIRST_LAUNCH_SHARE,
        MIN_W,
        visible.size.width,
    );
    let h = fit(
        visible.size.height * FIRST_LAUNCH_SHARE,
        MIN_H,
        visible.size.height,
    );
    Bounds::new(
        point(
            visible.origin.x + (visible.size.width - w) / 2.,
            visible.origin.y + (visible.size.height - h) / 2.,
        ),
        size(w, h),
    )
}

/// A window frame as saved in settings.toml (whole points).
pub fn to_layout(frame: Bounds<Pixels>, right_open: bool, right_width: Pixels) -> WindowLayout {
    let n = |p: Pixels| f32::from(p).round() as i32;
    WindowLayout {
        x: n(frame.origin.x),
        y: n(frame.origin.y),
        width: n(frame.size.width),
        height: n(frame.size.height),
        right_panel_open: right_open,
        right_panel_width: n(right_width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn first_launch_takes_most_of_the_display_centered() {
        let visible = rect(0., 25., 1728., 1092.);
        let b = initial_bounds(None, &[rect(0., 0., 1728., 1117.)], visible);
        assert_eq!(b.size.width, px(1728. * 0.88));
        assert_eq!(b.size.height, px(1092. * 0.88));
        assert_eq!(b.center(), visible.center());
    }

    #[test]
    fn a_small_display_clamps_to_it_and_a_big_one_never_goes_below_the_minimum() {
        let small = rect(0., 0., 1000., 700.);
        assert_eq!(initial_bounds(None, &[small], small), small);
        let b = initial_bounds(
            None,
            &[rect(0., 0., 1200., 800.)],
            rect(0., 0., 1200., 800.),
        );
        assert_eq!((b.size.width, b.size.height), (px(1100.), px(720.)));
    }

    #[test]
    fn a_saved_frame_is_reused_only_while_it_shows_on_a_display() {
        let main = rect(0., 0., 1728., 1117.);
        let saved = to_layout(rect(-1500., 100., 1400., 900.), true, px(480.));
        // Its display (to the left) is gone: fall back.
        assert_ne!(
            initial_bounds(Some(&saved), &[main], main).origin.x,
            px(-1500.)
        );
        // Plugged back in: reuse it exactly.
        let left = rect(-1920., 0., 1920., 1080.);
        assert_eq!(
            initial_bounds(Some(&saved), &[main, left], main),
            rect(-1500., 100., 1400., 900.)
        );
        // Only a sliver still on screen: fall back.
        let sliver = to_layout(rect(1700., 100., 1400., 900.), false, px(0.));
        assert_eq!(
            initial_bounds(Some(&sliver), &[main], main).size.width,
            px(1728. * 0.88)
        );
    }
}
