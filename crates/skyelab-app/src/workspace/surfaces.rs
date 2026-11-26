//! The right dock's first tab (T3's "Open a surface"): what the dock can show, each with its letter. A letter opens it
//! while the tab has focus; a surface the shown thread can't have is greyed out.

use super::thread::ThreadView;
use super::{NewTerminal, OpenBrowser, OpenDevices, Workspace, files::SearchFiles};
use crate::ui;
use crate::ui::Kbd;
use crate::ui::SkyeIcon;
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyBinding,
    Render, Subscription, Window, actions, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Icon,
    dock::{BasePanel, Panel, PanelControl, PanelEvent},
    h_flex, v_flex,
};

actions!(
    skyelab,
    [
        OpenThreadChanges,
        OpenThreadPr,
        OpenSideChat,
        /// The picker's Terminal: a shell in the drawer, and the picker put away when it was all the panel held.
        OpenSurfaceTerminal
    ]
);

const CONTEXT: &str = "Surfaces";

pub(super) fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("b", OpenBrowser, c),
        KeyBinding::new("t", OpenSurfaceTerminal, c),
        KeyBinding::new("f", SearchFiles, c),
        KeyBinding::new("d", OpenThreadChanges, c),
        KeyBinding::new("p", OpenThreadPr, c),
        KeyBinding::new("m", OpenDevices, c),
        KeyBinding::new("s", OpenSideChat, c),
        // The picker is the right panel's way in: Esc puts it away, as on Devices.
        KeyBinding::new("escape", super::layout::HideRightPanel, c),
    ]);
}

/// Which surfaces the shown thread can open; the tab redraws only when this changes.
#[derive(Clone, Copy, PartialEq, Default)]
struct Avail {
    thread: bool,
    pr: bool,
    side_chat: bool,
}

fn avail(v: &ThreadView) -> Avail {
    let row = v.thread.as_ref();
    Avail {
        thread: true,
        pr: row.is_some_and(|t| t.pr_url.is_some()),
        side_chat: row.is_some_and(|t| t.side_of.is_none()),
    }
}

/// A surface: its icon, name, letter, whether the thread can have it, why not (T3's unavailable hints), and the
/// action that opens it.
type Row = (
    SkyeIcon,
    &'static str,
    &'static str,
    bool,
    &'static str,
    Box<dyn Action>,
);

pub struct SurfacesPanel {
    focus: FocusHandle,
    avail: Avail,
    _sub: Option<Subscription>,
}

impl SurfacesPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            avail: Avail::default(),
            _sub: None,
        }
    }

    /// Follows `view`, the thread the window shows (none on Welcome or Settings).
    pub(super) fn set_thread(&mut self, view: Option<Entity<ThreadView>>, cx: &mut Context<Self>) {
        self._sub = view.as_ref().map(|view| {
            cx.observe(view, |this, view, cx| {
                let now = avail(view.read(cx));
                if now != this.avail {
                    this.avail = now;
                    cx.notify();
                }
            })
        });
        self.avail = view.map(|v| avail(v.read(cx))).unwrap_or_default();
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for SurfacesPanel {}

impl BasePanel for SurfacesPanel {
    fn panel_name(&self) -> &'static str {
        "Surfaces"
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for SurfacesPanel {
    // T3's "+" that opens this list.
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Icon::new(SkyeIcon::Plus).size_3p5()
    }
    // The expand button, and a working Zoom item in "…" (a menu-less control leaves that item disabled).
    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        Some(PanelControl::Both)
    }
}

impl Focusable for SurfacesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SurfacesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("SurfacesPanel");
        let theme = cx.theme();
        let a = self.avail;
        let rows: [Row; 8] = [
            (
                SkyeIcon::Browser,
                "Browser",
                "B",
                true,
                "",
                Box::new(OpenBrowser),
            ),
            (
                SkyeIcon::SquareTerminal,
                "Terminal",
                "T",
                true,
                "",
                Box::new(OpenSurfaceTerminal),
            ),
            (
                SkyeIcon::Files,
                "Files",
                "F",
                a.thread,
                "Available from a thread.",
                Box::new(SearchFiles),
            ),
            (
                SkyeIcon::FileDiff,
                "Diff",
                "D",
                a.thread,
                "Available from a thread.",
                Box::new(OpenThreadChanges),
            ),
            (
                SkyeIcon::GitPullRequest,
                "Pull request",
                "P",
                a.pr,
                "No pull request on this thread yet.",
                Box::new(OpenThreadPr),
            ),
            // No linked issues here yet: always greyed, as T3 greys what a thread doesn't have.
            (
                SkyeIcon::Link,
                "Linked items",
                "L",
                false,
                "No linked items available.",
                Box::new(gpui::NoAction),
            ),
            (
                SkyeIcon::Smartphone,
                "Device",
                "M",
                true,
                "",
                Box::new(OpenDevices),
            ),
            (
                SkyeIcon::MessagesSquare,
                "Side chat",
                "S",
                a.side_chat,
                "Available in a thread, not inside a side chat.",
                Box::new(OpenSideChat),
            ),
        ];
        let rows = rows.into_iter().map(|(icon, label, key, on, why, action)| {
            h_flex()
                .id(label)
                .h(gpui::px(32.))
                .px_2p5()
                .gap_2p5()
                .rounded(ui::RADIUS)
                .text_size(ui::BODY)
                .child(Icon::new(icon).size_4())
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(Kbd::new(gpui::Keystroke {
                    key: key.into(),
                    ..Default::default()
                }))
                .map(|row| {
                    if on {
                        row.cursor_pointer()
                            .hover(|s| s.bg(theme.accent.opacity(0.6)))
                            .on_click(move |_, window, cx| {
                                crate::workspace::dispatch(action.boxed_clone(), window, cx)
                            })
                    } else {
                        row.opacity(0.5).tooltip(ui::Tooltip::text(why))
                    }
                })
        });
        v_flex()
            .id("surfaces")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .items_center()
            .justify_center()
            .px_6()
            .bg(theme.background)
            .child(
                v_flex()
                    .w_full()
                    .max_w(gpui::px(320.))
                    .py_6()
                    .child(
                        div()
                            .mb_3()
                            .text_center()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("Open a surface"),
                    )
                    .child(v_flex().gap_0p5().children(rows)),
            )
    }
}

impl Workspace {
    /// How many tabs the right dock holds (Open a surface counts as one).
    pub(super) fn right_tab_count(&self, cx: &App) -> usize {
        let mut n = 0;
        if let Some(layout) = self
            .dock
            .read(cx)
            .layout(gpui_component::dock::DockPlacement::Right)
        {
            layout.root().walk(&mut |node| {
                if let gpui_component::dock::PaneRef::Tabs { panels, .. } = node.kind() {
                    n += panels.len();
                }
            });
        }
        n
    }

    pub(super) fn surface_actions(
        &self,
        el: gpui::Stateful<gpui::Div>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        el.on_action(cx.listener(|this, _: &OpenThreadChanges, window, cx| {
            if let Some(view) = this.current_thread() {
                this.open_changes(&view, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &OpenSurfaceTerminal, window, cx| {
            this.new_terminal(&NewTerminal, window, cx);
            // Terminals live in the drawer: a panel left showing only this picker goes away.
            if this.right_tab_count(cx) <= 1 {
                this.hide_right(&super::layout::HideRightPanel, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &OpenThreadPr, window, cx| {
            let url = this
                .current_thread()
                .and_then(|v| v.read(cx).thread.as_ref()?.pr_url.clone());
            if let Some(url) = url {
                let browser =
                    cx.new(|cx| skyelab_browser::BrowserView::new(Some(&url), window, cx));
                this.add_browser(browser, None, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &OpenSideChat, _, cx| {
            if let Some(view) = this.current_thread() {
                view.update(cx, |v, cx| {
                    if !v.is_side_chat() && v.thread.is_some() {
                        cx.emit(super::thread::ThreadViewEvent::Side(
                            super::side_chat::SideAsk::New(None),
                        ));
                    }
                });
            }
        }))
    }
}
