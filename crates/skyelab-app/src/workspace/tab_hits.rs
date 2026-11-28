//! A dock tab's title as `tab_context_menu` reads it: where each title was painted and which one a right-click hit.

use crate::ui::dock::{PanelId, TabGroupContext};
use gpui::{
    App, Bounds, Global, InteractiveElement as _, IntoElement, MouseButton, Pixels, Point, Window,
    WindowId, div, prelude::*, px,
};
use gpui_component::ElementExt as _;
use std::collections::HashMap;

/// Last painted title bounds and direct right-click hits, keyed by window and panel.
#[derive(Default)]
struct TabHits {
    bounds: HashMap<(WindowId, PanelId), Bounds<Pixels>>,
    direct: Option<(WindowId, PanelId, Point<Pixels>)>,
}

impl Global for TabHits {}

/// Wraps a dock panel's title so right-clicks on the title or its surrounding tab padding identify `panel_id`.
/// gpui-component's dock gives its tab no accessible name, so the title is a button named `name`: an accessibility
/// press lands as a click on its center, inside the tab, which selects it. `None` while the title holds a control of
/// its own (a rename field), which a button would hide.
pub(in crate::workspace) fn tab_title(
    panel_id: PanelId,
    name: Option<gpui::SharedString>,
    inner: impl IntoElement,
) -> impl IntoElement {
    div()
        .id(("tab-title", panel_id.as_u64()))
        .when_some(name, |d, name| {
            d.role(gpui::accesskit::Role::Button)
                .aria_label(name)
                .on_click(|_, _, _| {})
        })
        .h_full()
        .flex()
        .items_center()
        .child(inner)
        .on_mouse_down(MouseButton::Right, move |e, window, cx| {
            let wid = window.window_handle().window_id();
            cx.default_global::<TabHits>().direct = Some((wid, panel_id, e.position));
        })
        .on_prepaint(move |bounds, window, cx| {
            let wid = window.window_handle().window_id();
            cx.default_global::<TabHits>()
                .bounds
                .insert((wid, panel_id), bounds);
        })
}

/// Resolves which tab in `group` a right-click at `pos` landed on, if any.
pub(super) fn tab_at(
    group: &TabGroupContext,
    pos: Point<Pixels>,
    window: &Window,
    cx: &mut App,
) -> Option<PanelId> {
    let wid = window.window_handle().window_id();
    let direct = cx.default_global::<TabHits>().direct.take();
    if let Some((w, id, at)) = direct
        && w == wid
        && at == pos
        && group.panels().iter().any(|p| p.panel_id(cx) == id)
    {
        return Some(id);
    }
    let panels: Vec<(PanelId, bool)> = group
        .panels()
        .iter()
        .map(|p| (p.panel_id(cx), p.closable(cx)))
        .collect();
    let hits = cx.try_global::<TabHits>()?;
    let visible: Vec<(PanelId, Bounds<Pixels>, bool)> = panels
        .into_iter()
        .filter_map(|(id, closable)| {
            let bounds = *hits.bounds.get(&(wid, id))?;
            Some((id, bounds, closable))
        })
        .collect();
    // Each Tab in gpui-component's TabBar has 12 px horizontal padding around its title, plus ~24 px for a close ×.
    let pad_l = px(13.);
    for (i, &(id, bounds, closable)) in visible.iter().enumerate() {
        if pos.y < bounds.top() - px(6.) || pos.y > bounds.bottom() + px(6.) {
            continue;
        }
        let left = bounds.left() - pad_l;
        let right = match visible.get(i + 1) {
            Some(&(_, next, _)) => next.left() - px(12.),
            None => bounds.right() + if closable { px(38.) } else { px(13.) },
        };
        if pos.x >= left && pos.x <= right {
            return Some(id);
        }
    }
    None
}
