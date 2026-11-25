//! A thread row's settle and snooze hover buttons, and the snooze menu the sidebar draws for them above its list.

use super::sidebar::{RowShelf, Sidebar};
use super::*;
use crate::ui::SkyeIcon;
use skyelab_core::snooze;

/// The open snooze menu (its thread, where it opened, the menu), and the sidebar that owns it.
#[derive(Clone)]
pub(in crate::workspace) struct SnoozeMenu {
    pub open: Option<(
        i64,
        gpui::Point<gpui::Pixels>,
        Entity<gpui_component::menu::PopupMenu>,
    )>,
    pub sidebar: gpui::WeakEntity<Sidebar>,
    /// The sidebar's focus: the action context of its rows' menus.
    pub focus: FocusHandle,
}

impl SnoozeMenu {
    pub(super) fn new(focus: &FocusHandle, cx: &mut Context<Sidebar>) -> Self {
        let sidebar = cx.entity().downgrade();
        Self {
            open: None,
            sidebar,
            focus: focus.clone(),
        }
    }

    /// The open menu, above everything at the point it opened.
    pub(super) fn render(&self) -> Option<impl IntoElement + use<>> {
        let (_, at, menu) = self.open.as_ref()?;
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(*at)
                    .snap_to_window_with_margin(gpui::px(8.))
                    .child(div().occlude().child(menu.clone())),
            )
            .with_priority(1),
        )
    }
}

impl Sidebar {
    /// Opens thread `id`'s snooze menu at `at` (window coordinates), in the sidebar's action context so a pick
    /// reaches the workspace whatever held the focus.
    pub(in crate::workspace) fn open_snooze(
        &mut self,
        id: i64,
        host: Option<SharedString>,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.focus.clone();
        let menu = gpui_component::menu::PopupMenu::build(window, cx, move |menu, _, _| {
            super::super::thread_list::snooze_menu(
                menu.action_context(focus.clone()),
                id,
                host.clone(),
            )
        });
        menu.focus_handle(cx).focus(window, cx);
        cx.subscribe(&menu, |this, _, _: &gpui::DismissEvent, cx| {
            this.snooze.open = None;
            cx.notify();
        })
        .detach(); // lives as long as the menu, which the next open or a dismiss drops
        self.snooze.open = Some((id, at, menu));
        cx.notify();
    }
}

/// T3's settle and snooze buttons, shown over the status or time while the row is hovered.
pub(super) fn hover_actions(
    s: RowShelf,
    snooze: &SnoozeMenu,
    now: i64,
    theme: &gpui_component::Theme,
) -> gpui::Stateful<gpui::Div> {
    use super::super::thread_list::{Shelf, ShelveThread};
    let (id, host) = (s.id, s.host.clone());
    let menu_host = host.clone();
    let button = |name: &str, icon: SkyeIcon, tip: &'static str| {
        Button::new(SharedString::from(format!("{name}-{id}")))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon).size_3())
            .tooltip(tip)
    };
    let act = |to: Shelf| {
        let host = host.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            let host = host.clone();
            window.dispatch_action(Box::new(ShelveThread { id, to, host }), cx)
        }
    };
    // gpui-component's Button lets a click bubble on to the row, which would open the thread too.
    let row = h_flex()
        .id(SharedString::from(format!("shelf-actions-{id}")))
        .gap_0p5()
        .text_color(theme.muted_foreground)
        .on_click(|_, _, cx| cx.stop_propagation());
    if s.settled {
        return row.child(
            button("unsettle", SkyeIcon::Undo2, "Un-settle").on_click(act(Shelf::Unsettle)),
        );
    }
    if snooze::is_asleep(s.snoozed_until, now) {
        return row
            .child(button("wake", SkyeIcon::AlarmClockOff, "Wake now").on_click(act(Shelf::Wake)));
    }
    let sidebar = snooze.sidebar.clone();
    row.child(div().debug_selector(move || format!("snooze-{id}")).child(
        button("snooze", SkyeIcon::AlarmClock, "Snooze").on_click(move |_, window, cx| {
            // Just under the pointer, as a dropdown from the button would sit.
            let at = window.mouse_position() + gpui::point(gpui::px(-8.), gpui::px(12.));
            let host = menu_host.clone();
            let _ = sidebar.update(cx, |s, cx| s.open_snooze(id, host, at, window, cx));
        }),
    ))
    .child(button("settle", SkyeIcon::Check, "Settle").on_click(act(Shelf::Settle)))
}
