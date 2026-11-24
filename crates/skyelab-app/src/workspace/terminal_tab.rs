//! A terminal tab's dock tab: its title (the running command or folder, or the name it was given), the inline
//! rename a double-click opens, its account and its ×.

use super::terminal_panel::{Pane, TabEvent, TerminalPanel};
use crate::ui;
use crate::ui::SkyeIcon;
use gpui::{
    App, ClickEvent, Context, EventEmitter, Focusable as _, IntoElement, KeyBinding, SharedString,
    Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _,
    dock::{BasePanel, Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Input, InputEvent, InputState},
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

    /// The tab's title: its given name, else the focused pane's.
    pub fn tab_label(&self) -> SharedString {
        self.name.clone().unwrap_or_else(|| {
            let pane = self.panes.get(self.active).or(self.panes.first());
            pane.map_or_else(|| "Terminal".into(), |p| self.pane_title(p))
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
                .child(Input::new(input).xsmall())
                .into_any_element(),
            None => div()
                .id("terminal-title")
                .max_w(px(220.))
                .truncate()
                .child(self.tab_label())
                .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                    if e.click_count() >= 2 {
                        this.start_rename(window, cx);
                    }
                }))
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

impl EventEmitter<PanelEvent> for TerminalPanel {}

impl BasePanel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "Terminal"
    }
    // The tab draws its own ×, which asks first while a shell is busy.
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for TerminalPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tab(cx)
    }
    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        None
    }
}
