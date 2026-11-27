//! F6 / Shift-F6 move focus between the window's areas (T3's keyboard-focus.md): the sidebar, the conversation,
//! the message input, the page (with no thread open), the right panel and the terminal. Hidden areas are skipped,
//! and the area F6 lands on wears a ring until the next click. Esc returns to the message input.

use super::Workspace;
use super::center::Page;
use gpui::{
    App, Context, FocusHandle, Focusable as _, Global, IntoElement, Window, actions, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    dock::{DockPlacement, PaneRef},
};

actions!(skyelab, [FocusNextArea, FocusPreviousArea, FocusComposer]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Sidebar,
    Timeline,
    Composer,
    Page,
    Right,
    Terminal,
}

/// F6's order.
pub const ORDER: [Area; 6] = [
    Area::Sidebar,
    Area::Timeline,
    Area::Composer,
    Area::Page,
    Area::Right,
    Area::Terminal,
];

/// The area F6 (or Shift-F6 with `back`) goes to from `current`, wrapping, among the `visible` ones.
pub fn step(visible: &[Area], current: Option<Area>, back: bool) -> Option<Area> {
    let shown: Vec<Area> = ORDER.into_iter().filter(|a| visible.contains(a)).collect();
    let n = shown.len();
    let at = current.and_then(|c| shown.iter().position(|a| *a == c));
    let i = match (at, back) {
        (Some(i), false) => (i + 1) % n.max(1),
        (Some(i), true) => (i + n - 1) % n,
        (None, false) => 0,
        (None, true) => n.checked_sub(1)?,
    };
    shown.get(i).copied()
}

/// The area F6 last moved to and the handle it focused there. Cleared by any click.
#[derive(Default)]
pub struct FocusRing(pub Option<(Area, FocusHandle)>);

impl Global for FocusRing {}

/// Whether the composer whose prompt has `prompt` should draw the F6 ring.
pub fn composer_ringed(prompt: &FocusHandle, window: &Window, cx: &App) -> bool {
    cx.try_global::<FocusRing>()
        .and_then(|r| r.0.as_ref())
        .is_some_and(|(area, h)| *area == Area::Composer && h == prompt && h.is_focused(window))
}

impl Workspace {
    /// The active tab's focus in a dock, and whether any of its tabs holds focus now.
    pub(super) fn dock_focus(
        &self,
        at: DockPlacement,
        window: &Window,
        cx: &App,
    ) -> Option<(FocusHandle, bool)> {
        let dock = self.dock.read(cx);
        if !dock.is_dock_open(at) {
            return None;
        }
        let (mut active, mut inside) = (None, false);
        dock.layout(at)?.root().walk(&mut |node| {
            if let PaneRef::Tabs { panels, active_ix } = node.kind() {
                for (i, id) in panels.iter().enumerate() {
                    let Some(focus) = dock.panel(*id).map(|p| p.focus_handle(cx)) else {
                        continue;
                    };
                    inside |= focus.contains_focused(window, cx);
                    if i == active_ix && active.is_none() {
                        active = Some(focus);
                    }
                }
            }
        });
        Some((active?, inside))
    }

    /// Every shown area in F6 order: the handle entering it focuses, and whether focus is in it now.
    pub(super) fn areas(&self, window: &Window, cx: &App) -> Vec<(Area, FocusHandle, bool)> {
        let mut out = Vec::new();
        let mut dock = |area, at| {
            if let Some((focus, inside)) = self.dock_focus(at, window, cx) {
                out.push((area, focus, inside));
            }
        };
        dock(Area::Sidebar, DockPlacement::Left);
        dock(Area::Right, DockPlacement::Right);
        dock(Area::Terminal, DockPlacement::Bottom);
        let thread = match &self.page {
            Page::Thread(v) => v.upgrade().map(|v| {
                let v = v.read(cx);
                (v.focus_handle(cx), v.prompt.read(cx).focus_handle(cx))
            }),
            Page::RemoteThread(v) => v.upgrade().map(|v| {
                let v = v.read(cx);
                (v.focus_handle(cx), v.prompt.read(cx).focus_handle(cx))
            }),
            _ => None,
        };
        match thread {
            Some((timeline, prompt)) => {
                let typing = prompt.contains_focused(window, cx);
                out.push((Area::Composer, prompt, typing));
                let inside = !typing && timeline.contains_focused(window, cx);
                out.push((Area::Timeline, timeline, inside));
            }
            None => {
                let page = match &self.page {
                    Page::Bots => self
                        .bots
                        .as_ref()
                        .map(|b| b.read(cx).focus_handle(cx))
                        .unwrap_or_else(|| self.center.read(cx).focus_handle(cx)),
                    Page::Github(super::gh_pages::Kind::Prs) => self
                        .gh_pages
                        .prs
                        .as_ref()
                        .map(|p| p.read(cx).focus_handle(cx))
                        .unwrap_or_else(|| self.center.read(cx).focus_handle(cx)),
                    Page::Github(super::gh_pages::Kind::Issues) => self
                        .gh_pages
                        .issues
                        .as_ref()
                        .map(|p| p.read(cx).focus_handle(cx))
                        .unwrap_or_else(|| self.center.read(cx).focus_handle(cx)),
                    _ => self.welcome.read(cx).focus_handle(cx),
                };
                let inside = self
                    .center
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx)
                    || page.contains_focused(window, cx);
                out.push((Area::Page, page, inside));
            }
        }
        out
    }

    fn step_area(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let areas = self.areas(window, cx);
        let visible: Vec<Area> = areas.iter().map(|(a, ..)| *a).collect();
        let current = areas.iter().find(|(.., inside)| *inside).map(|(a, ..)| *a);
        let Some(next) = step(&visible, current, back) else {
            return;
        };
        if let Some((_, focus, _)) = areas.into_iter().find(|(a, ..)| *a == next) {
            self.ring(next, focus, window, cx);
        }
    }

    fn ring(
        &mut self,
        area: Area,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        focus.focus(window, cx);
        cx.set_global(FocusRing(Some((area, focus))));
        self.redraw_ring(cx);
    }

    /// The thread view is a cached dock panel: it redraws its composer's ring only when told.
    fn redraw_ring(&self, cx: &mut Context<Self>) {
        if let Some(view) = self.current_thread() {
            view.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    pub(super) fn focus_next_area(
        &mut self,
        _: &FocusNextArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_area(false, window, cx);
    }

    pub(super) fn focus_previous_area(
        &mut self,
        _: &FocusPreviousArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_area(true, window, cx);
    }

    /// Esc outside a text field: back to the message input. Without one, Esc goes on to whoever else wants it.
    pub(super) fn focus_composer(
        &mut self,
        _: &FocusComposer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.details_open {
            return self.close_details(window, cx);
        }
        // The workspace's escape binding runs before the open machine menu's own key handler.
        if self.close_env_menu(window, cx) || self.close_tab_menu(window, cx) {
            return;
        }
        if let Some(view) = self.current_thread()
            && view.update(cx, |v, cx| v.close_message_menu(window, cx))
        {
            return;
        }
        if let Page::RemoteThread(v) = &self.page
            && let Some(view) = v.upgrade()
            && view.update(cx, |v, cx| v.close_message_menu(window, cx))
        {
            return;
        }
        if self.unzoom(window, cx) {
            return;
        }
        let areas = self.areas(window, cx);
        match areas.into_iter().find(|(a, ..)| *a == Area::Composer) {
            Some((_, focus, false)) => self.ring(Area::Composer, focus, window, cx),
            _ => cx.propagate(),
        }
    }

    /// Esc on an expanded (zoomed) dock group puts it back first, as T3's expanded panel does. Whether it did.
    pub(super) fn unzoom(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.dock.read(cx).is_zoomed() {
            return false;
        }
        self.dock.update(cx, |d, cx| d.set_zoomed_out(window, cx));
        true
    }

    /// A click ends keyboard navigation, so the ring goes.
    pub(super) fn clear_ring(&mut self, cx: &mut Context<Self>) {
        if cx.try_global::<FocusRing>().is_some_and(|r| r.0.is_some()) {
            cx.set_global(FocusRing(None));
            self.redraw_ring(cx);
        }
    }

    /// The ring around the area F6 moved to, drawn over the dock while focus is still in it. The composer draws its
    /// own (its card's border).
    pub(super) fn render_ring(&self, window: &Window, cx: &App) -> Option<impl IntoElement> {
        let (area, focus) = cx.try_global::<FocusRing>()?.0.as_ref()?;
        if *area == Area::Composer || !focus.contains_focused(window, cx) {
            return None;
        }
        let size = |at| self.dock_width(at, cx);
        let (left, right, bottom) = (
            size(DockPlacement::Left),
            size(DockPlacement::Right),
            // A closed bottom dock keeps its tab strip.
            match size(DockPlacement::Bottom) {
                h if h == px(0.) => gpui_component::dock::CLOSED_BOTTOM_STRIP,
                h => h,
            },
        );
        let ring = div().absolute().border_2().border_color(cx.theme().ring);
        Some(match area {
            Area::Sidebar => ring.top_0().bottom_0().left_0().w(left),
            Area::Right => ring.top_0().bottom_0().right_0().w(right),
            Area::Terminal => ring.bottom_0().left(left).right(right).h(bottom),
            _ => ring.top_0().left(left).right(right).bottom(bottom),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Area::*, *};

    #[test]
    fn f6_goes_round_the_shown_areas_in_order() {
        let shown = [Terminal, Composer, Sidebar, Timeline, Right];
        let mut at = None;
        let mut seen = Vec::new();
        for _ in 0..6 {
            at = step(&shown, at, false);
            seen.extend(at);
        }
        assert_eq!(
            seen,
            [Sidebar, Timeline, Composer, Right, Terminal, Sidebar]
        );
    }

    #[test]
    fn shift_f6_goes_back_and_hidden_areas_are_skipped() {
        let shown = [Sidebar, Page, Terminal];
        assert_eq!(step(&shown, Some(Sidebar), true), Some(Terminal));
        assert_eq!(step(&shown, Some(Terminal), true), Some(Page));
        assert_eq!(step(&shown, Some(Page), false), Some(Terminal));
        assert_eq!(
            step(&shown, None, true),
            Some(Terminal),
            "from outside, back starts at the end"
        );
        assert_eq!(
            step(&shown, Some(Right), false),
            Some(Sidebar),
            "from a hidden area, start over"
        );
        assert_eq!(step(&[], None, false), None);
        assert_eq!(step(&[Page], Some(Page), false), Some(Page));
    }
}
