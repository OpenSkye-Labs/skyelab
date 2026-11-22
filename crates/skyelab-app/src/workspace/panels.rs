//! The workspace's dock panels. Each only renders what the workspace hands it.

use crate::settings::Settings;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke,
    SharedString, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _,
    dock::{BasePanel, Panel, PanelEvent},
    h_flex,
    kbd::Kbd,
    v_flex,
};
use skyelab_terminal::TerminalView;

/// The boilerplate every panel shares: a name for persisted layouts, a tab title,
/// and whether the user may close it.
macro_rules! panel {
    ($ty:ty, $name:literal, closable: $closable:literal, |$this:ident| $title:expr) => {
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
        }
    };
}

/// A row in a sidebar section. `id` is the domain id (project path, thread id), so it stays stable.
pub struct SidebarItem {
    pub id: SharedString,
    pub label: SharedString,
}

/// One titled group in the sidebar, with what to show while it is empty.
pub struct SidebarSection {
    pub title: SharedString,
    pub items: Vec<SidebarItem>,
    pub empty_title: SharedString,
    pub empty_hint: SharedString,
}

pub struct Sidebar {
    pub sections: Vec<SidebarSection>,
    focus: FocusHandle,
}

impl Sidebar {
    pub fn new(sections: Vec<SidebarSection>, cx: &mut Context<Self>) -> Self {
        Self {
            sections,
            focus: cx.focus_handle(),
        }
    }
}

panel!(Sidebar, "Sidebar", closable: false, |_this| "Sidebar");

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let section = |s: &SidebarSection| {
            let body = if s.items.is_empty() {
                v_flex()
                    .gap_1()
                    .px_3()
                    .py_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.sidebar_border)
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.sidebar_foreground)
                            .child(s.empty_title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(gpui::rems(1.1))
                            .text_color(theme.muted_foreground)
                            .child(s.empty_hint.clone()),
                    )
            } else {
                v_flex().gap_0p5().children(s.items.iter().map(|item| {
                    div()
                        .id(item.id.clone())
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .hover(|this| this.bg(theme.sidebar_accent))
                        .child(item.label.clone())
                }))
            };
            v_flex()
                .gap_2()
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(s.title.to_uppercase()),
                )
                .child(body)
        };
        v_flex()
            .id("sidebar")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .px_3()
            .pt_4()
            .pb_3()
            .gap_6()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                div()
                    .px_1()
                    .text_base()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("SkyeLab"),
            )
            .children(self.sections.iter().map(section))
    }
}

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

panel!(Welcome, "Welcome", closable: false, |_this| "Welcome");

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let shortcut = |label: &'static str, key: &'static str| {
            h_flex()
                .w_full()
                .justify_between()
                .py_1p5()
                .text_sm()
                .child(div().text_color(theme.foreground).child(label))
                .children(Keystroke::parse(key).ok().map(Kbd::new))
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
                    .w(gpui::px(340.))
                    .gap_6()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child("No thread open"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("Every folder runs on the account it is bound to."),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(shortcut("Toggle sidebar", super::TOGGLE_SIDEBAR_KEY))
                            .child(shortcut("Toggle bottom panel", super::TOGGLE_BOTTOM_KEY))
                            .child(shortcut("New terminal", super::NEW_TERMINAL_KEY))
                            .child(shortcut("Accounts and folders", super::OPEN_SETTINGS_KEY)),
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
            .text_sm()
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
}

impl TerminalPanel {
    pub fn new(view: Entity<TerminalView>, title: SharedString) -> Self {
        Self { view, title }
    }
}

// shortcut: terminal tabs can't be closed yet (the workspace owns their PTYs), add closing with tab management.
panel!(TerminalPanel, "Terminal", closable: false, |this| this.title.clone());

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

panel!(Settings, "Settings", closable: true, |_this| "Settings");

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
