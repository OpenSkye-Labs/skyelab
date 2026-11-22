//! The workspace's dock panels and the dock traits each one needs.

use crate::settings::Settings;
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke,
    SharedString, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{BasePanel, Panel, PanelEvent},
    h_flex,
    kbd::Kbd,
    v_flex,
};
use skyelab_browser::BrowserView;
use skyelab_terminal::TerminalView;

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

/// A row in a sidebar section. `id` is the domain id (project path, thread id), so it stays stable.
pub struct SidebarItem {
    pub id: SharedString,
    pub label: SharedString,
    pub selected: bool,
    /// Dispatched when the row is clicked.
    pub action: Box<dyn Action>,
}

/// One titled group in the sidebar, with what to show while it is empty.
pub struct SidebarSection {
    pub title: SharedString,
    pub items: Vec<SidebarItem>,
    pub empty_title: SharedString,
    pub empty_hint: SharedString,
    /// A small button next to the title, e.g. "Add".
    pub button: Option<(SharedString, Box<dyn Action>)>,
}

pub struct Sidebar {
    pub sections: Vec<SidebarSection>,
    /// The environment switcher, drawn in place of the title once the workspace sets it.
    pub picker: Option<Entity<super::env_picker::EnvPicker>>,
    focus: FocusHandle,
}

impl Sidebar {
    pub fn new(sections: Vec<SidebarSection>, cx: &mut Context<Self>) -> Self {
        Self {
            sections,
            picker: None,
            focus: cx.focus_handle(),
        }
    }
}

// The sidebar draws its own header.
panel!(Sidebar, "Sidebar", closable: false, title_bar: false, |_this| "Sidebar");

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
                    let action = item.action.boxed_clone();
                    div()
                        .id(item.id.clone())
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .truncate()
                        .cursor_pointer()
                        .when(item.selected, |this| this.bg(theme.sidebar_accent))
                        .hover(|this| this.bg(theme.sidebar_accent))
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(action.boxed_clone(), cx)
                        })
                        .child(item.label.clone())
                }))
            };
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .px_1()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(s.title.to_uppercase()),
                        )
                        .children(s.button.as_ref().map(|(label, action)| {
                            let action = action.boxed_clone();
                            Button::new(SharedString::from(format!("section-{}", s.title)))
                                .ghost()
                                .xsmall()
                                .label(label.clone())
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(action.boxed_clone(), cx)
                                })
                        })),
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
            .child(match &self.picker {
                Some(picker) => picker.clone().into_any_element(),
                None => div()
                    .px_1()
                    .text_base()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("SkyeLab")
                    .into_any_element(),
            })
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
        let shortcut = |label: &'static str, key: &'static str, action: Box<dyn Action>| {
            h_flex()
                .id(label)
                .w_full()
                .justify_between()
                .px_2()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .hover(|this| this.bg(theme.secondary))
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
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
panel!(super::thread::ThreadView, "Thread", closable: true, |this| this.title());
panel!(crate::remote::RemotePanel, "Remote", closable: true, |_this| "Remote");
panel!(super::remote_thread::RemoteThreadView, "RemoteThread", closable: true, |this| this.title());
panel!(super::changes::ChangesView, "Changes", closable: true, |this| this.title());

/// The page is a native view: it stays on screen until told otherwise (see skyelab-browser's docs).
pub struct BrowserPanel {
    pub browser: Entity<BrowserView>,
}

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
        "Browser"
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

impl Render for BrowserPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.browser.clone())
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
