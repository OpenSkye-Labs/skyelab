//! Scrolls a control into view when an accessibility client asks (`AXScrollToVisible`, AccessKit's `ScrollIntoView`).
//! GPUI 0.3.14 drops that request, so `agent-desktop click` refused anything scrolled out of view. Each scroll
//! container records the part of it on screen ([`A11yScrollArea::a11y_scroll_area`]) and each primitive records where
//! it was drawn; on the request the control sends one wheel event into its container, sized to bring it in, the way a
//! user would. Vertical only: a horizontal-only fix would be read as a vertical scroll by a `overflow_y_scroll` box.

use gpui::{
    Bounds, Canvas, Modifiers, ParentElement, Pixels, PlatformInput, ScrollDelta, ScrollEvent,
    ScrollSource, StatefulInteractiveElement, Styled, TouchPhase, accesskit, canvas, point, px,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

pub(crate) type Area = Rc<Cell<Option<Bounds<Pixels>>>>;
type WeakArea = Weak<Cell<Option<Bounds<Pixels>>>>;

thread_local! {
    /// Every live scroll area, by window, in draw order. An area's own a11y listeners keep it alive for the frame, so
    /// the weak entries of frames already replaced stop upgrading. (Without an accessibility client there are no
    /// listeners and no requests, so the list stays empty.)
    static AREAS: RefCell<Vec<(gpui::WindowId, WeakArea)>> = const { RefCell::new(Vec::new()) };
}

/// Room left between the control and the clip's edge after scrolling.
const MARGIN: Pixels = px(8.);

/// Where a control was last drawn and the clip it was drawn in (its own included, so only its visible part).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seen {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) clip: Bounds<Pixels>,
}

/// A scroll container whose controls can be scrolled into view. Put it right after `.overflow_*_scroll()` (the audit
/// asks for it).
pub trait A11yScrollArea: StatefulInteractiveElement + ParentElement + Sized {
    fn a11y_scroll_area(self) -> Self {
        let area: Area = Rc::default();
        let register = area.clone();
        // A page down / up from where it is on screen. These listeners also keep `area` alive for the frame.
        let page = |area: Area, sign: f32| {
            move |_: Option<&accesskit::ActionData>,
                  window: &mut gpui::Window,
                  cx: &mut gpui::App| {
                if let Some(shown) = area.get() {
                    let dy = shown.size.height * (0.8 * sign);
                    let event = wheel(shown.center(), dy);
                    window.dispatch_event(PlatformInput::Scroll(event), cx);
                }
            }
        };
        self.on_a11y_action(accesskit::Action::ScrollDown, page(area.clone(), -1.))
            .on_a11y_action(accesskit::Action::ScrollUp, page(area.clone(), 1.))
            .child(area_recorder(area, move |window| {
                let id = window.window_handle().window_id();
                AREAS.with_borrow_mut(|areas| {
                    // Pruned only when it has doubled since the last prune, so a frame stays linear.
                    if areas.len() == areas.capacity() {
                        areas.retain(|(_, w)| w.strong_count() > 0);
                        areas.reserve(areas.len().max(16));
                    }
                    areas.push((id, Rc::downgrade(&register)));
                });
            }))
    }
}

impl<E: StatefulInteractiveElement + ParentElement> A11yScrollArea for E {}

/// The child that records a scroll container's visible rect into `area`. Absolute with no insets and no size, it sits
/// at the start of the content box, so it adds no scroll range; inside the container, the content mask is the part of
/// the container on screen whatever the scroll offset.
pub(crate) fn area_recorder(
    area: Area,
    then: impl FnOnce(&mut gpui::Window) + 'static,
) -> Canvas<()> {
    canvas(
        move |_, window, _| {
            let shown = window.content_mask().bounds;
            area.set(has_room(shown).then_some(shown));
            then(window);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// Adds the recorder and the `ScrollIntoView` handler to a control.
pub(crate) fn scrolls_into_view<E: StatefulInteractiveElement + ParentElement>(el: E) -> E {
    let seen: Rc<Cell<Option<Seen>>> = Rc::default();
    let record = seen.clone();
    el.child(
        canvas(
            move |bounds, window, _| {
                let clip = window.content_mask().bounds;
                record.set(Some(Seen { bounds, clip }));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
    .on_a11y_action(accesskit::Action::ScrollIntoView, move |_, window, cx| {
        let Some(target) = seen.get() else {
            return;
        };
        let id = window.window_handle().window_id();
        let areas: Vec<Bounds<Pixels>> = AREAS.with_borrow(|areas| {
            areas
                .iter()
                .filter(|(w, _)| *w == id)
                .filter_map(|(_, a)| a.upgrade()?.get())
                .collect()
        });
        if let Some(event) = scroll_for(target, &areas) {
            window.dispatch_event(PlatformInput::Scroll(event), cx);
        }
    })
}

/// The wheel event that brings `target` into the scroll area holding it, if it isn't in view already.
pub(crate) fn scroll_for(target: Seen, areas: &[Bounds<Pixels>]) -> Option<ScrollEvent> {
    let clip = area_for(target, areas)?;
    wheel_toward(target.bounds, clip)
}

/// The scroll area holding a control: the frontmost (last drawn) one spanning the control's clip horizontally. The
/// clip is every ancestor's clip intersected, so it lies within the area's columns even when the control is scrolled
/// fully away (only its height goes to zero); a narrower box over the same column (a popover) can't hold it.
pub(crate) fn area_for(target: Seen, areas: &[Bounds<Pixels>]) -> Option<Bounds<Pixels>> {
    let (lo, hi) = (target.clip.left(), target.clip.right());
    let slack = px(0.5);
    areas
        .iter()
        .rfind(|a| a.left() <= lo + slack && hi <= a.right() + slack)
        .copied()
}

/// The vertical wheel event that brings `bounds` inside `clip`, or `None` when it already is.
pub(crate) fn wheel_toward(bounds: Bounds<Pixels>, clip: Bounds<Pixels>) -> Option<ScrollEvent> {
    if !has_room(clip) {
        return None;
    }
    let dy = shift(bounds.top(), bounds.bottom(), clip.top(), clip.bottom());
    if dy == Pixels::ZERO {
        return None;
    }
    // Inside the visible clip, so the hit test lands on the scroll container.
    let position = point(
        bounds
            .center()
            .x
            .clamp(clip.left() + px(1.), clip.right() - px(1.)),
        bounds
            .center()
            .y
            .clamp(clip.top() + px(1.), clip.bottom() - px(1.)),
    );
    Some(wheel(position, dy))
}

fn wheel(position: gpui::Point<Pixels>, dy: Pixels) -> ScrollEvent {
    ScrollEvent {
        position,
        delta: ScrollDelta::Pixels(point(Pixels::ZERO, dy)),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    }
}

/// How far content must move (positive: down) so `lo..hi` sits inside `clip_lo..clip_hi`. A control taller than the
/// clip lines up its top.
fn shift(lo: Pixels, hi: Pixels, clip_lo: Pixels, clip_hi: Pixels) -> Pixels {
    if lo < clip_lo {
        clip_lo - lo + MARGIN
    } else if hi > clip_hi {
        (clip_hi - hi - MARGIN).max(clip_lo - lo)
    } else {
        Pixels::ZERO
    }
}

fn has_room(clip: Bounds<Pixels>) -> bool {
    clip.size.width > px(2.) && clip.size.height > px(2.)
}

#[cfg(test)]
#[path = "a11y_scroll_test.rs"]
mod tests;
