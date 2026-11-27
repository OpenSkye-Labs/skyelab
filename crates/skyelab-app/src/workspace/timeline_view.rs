//! A thread's timeline (its rows and the minimap) as its own view, drawn cached by the thread view: the composer's
//! caret blink and other composer-only redraws render the thread view, not the rows. It renders again when the thread
//! view notifies (an event, a streamed delta, a fold, find marks), since everything its rows show lives there, and on
//! a wheel scroll of its list (the list notifies the view that drew it), which moves the minimap's current tick.
//! The dock draws the center uncached for this to hold (`center_skin.rs`, docs/gpui/redraw.md).

use super::thread::ThreadView;
use super::thread_items::render_item;
use gpui::{
    AnyView, Context, Entity, IntoElement, Render, StyleRefinement, Subscription, WeakEntity,
    Window, div, list, prelude::*,
};

pub(in crate::workspace) struct TimelineView {
    thread: WeakEntity<ThreadView>,
    _thread_changed: Subscription,
}

impl TimelineView {
    pub(in crate::workspace) fn new(thread: &Entity<ThreadView>, cx: &mut Context<Self>) -> Self {
        Self {
            thread: thread.downgrade(),
            _thread_changed: cx.observe(thread, |_, _, cx| cx.notify()),
        }
    }
}

/// The timeline as its thread view draws it: cached, filling what is left of the timeline column.
pub(in crate::workspace) fn cached(
    view: &Entity<TimelineView>,
    window: &gpui::Window,
) -> impl IntoElement {
    crate::ui::cached_view(
        AnyView::from(view.clone()),
        StyleRefinement::default().flex_1().min_h_0().w_full(),
        window,
    )
}

impl Render for TimelineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Timeline");
        let Some(thread) = self.thread.upgrade() else {
            return div().into_any_element();
        };
        let (rows, minimap) = thread.update(cx, |v, cx| {
            let rows = v.list_in(window.window_handle()).clone();
            (rows, v.render_minimap(window, cx))
        });
        super::timeline_list::settle_rows_above(&rows);
        let thread = self.thread.clone();
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            // Rows are built on the thread view (their clicks and inputs are its); the list draws them here.
            .child(
                list(rows, move |ix, window, cx| {
                    thread
                        .update(cx, |v, cx| {
                            let row = render_item(v, ix, window, cx);
                            // Rows a live record just added fade in once (Rex's list insertion).
                            let since = v.appeared.filter(|a| ix >= a.0).map(|a| a.1);
                            let key = super::thread_items::ItemsView::items(v).key(ix);
                            let id = gpui::ElementId::NamedInteger("appear".into(), key);
                            crate::ui::motion::appear_since(row, since, id)
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .flex_1()
                .py_3(),
            )
            .children(minimap)
            .into_any_element()
    }
}
