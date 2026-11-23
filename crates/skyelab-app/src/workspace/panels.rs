//! The workspace's dock panels and the dock traits each one needs.

use crate::settings::Settings;
use crate::ui;
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke,
    SharedString, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{BasePanel, Panel, PanelEvent},
    h_flex,
    kbd::Kbd,
    v_flex,
};
use skyelab_browser::BrowserView;
use skyelab_terminal::TerminalView;

/// The dock traits every panel needs: its `panel_name` id, tab title, closability and title bar. `center` panels
/// carry the dock toggles at the end of their tab bar.
macro_rules! panel {
    ($ty:ty, $name:literal, closable: $closable:literal, |$this:ident| $title:expr) => {
        panel!($ty, $name, closable: $closable, title_bar: true, toggles: false, |$this| $title);
    };
    ($ty:ty, $name:literal, closable: $closable:literal, center, |$this:ident| $title:expr) => {
        panel!($ty, $name, closable: $closable, title_bar: true, toggles: true, |$this| $title);
    };
    ($ty:ty, $name:literal, closable: $closable:literal, title_bar: $title_bar:literal, toggles: $toggles:literal, $(removed: $removed:ident,)? |$this:ident| $title:expr) => {
        impl EventEmitter<PanelEvent> for $ty {}
        impl BasePanel for $ty {
            fn panel_name(&self) -> &'static str {
                $name
            }
            fn closable(&self, _: &App) -> bool {
                $closable
            }
            fn zoomable(&self, _: &App) -> bool {
                false
            }
            $(
                fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
                    self.$removed(cx);
                }
            )?
        }
        impl Panel for $ty {
            fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let $this = &*self;
                $title
            }
            fn zoom_control(&self, _: &App) -> Option<gpui_component::dock::PanelControl> {
                None
            }
            fn title_bar(&self, _: &App) -> bool {
                $title_bar
            }
            fn toolbar_buttons(
                &mut self,
                _: &mut Window,
                cx: &mut Context<Self>,
            ) -> Option<Vec<gpui_component::button::Button>> {
                $toggles.then(|| crate::workspace::layout::dock_toggles(cx))
            }
        }
    };
}

mod sidebar;
pub use sidebar::{RowAccount, Sidebar, SidebarItem, SidebarSection};

/// The center's first tab: what SkyeLab is and the keys that do things today.
pub struct Welcome {
    focus: FocusHandle,
}

impl Welcome {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

panel!(Welcome, "Welcome", closable: false, center, |_this| "Welcome");

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let shortcut = |label: &'static str, key: &'static str, action: Box<dyn Action>| {
            div()
                .id(label)
                .w_full()
                .px_2p5()
                .py_1p5()
                .rounded(ui::RADIUS)
                .cursor_pointer()
                .hover(|this| this.bg(theme.accent))
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
                .child(ui::KeyHint::new(label, key))
        };
        v_flex()
            .id("welcome")
            .track_focus(&self.focus)
            .size_full()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .child(
                v_flex()
                    .w(gpui::px(384.))
                    .gap_6()
                    .child(
                        ui::EmptyState::new("Pick a thread to continue")
                            .hint("Or start one from a project in the sidebar. Every folder runs on the account it is bound to.")
                            .page(),
                    )
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(shortcut(
                                "Toggle sidebar",
                                super::TOGGLE_SIDEBAR_KEY,
                                Box::new(super::ToggleSidebar),
                            ))
                            .child(shortcut(
                                "Toggle bottom panel",
                                super::TOGGLE_BOTTOM_KEY,
                                Box::new(super::ToggleBottomPanel),
                            ))
                            .child(shortcut(
                                "New terminal",
                                super::NEW_TERMINAL_KEY,
                                Box::new(super::NewTerminal),
                            ))
                            .child(shortcut(
                                "Command palette",
                                crate::palette::OPEN_PALETTE_KEY,
                                Box::new(crate::palette::OpenCommandPalette),
                            ))
                            .child(shortcut(
                                "Accounts and folders",
                                super::OPEN_SETTINGS_KEY,
                                Box::new(super::OpenSettings),
                            )),
                    ),
            )
    }
}

/// The bottom dock's content while no terminal is open.
pub struct TerminalsEmpty {
    focus: FocusHandle,
}

impl TerminalsEmpty {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

panel!(TerminalsEmpty, "TerminalsEmpty", closable: false, |_this| "Terminal");

impl Render for TerminalsEmpty {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .id("terminals-empty")
            .track_focus(&self.focus)
            .size_full()
            .justify_center()
            .gap_2()
            .bg(theme.background)
            .text_size(ui::BODY)
            .text_color(theme.muted_foreground)
            .child("No terminal running. Press")
            .children(Keystroke::parse(super::NEW_TERMINAL_KEY).ok().map(Kbd::new))
            .child("to open one.")
    }
}

/// One terminal tab in the bottom dock.
pub struct TerminalPanel {
    pub view: Entity<TerminalView>,
    title: SharedString,
    /// The account the shell was started under (`None`: unbound folder), shown on its tab.
    account: Option<skyelab_core::Account>,
}

impl TerminalPanel {
    pub fn new(
        view: Entity<TerminalView>,
        title: SharedString,
        account: Option<skyelab_core::Account>,
    ) -> Self {
        Self {
            view,
            title,
            account,
        }
    }
}

// shortcut: terminal tabs can't be closed yet (the workspace owns their PTYs), add closing with tab management.
panel!(TerminalPanel, "Terminal", closable: false, |this| {
    h_flex()
        .gap_2()
        .child(this.title.clone())
        .child(
            this.account
                .as_ref()
                .map_or_else(ui::AccountBadge::unbound, ui::AccountBadge::of)
                .chip(),
        )
});

impl Focusable for TerminalPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.view.read(cx).focus_handle(cx)
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.view.clone())
    }
}

panel!(Settings, "Settings", closable: true, center, |_this| "Settings");
panel!(super::thread::ThreadView, "Thread", closable: true, center, |this| this.title());
panel!(crate::remote::RemotePanel, "Remote", closable: true, center, |_this| "Remote");
panel!(super::remote_thread::RemoteThreadView, "RemoteThread", closable: true, title_bar: true, toggles: true, removed: closed, |this| this.title());

/// The right dock's content while no tool is open in it.
pub struct RightEmpty {
    focus: FocusHandle,
}

impl RightEmpty {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

panel!(RightEmpty, "RightEmpty", closable: false, |_this| "Tools");

impl Render for RightEmpty {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("right-empty")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(cx.theme().background)
            .child(
                ui::EmptyState::new("Nothing open here").hint(
                    "Changes, the browser and devices open on this side, next to the thread.",
                ),
            )
    }
}
panel!(super::changes::ChangesView, "Changes", closable: true, |this| this.title());

/// The page is a native view: it stays on screen until told otherwise (see skyelab-browser's docs).
pub struct BrowserPanel {
    pub browser: Entity<BrowserView>,
    /// The thread whose agent drives this tab. `None` for the user's tabs, and after Take over: an agent
    /// only ever reaches a tab whose owner is its own thread.
    pub agent: Option<AgentOwner>,
}

pub struct AgentOwner {
    #[cfg(unix)] // read only by `browser_agent`, which is unix-only
    pub thread_id: i64,
    pub title: SharedString,
}

/// The tab's Close button was pressed; the workspace removes it from the dock.
pub struct CloseBrowser;

impl EventEmitter<CloseBrowser> for BrowserPanel {}
impl EventEmitter<PanelEvent> for BrowserPanel {}
impl BasePanel for BrowserPanel {
    fn panel_name(&self) -> &'static str {
        "Browser"
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.browser.read(cx).set_visible(active, cx);
    }
    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |b, cx| b.close(cx));
    }
}
impl Panel for BrowserPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.agent.is_some() {
            "Agent browser"
        } else {
            "Browser"
        }
    }
    fn zoom_control(&self, _: &App) -> Option<gpui_component::dock::PanelControl> {
        None
    }
}

impl Focusable for BrowserPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.browser.read(cx).focus_handle(cx)
    }
}

impl BrowserPanel {
    /// The user takes the tab: the agent's calls fail from now on, and the reload ends the scripts it
    /// left running in the page.
    pub fn take_over(&mut self, cx: &mut Context<Self>) {
        self.agent = None;
        self.browser.update(cx, |b, cx| b.reload(cx));
        cx.notify();
    }
}

impl Render for BrowserPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // Drawn above the page: the native webview covers anything inside its own bounds.
        let bar = self.agent.as_ref().map(|a| {
            // T3's warning surface: the color tinted, ink in the foreground, a hairline under it.
            h_flex()
                .id("agent-bar")
                .h(gpui::px(36.))
                .gap_2()
                .px_3()
                .bg(theme.warning.opacity(0.14))
                .border_b_1()
                .border_color(theme.warning.opacity(0.4))
                .text_color(theme.foreground)
                .text_size(ui::BODY)
                .child(ui::StatusDot::new(theme.warning))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child("Agent is controlling this tab"),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(format!("({})", a.title)),
                        ),
                )
                .child(
                    Button::new("take-over")
                        .label("Take over")
                        .primary()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| this.take_over(cx))),
                )
                .child(
                    Button::new("close-agent-tab")
                        .label("Close")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseBrowser))),
                )
        });
        v_flex()
            .size_full()
            .children(bar)
            .child(div().flex_1().min_h_0().child(self.browser.clone()))
    }
}

macro_rules! focusable {
    ($($ty:ty),*) => {$(
        impl Focusable for $ty {
            fn focus_handle(&self, _: &App) -> FocusHandle {
                self.focus.clone()
            }
        }
    )*};
}
focusable!(Sidebar, Welcome, TerminalsEmpty, RightEmpty);
