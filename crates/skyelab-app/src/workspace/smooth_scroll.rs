//! An eased scroll of a `ListState` to a row, for jumps the user asked for (a minimap tick), as T3's smooth scroll.
//! A short task moves the list a step per frame and ends; nothing runs once it has arrived.

use gpui::{Context, ListOffset, ListState, Pixels, Task, px};
use std::time::Duration;

/// Frames in one jump: about 200 ms at 60 Hz.
const STEPS: u32 = 12;
const FRAME: Duration = Duration::from_millis(16);

/// Eases out: fast at first, settling on the target.
pub(super) fn ease(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

fn px_top(list: &ListState) -> Pixels {
    -list.scroll_px_offset_for_scrollbar().y
}

/// Scrolls `list` to `target` over [`STEPS`] frames. Keep the task: dropping it (a newer jump) stops this one. A wheel
/// or drag in between moves the list off the step's spot, and the animation leaves it there.
pub(super) fn scroll_to<V: 'static>(
    list: ListState,
    target: ListOffset,
    cx: &mut Context<V>,
) -> Task<()> {
    list.pause_following_tail();
    let here = list.logical_scroll_top();
    let from = px_top(&list);
    // Where the target sits in the list's own estimate, read by moving there and back before anything draws.
    list.scroll_to(target);
    let to = px_top(&list);
    list.scroll_to(here);
    cx.spawn(async move |this, cx| {
        let mut at = list.logical_scroll_top();
        for step in 1..=STEPS {
            cx.background_executor().timer(FRAME).await;
            if list.logical_scroll_top() != at {
                return; // the user scrolled meanwhile
            }
            let want = from + (to - from) * ease(step as f32 / STEPS as f32);
            let delta = want - px_top(&list);
            if delta.abs() > px(0.5) {
                list.scroll_by(delta);
            }
            at = list.logical_scroll_top();
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                return;
            }
        }
        // Rows measured on the way may have moved it: land on the row itself.
        list.scroll_to(target);
        let _ = this.update(cx, |_, cx| cx.notify());
    })
}

#[cfg(test)]
mod tests {
    use super::ease;

    #[test]
    fn ease_starts_at_zero_ends_at_one_and_never_overshoots() {
        assert_eq!(ease(0.), 0.);
        assert_eq!(ease(1.), 1.);
        assert!(ease(0.5) > 0.5, "ease-out: past halfway at half time");
        let mut last = 0.;
        for i in 0..=20 {
            let v = ease(i as f32 / 20.);
            assert!((0. ..=1.).contains(&v) && v >= last);
            last = v;
        }
    }
}
