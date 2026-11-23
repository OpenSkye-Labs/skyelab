//! The workspace's dock panels and the dock traits each one needs.

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

/// The dock traits every panel needs: its `panel_name` id, tab title, closability and title bar.
macro_rules! panel {
    ($ty:ty, $name:literal, closable: $closable:literal, |$this:ident| $title:expr) => {
        panel!($ty, $name, closable: $closable, title_bar: true, |$this| $title);
    };
    ($ty:ty, $name:literal, closable: $closable:literal, title_bar: $title_bar:literal, |$this:ident| $title:expr) => {
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
        }
    };
}

// The page draws its own header, which carries the dock toggles.
panel!(super::center::Center, "Center", closable: false, title_bar: false, |_this| "Center");

mod sidebar;
mod sidebar_card;
mod sidebar_rows;
pub use sidebar::{
    ArchivedRows, RowAccount, RowCard, RowMarks, ScopeMenu, ScopeProject, Sidebar, SidebarItem,
    SidebarSearch, SidebarSection,
};

/// The page with no thread open: what SkyeLab is and its main keys, or first-run setup while `show_setup`.
pub struct Welcome {
    focus: FocusHandle,
    pub setup: Entity<super::onboarding::Onboarding>,
    pub show_setup: bool,
}

impl Welcome {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            setup: cx.new(super::onboarding::Onboarding::new),
            show_setup: false,
        }
    }
}

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        if self.show_setup {
            let header = super::layout::page_header(cx)
                .child(
                    div()
                        .flex_1()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground.opacity(0.5))
                        .child("Set up"),
                )
                .child(super::layout::panel_toggles(cx));
            return v_flex()
                .id("welcome")
                .track_focus(&self.focus)
                .size_full()
                .bg(theme.background)
                .child(header)
                .child(self.setup.clone())
                .into_any_element();
        }
        let shortcut = |label: &'static str, action: Box<dyn Action>| {
            div()
                .id(label)
                .w_full()
                .px_2p5()
                .py_1p5()
                .rounded(ui::RADIUS)
                .cursor_pointer()
                .hover(|this| this.bg(theme.accent))
                .on_click({
                    let action = action.boxed_clone();
                    move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)
                })
                .child(ui::KeyHint::new(label, action))
        };
        let welcome = ui::EmptyState::new("Pick a thread to continue")
            .hint("Or start one with + at the top of the sidebar. Every folder runs on the account it is bound to.")
            .page();
        let body = div().flex_1().flex().items_center().justify_center().child(
            v_flex()
                .w(gpui::px(384.))
                .gap_6()
                .child(
                    v_flex()
                        .gap_2()
                        .child(ui::grip_mark(gpui::px(28.), theme.foreground))
                        .child(welcome),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(shortcut("Toggle sidebar", Box::new(super::ToggleSidebar)))
                        .child(shortcut(
                            "Toggle bottom panel",
                            Box::new(super::ToggleBottomPanel),
                        ))
                        .child(shortcut("New terminal", Box::new(super::NewTerminal)))
                        .child(shortcut(
                            "Command palette",
                            Box::new(crate::palette::OpenCommandPalette),
                        ))
                        .child(shortcut(
                            "Accounts and folders",
                            Box::new(super::OpenSettings),
                        )),
                ),
        );
        let header = super::layout::page_header(cx)
            .child(
                div()
                    .flex_1()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground.opacity(0.5))
                    .child("No active thread"),
            )
            .child(super::layout::panel_toggles(cx));
        v_flex()
            .id("welcome")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(body)
            .into_any_element()
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

panel!(super::terminal_panel::TerminalPanel, "Terminal", closable: false, |this| this.tab_title());

panel!(super::changes::ChangesView, "Changes", closable: true, |this| this.title());

/// The page is a native view: it stays on screen until told otherwise (see skyelab-browser's docs).
pub struct BrowserPanel {
    pub browser: Entity<BrowserView>,
    /// The thread whose agent drives this tab. `None` for the user's tabs, and after Take over: an agent
    /// only ever reaches a tab whose owner is its own thread.
    pub agent: Option<AgentOwner>,
}

pub struct AgentOwner {
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
            // T3's warning surface: the warning color tinted under foreground text, a hairline below.
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
focusable!(Sidebar, Welcome, TerminalsEmpty);
