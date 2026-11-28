//! The workspace's dock panels and the dock traits each one needs.

use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::{Icon, h_flex, v_flex};
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, SharedString,
    Window, div, prelude::*,
};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};
use skyelab_browser::BrowserView;

/// The dock traits every panel needs: its `panel_name` id, tab title, closability and title bar.
macro_rules! panel {
    ($ty:ty, $name:literal, closable: $closable:literal, |$this:ident| $title:expr) => {
        panel!($ty, $name, closable: $closable, title_bar: true, |$this| $title);
    };
    ($ty:ty, $name:literal, closable: $closable:literal, title_bar: $title_bar:literal, |$this:ident| $title:expr) => {
        panel!($ty, $name, closable: $closable, title_bar: $title_bar, removed: |_this, _window, _cx| {}, |$this| $title);
    };
    ($ty:ty, $name:literal, closable: $closable:literal, title_bar: $title_bar:literal, removed: |$rthis:ident, $rwindow:ident, $rcx:ident| $removed:block, |$this:ident| $title:expr) => {
        impl EventEmitter<PanelEvent> for $ty {}
        impl BasePanel for $ty {
            fn panel_name(&self) -> &'static str {
                $name
            }
            fn closable(&self, _: &App) -> bool {
                $closable
            }
            fn on_removed(&mut self, $rwindow: &mut Window, $rcx: &mut Context<Self>) {
                let $rthis = &*self;
                $removed
            }
        }
        impl Panel for $ty {
            fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let $this = &*self;
                let title: SharedString = ($title).into();
                crate::workspace::tab_context_menu::tab_title(cx.entity_id().into(), Some(title.clone()), title)
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
mod sidebar_a11y;
mod sidebar_card;
mod sidebar_rows;
mod sidebar_snooze;
mod sidebar_undo;
mod sidebar_views;
#[cfg(test)]
pub(crate) use sidebar::FOOTER_BUTTON_IDS;
pub use sidebar::{
    ArchivedRows, Ask, RowAccount, RowCard, RowMarks, RowShelf, ScopeMenu, ScopeProject, Shelf,
    Sidebar, SidebarItem, SidebarSearch, SidebarSection,
};
pub use sidebar_card::{ThreadCards, with_card_at};

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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Welcome");
        // Onboarding held the focus until it closed: take it back so the keys work.
        let stale = window
            .focused(cx)
            .is_none_or(|f| f == self.setup.read(cx).focus);
        if !self.show_setup && stale {
            window.focus(&self.focus, cx);
        }
        let theme = cx.theme();
        if self.show_setup {
            let header = super::layout::page_header(window, cx)
                .child(
                    div()
                        .id("page-title")
                        .map(ui::drag_area)
                        .flex_1()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground.opacity(0.5))
                        .child("Onboarding"),
                )
                .child(super::layout::panel_toggles(window, cx));
            return v_flex()
                .id("welcome")
                .track_focus(&self.focus)
                .size_full()
                .bg(theme.background)
                .child(header)
                .child(div().flex_1().min_h_0().child(self.setup.clone()))
                .into_any_element();
        }
        // Each row is a button: an icon in front says so (plain label rows read as text).
        let shortcut = |icon: ui::SkyeIcon, label: &'static str, action: Box<dyn Action>| {
            crate::ui::h_flex()
                .id(label)
                .role(gpui::accesskit::Role::Button)
                .aria_label(label)
                .gap_2p5()
                .w_full()
                .px_2p5()
                .py_1p5()
                .rounded(ui::RADIUS)
                .cursor_pointer()
                .hover(|this| this.bg(theme.accent))
                .on_click({
                    let action = action.boxed_clone();
                    move |_, window, cx| {
                        crate::workspace::dispatch(action.boxed_clone(), window, cx)
                    }
                })
                .child(
                    crate::ui::Icon::new(icon)
                        .size_4()
                        .text_color(theme.muted_foreground),
                )
                .child(div().flex_1().child(ui::KeyHint::new(label, action)))
        };
        let welcome = ui::EmptyState::new("Pick a thread to continue")
            .hint("Or start a new one. Every folder runs on the account it is bound to.")
            .page();
        let body = div().flex_1().flex().items_center().justify_center().child(
            v_flex()
                .w(gpui::px(384.))
                .gap_6()
                .child(
                    v_flex()
                        .gap_2()
                        .child(ui::peek_mark(gpui::px(28.), theme.foreground))
                        .child(welcome),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(shortcut(
                            ui::SkyeIcon::NewThread,
                            "New thread",
                            Box::new(super::NewThread),
                        ))
                        .child(shortcut(
                            ui::SkyeIcon::PanelLeft,
                            "Toggle sidebar",
                            Box::new(super::ToggleSidebar),
                        ))
                        .child(shortcut(
                            ui::SkyeIcon::PanelBottom,
                            "Toggle terminal",
                            Box::new(super::ToggleBottomPanel),
                        ))
                        .child(shortcut(
                            ui::SkyeIcon::SquareTerminal,
                            "New terminal",
                            Box::new(super::NewTerminal),
                        ))
                        .child(shortcut(
                            ui::SkyeIcon::Search,
                            "Command palette",
                            Box::new(crate::palette::OpenCommandPalette),
                        ))
                        .child(shortcut(
                            ui::SkyeIcon::CircleUser,
                            "Accounts and folders",
                            Box::new(super::OpenSettings),
                        )),
                ),
        );
        let header = super::layout::page_header(window, cx)
            .child(
                div()
                    .id("page-title")
                    .map(ui::drag_area)
                    .flex_1()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground.opacity(0.5))
                    .child("No active thread"),
            )
            .child(super::layout::panel_toggles(window, cx));
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
        let _render = crate::debug_renders::count("TerminalsEmpty");
        let theme = cx.theme();
        // T3's empty drawer: a line and a New terminal button (with its key).
        v_flex()
            .id("terminals-empty")
            .track_focus(&self.focus)
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(theme.background)
            .text_size(ui::BODY)
            .text_color(theme.muted_foreground)
            .child("No terminal running.")
            .child(
                Button::new("terminals-empty-new")
                    .outline()
                    .xsmall()
                    .label("New terminal")
                    .key_hint(&super::NewTerminal, Some(super::CONTEXT))
                    .on_click(|_, window, cx| {
                        crate::workspace::dispatch(Box::new(super::NewTerminal), window, cx)
                    }),
            )
    }
}

panel!(super::changes::ChangesView, "Changes", closable: true, title_bar: true, removed: |this, window, cx| {
    if let Some(thread) = this.thread.clone() {
        super::reopen::remember(window, super::reopen::Closed::Changes(thread), cx);
    }
}, |this| this.title());

/// The page is a native view: it stays on screen until told otherwise (see skyelab-browser's docs).
pub struct BrowserPanel {
    pub browser: Entity<BrowserView>,
    /// The thread whose agent drives this tab. `None` for the user's tabs, and after Take over: an agent
    /// only ever reaches a tab whose owner is its own thread.
    pub agent: Option<AgentOwner>,
    /// A tab on the user's own profile. `false` for every private tab (an agent's, also after Take over, a forwarded
    /// page, and their copies): Reopen Closed never brings those back, so none reaches the profile.
    pub users: bool,
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
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |b, cx| b.set_visible(active, cx));
    }
    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.users
            && let Ok(url) = self.browser.read(cx).current_url(cx)
        {
            super::reopen::remember(window, super::reopen::Closed::Browser(url), cx);
        }
        self.browser.update(cx, |b, cx| b.close(cx));
    }
}
impl Panel for BrowserPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            let title = self.tab_title(cx);
            crate::workspace::tab_context_menu::tab_title(
                cx.entity_id().into(),
                Some(title.clone()),
                title,
            )
        }
    }
}

impl BrowserPanel {
    /// What its tab and its palette entry say.
    pub(super) fn tab_title(&self, cx: &App) -> SharedString {
        browser_tab_title(
            self.browser.read(cx).title().map(|t| t.as_ref()),
            self.agent.is_some(),
        )
    }
}

/// The page's title, at most 28 characters, else what kind of tab it is. An agent's tab keeps saying so.
pub(super) fn browser_tab_title(page: Option<&str>, agent: bool) -> SharedString {
    const MAX: usize = 28;
    let kind = if agent { "Agent browser" } else { "Browser" };
    let Some(page) = page.map(str::trim).filter(|p| !p.is_empty()) else {
        return kind.into();
    };
    let page = match page.char_indices().nth(MAX) {
        Some((at, _)) => format!("{}…", page[..at].trim_end()),
        None => page.to_owned(),
    };
    if agent {
        format!("Agent: {page}").into()
    } else {
        page.into()
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
        let _render = crate::debug_renders::count("BrowserPanel");
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
                        .accessibility_label("Close browser tab")
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
