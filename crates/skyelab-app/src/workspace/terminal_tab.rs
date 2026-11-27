//! A terminal tab's dock tab: its title (the running command or folder, or the name it was given), the inline
//! rename a double-click opens, its account and its ×.

use super::terminal_panel::{Pane, TabEvent, TerminalPanel};
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use gpui::{
    App, ClickEvent, Context, EventEmitter, Focusable as _, IntoElement, KeyBinding, SharedString,
    Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _,
    dock::{BasePanel, Panel, PanelEvent},
    h_flex,
    input::{InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
};

actions!(skyelab, [CancelTerminalRename]);

const RENAME: &str = "TerminalRename";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        CancelTerminalRename,
        Some(RENAME),
    )]);
}

impl TerminalPanel {
    /// A pane's own title: what runs in front of its shell, else its folder's name.
    pub(super) fn pane_title(&self, p: &Pane) -> SharedString {
        if let Some(command) = &p.now.command {
            return command.clone().into();
        }
        p.cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.cwd.display().to_string())
            .into()
    }

    /// The tab's title: its given name, else the focused pane's, numbered after the first unnamed tab in its folder
    /// (T3's "Terminal 2"), so two shells in one folder read differently.
    pub fn tab_label(&self) -> SharedString {
        self.name.clone().unwrap_or_else(|| {
            let pane = self.panes.get(self.active).or(self.panes.first());
            let title = pane.map_or_else(|| "Terminal".into(), |p| self.pane_title(p));
            match self.number {
                0 | 1 => title,
                n => format!("{title} {n}").into(),
            }
        })
    }

    /// Opens the inline rename field over the title, holding the current title.
    pub fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.tab_label();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        let sub = cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| {
            if matches!(e, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.finish_rename(true, cx);
            }
        });
        input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
        self.renaming = Some((input, sub));
        cx.notify();
    }

    /// Ends a rename: `keep` takes the typed name (empty goes back to the automatic title), else nothing changes.
    pub fn finish_rename(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some((input, _)) = self.renaming.take() else {
            return;
        };
        if keep {
            let typed = input.read(cx).value().trim().to_owned();
            let name = (!typed.is_empty()).then(|| SharedString::from(typed));
            // Enter on the untouched automatic title keeps it automatic.
            let untouched = self.name.is_none() && name.as_ref() == Some(&self.tab_label());
            if !untouched && name != self.name {
                self.name = name;
                cx.emit(TabEvent::Changed);
            }
        }
        cx.notify();
    }

    fn cancel_rename(
        &mut self,
        _: &CancelTerminalRename,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_rename(false, cx);
        self.focus_handle(cx).focus(window, cx);
    }

    fn tab(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let account = self.panes.get(self.active).and_then(|p| p.account.as_ref());
        let theme = cx.theme();
        let label = match &self.renaming {
            Some((input, _)) => div()
                .key_context(RENAME)
                .on_action(cx.listener(Self::cancel_rename))
                .w(px(140.))
                .child(
                    TextField::new(input)
                        .xsmall()
                        .accessibility_label("Rename terminal tab"),
                )
                .into_any_element(),
            None => div()
                .id("terminal-title")
                .role(gpui::accesskit::Role::Button)
                .aria_label(format!("Rename terminal: {}", self.tab_label()))
                .debug_selector(|| "terminal-title".into())
                .max_w(px(220.))
                .truncate()
                .child(self.tab_label())
                .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                    if e.click_count() >= 2 {
                        this.start_rename(window, cx);
                    }
                }))
                // An accessibility press arrives as a single click, which the double-click rule above ignores.
                .on_a11y_action(gpui::accesskit::Action::Click, {
                    let view = cx.entity().downgrade();
                    move |_, window, cx| {
                        let _ = view.update(cx, |this, cx| this.start_rename(window, cx));
                    }
                })
                .into_any_element(),
        };
        h_flex()
            .gap_2()
            .child(label)
            .child(
                account
                    .map_or_else(ui::AccountBadge::unbound, ui::AccountBadge::of)
                    .chip(),
            )
            .child(
                div()
                    .id("close-terminal-tab")
                    .role(gpui::accesskit::Role::Button)
                    .aria_label("Close terminal tab")
                    .debug_selector(|| "close-terminal-tab".into())
                    .p_0p5()
                    .rounded(ui::RADIUS)
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.secondary_hover).text_color(theme.foreground))
                    .child(Icon::new(SkyeIcon::Close).xsmall())
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(TabEvent::Close);
                    })),
            )
    }
}

impl TerminalPanel {
    /// T3's drawer controls (`ThreadTerminalDrawer.tsx`): Split right, Split down, New terminal and Close terminal (the
    /// active pane), each naming its key. They act on this tab, wherever focus is; the tab's × closes the whole tab.
    fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use super::terminal_panel::{SplitTerminal, SplitTerminalDown};
        use skyelab_core::terminal_layout::Axis;
        let button = |id: &'static str,
                      icon: SkyeIcon,
                      tip: &'static str,
                      action: &dyn gpui::Action,
                      context: &'static str,
                      event: fn() -> TabEvent| {
            div().debug_selector(move || id.into()).child(
                ui::icon_button(id, icon)
                    .xsmall()
                    .tooltip_with_action(tip, action, Some(context))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(event()))),
            )
        };
        h_flex()
            .gap_0p5()
            .child(button(
                "terminal-split-right",
                SkyeIcon::Columns2,
                "Split right",
                &SplitTerminal,
                "Terminal",
                || TabEvent::Split(Axis::Row),
            ))
            .child(button(
                "terminal-split-down",
                SkyeIcon::Rows2,
                "Split down",
                &SplitTerminalDown,
                "Terminal",
                || TabEvent::Split(Axis::Column),
            ))
            .child(button(
                "terminal-new",
                SkyeIcon::Plus,
                "New terminal",
                &super::NewTerminal,
                super::CONTEXT,
                || TabEvent::New,
            ))
            .child(button(
                "terminal-close",
                SkyeIcon::Trash,
                "Close terminal",
                &super::terminal_panel::CloseTerminal,
                "Terminal",
                || TabEvent::CloseActive,
            ))
    }
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

impl BasePanel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "Terminal"
    }
    // The tab draws its own ×, which asks first while a shell is busy.
    fn closable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for TerminalPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        super::tab_context_menu::tab_title(cx.entity_id().into(), self.tab(cx))
    }
    fn title_suffix(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        Some(self.controls(cx))
    }
    /// The "…" menu (gpui-component always draws it, with Zoom) also holds the drawer's own controls, so it is
    /// never a menu of one item.
    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        use skyelab_core::terminal_layout::Axis;
        type Event = fn() -> TabEvent;
        let items: [(&str, Event); 4] = [
            ("Split right", || TabEvent::Split(Axis::Row)),
            ("Split down", || TabEvent::Split(Axis::Column)),
            ("New terminal", || TabEvent::New),
            ("Close terminal", || TabEvent::CloseActive),
        ];
        items.into_iter().fold(menu, |menu, (label, event)| {
            let panel = cx.weak_entity();
            menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                let _ = panel.update(cx, |_, cx| cx.emit(event()));
            }))
        })
    }
}
