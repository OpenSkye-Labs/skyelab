//! The sidebar: the environment picker, the search / project scope / new thread row, then every project's threads in
//! one flat list (T3's default layout), and Settings at the bottom. Another machine's sidebar keeps its projects with
//! the selected one's threads under it.

use super::sidebar_rows::{THREAD_INDENT, scope_button, section_button, sidebar_row};
use super::*;
use gpui::{AnyElement, Subscription};
use gpui_component::input::{Input, InputEvent, InputState};

/// Which account a project row runs on.
#[derive(Clone)]
pub enum RowAccount {
    Bound(skyelab_core::Account),
    Unbound,
    /// Not shown: still resolving in the background, or bound on another machine.
    Hidden,
}

/// A row in a sidebar section. `id` is the domain id (project path, thread id), so it stays stable.
pub struct SidebarItem {
    pub id: SharedString,
    pub label: SharedString,
    pub selected: bool,
    /// Dispatched when the row is clicked.
    pub action: Box<dyn Action>,
    /// Thread rows: the status dot.
    pub status: Option<ui::Status>,
    /// Thread rows: when the thread last changed (ms since the epoch), shown as "3m".
    pub updated_ms: Option<i64>,
    /// Project rows, and flat thread rows: the account the folder runs on.
    pub account: Option<RowAccount>,
    /// Flat thread rows: the project's name, drawn above the title. `None` for project rows and nested threads.
    pub project: Option<SharedString>,
}

/// One titled group in the sidebar, with what to show while it is empty.
pub struct SidebarSection {
    pub title: SharedString,
    pub items: Vec<SidebarItem>,
    pub empty_title: SharedString,
    pub empty_hint: SharedString,
    /// A small button next to the title, e.g. "Add".
    pub button: Option<(SharedString, Box<dyn Action>)>,
    /// Drawn under the previous section's selected row (a project's threads, as in T3). With no row selected
    /// there, it is drawn as its own section.
    pub nested: bool,
}

/// What a thread row carries besides its section item: kept apart so remote sections build rows as before.
#[derive(Default)]
pub struct RowMarks {
    pub pinned: bool,
    pub unread: bool,
    /// The agent waits for a permission answer.
    pub approval: bool,
    /// The right-click menu: label and the action it dispatches.
    pub menu: Vec<(SharedString, Box<dyn Action>)>,
    /// The agent's last reply as plain text, drawn muted under the title.
    pub preview: Option<SharedString>,
    /// The thread's worktree branch, else its project folder's.
    pub branch: Option<SharedString>,
    /// The pull request opened from the thread: "#24".
    pub pr: Option<SharedString>,
    /// When the working turn started (ms since the epoch): the row reads "Working 1m 12s", ticking.
    pub started_ms: Option<i64>,
    /// What the row's hover card shows.
    pub card: Option<std::rc::Rc<RowCard>>,
}

/// A thread row's hover card (T3's thread tooltip): where it runs and on which account.
#[derive(Clone, Default)]
pub struct RowCard {
    pub title: SharedString,
    /// The project's name, as its sidebar row shows it.
    pub project: SharedString,
    /// The project folder, `~` for home.
    pub path: SharedString,
    pub worktree: Option<SharedString>,
    pub branch: Option<SharedString>,
    pub pr_url: Option<SharedString>,
    /// The account the thread runs on.
    pub account: Option<RowAccount>,
    /// "Bound at ~/work" (the binding that picks it), or that the folder is bound elsewhere now.
    pub binding: Option<SharedString>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// The selected project's archived threads, listed apart under a toggle row.
pub struct ArchivedRows {
    pub items: Vec<SidebarItem>,
    pub open: bool,
    /// Dispatched by the toggle row.
    pub toggle: Box<dyn Action>,
}

/// The project scope filter: which projects' threads the list shows, and each project's menu.
#[derive(Clone, Default)]
pub struct ScopeMenu {
    pub projects: Vec<ScopeProject>,
}

#[derive(Clone)]
pub struct ScopeProject {
    pub id: i64,
    pub name: SharedString,
    pub checked: bool,
}

impl ScopeMenu {
    pub(super) fn filtered(&self) -> bool {
        self.projects.iter().any(|p| p.checked)
    }
}

/// The thread filter's text changed.
pub struct SidebarSearch(pub SharedString);

pub struct Sidebar {
    pub sections: Vec<SidebarSection>,
    /// The environment switcher, drawn in place of the title once the workspace sets it.
    pub picker: Option<Entity<super::super::env_picker::EnvPicker>>,
    /// Thread rows' pin, unread and menu, by row id.
    pub marks: std::collections::HashMap<SharedString, RowMarks>,
    pub archived: Option<ArchivedRows>,
    /// The thread filter (title and message text); hidden while another machine is shown.
    pub search: Entity<InputState>,
    pub searchable: bool,
    /// The project filter; `None` while another machine is shown.
    pub scope: Option<ScopeMenu>,
    pub(super) focus: FocusHandle,
    /// A click happened and the pointer hasn't moved since: rows show no hover card, so a row the click left under
    /// a resting pointer (or one the list moved there) opens its card only after the pointer moves and rests again.
    cards_held: bool,
    _search: Subscription,
    /// Redraws each second while a row shows a working timer.
    _tick: Option<gpui::Task<()>>,
}

impl EventEmitter<SidebarSearch> for Sidebar {}

impl Sidebar {
    pub fn new(sections: Vec<SidebarSection>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search threads"));
        let _search = cx.subscribe(&search, |_, input, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let text = input.read(cx).value();
                cx.emit(SidebarSearch(text));
            }
        });
        Self {
            sections,
            picker: None,
            marks: Default::default(),
            archived: None,
            search,
            searchable: false,
            scope: None,
            focus: cx.focus_handle(),
            cards_held: false,
            _search,
            _tick: None,
        }
    }

    /// Working rows read "Working 1m 12s": redraw each second while one does, and stop when none does.
    pub fn sync_tick(&mut self, cx: &mut Context<Self>) {
        if !self.marks.values().any(|m| m.started_ms.is_some()) {
            self._tick = None;
        } else if self._tick.is_none() {
            self._tick = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
            }));
        }
    }
}

// The sidebar draws its own header.
panel!(Sidebar, "Sidebar", closable: false, title_bar: false, |_this| "Sidebar");

impl Sidebar {
    fn rows(
        &self,
        s: &SidebarSection,
        indent: bool,
        now: i64,
        theme: &gpui_component::Theme,
    ) -> gpui::Div {
        if s.items.is_empty() {
            return v_flex()
                .when(indent, |this| this.pl(gpui::px(THREAD_INDENT - 8.)))
                .child(ui::EmptyState::new(s.empty_title.clone()).hint(s.empty_hint.clone()));
        }
        v_flex().gap_0p5().children(
            s.items
                .iter()
                .map(|item| self.row(item, indent, now, theme)),
        )
    }

    fn row(
        &self,
        item: &SidebarItem,
        indent: bool,
        now: i64,
        theme: &gpui_component::Theme,
    ) -> AnyElement {
        sidebar_row(
            item,
            self.marks.get(&item.id),
            &self.focus,
            indent,
            !self.cards_held,
            now,
            theme,
        )
    }

    /// The archived threads' toggle row ("Archived 3") and, when open, their rows.
    fn archived(&self, now: i64, theme: &gpui_component::Theme) -> Option<gpui::Div> {
        let a = self.archived.as_ref().filter(|a| !a.items.is_empty())?;
        let toggle = a.toggle.boxed_clone();
        let head = ui::row("archived-toggle", false, theme)
            .pl(gpui::px(THREAD_INDENT))
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .on_click(move |_, window, cx| window.dispatch_action(toggle.boxed_clone(), cx))
            .child(
                Icon::new(if a.open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size_3(),
            )
            .child(div().flex_1().child("Archived"))
            .child(a.items.len().to_string());
        Some(v_flex().gap_0p5().child(head).when(a.open, |this| {
            this.children(a.items.iter().map(|item| self.row(item, true, now, theme)))
        }))
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let now = ui::now_ms();
        let mut blocks = Vec::new();
        let mut i = 0;
        while let Some(s) = self.sections.get(i) {
            // A project's threads go under its row (T3's folder layout); with no project selected they stand alone.
            let nested = self
                .sections
                .get(i + 1)
                .filter(|n| n.nested && s.items.iter().any(|it| it.selected));
            let body =
                match nested {
                    None => self.rows(s, false, now, theme),
                    Some(n) => v_flex().gap_0p5().children(s.items.iter().map(|item| {
                        let row = self.row(item, false, now, theme);
                        if !item.selected {
                            return row;
                        }
                        v_flex()
                            .gap_0p5()
                            .child(row)
                            .child(self.rows(n, true, now, theme))
                            .children(self.archived(now, theme))
                            .children(section_button(n).map(|b| {
                                h_flex().pl(gpui::px(THREAD_INDENT - 10.)).pb_1().child(b)
                            }))
                            .into_any_element()
                    })),
                };
            blocks.push(
                v_flex()
                    .gap_1()
                    .child(
                        ui::SectionHeader::new(s.title.clone())
                            .when_some(section_button(s), |h, b| h.trailing(b)),
                    )
                    .child(body),
            );
            i += if nested.is_some() { 2 } else { 1 };
        }
        let focus = self.focus.clone();
        v_flex()
            .id("sidebar")
            .track_focus(&self.focus)
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.cards_held = true;
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                if this.cards_held {
                    this.cards_held = false;
                    cx.notify();
                }
            }))
            .size_full()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            // The window's top row: on macOS the traffic lights sit at its start, so the picker starts after them.
            .child(
                div()
                    .px_2()
                    .min_h(ui::TOP_ROW_H)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .when(cfg!(target_os = "macos"), |d| d.pl(ui::TRAFFIC_LIGHTS_W))
                    .child(match &self.picker {
                        Some(picker) => picker.clone().into_any_element(),
                        None => h_flex()
                            .gap_1p5()
                            .px_2p5()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(ui::brand_mark(gpui::px(16.), theme.sidebar_foreground))
                            .child("SkyeLab")
                            .into_any_element(),
                    }),
            )
            .when(self.searchable, |this| {
                this.child(
                    h_flex()
                        .px_2()
                        .pb_2()
                        .gap_1()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.search).small().cleanable(true).prefix(
                                    Icon::new(IconName::Search)
                                        .size_3p5()
                                        .text_color(theme.muted_foreground),
                                ),
                            ),
                        )
                        .children(self.scope.clone().map(|scope| scope_button(scope, &focus)))
                        .child(
                            Button::new("sidebar-new-thread")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .tooltip("New thread")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(super::super::NewThread), cx)
                                }),
                        ),
                )
            })
            .child(
                v_flex()
                    .id("sidebar-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_3()
                    .gap_4()
                    .children(blocks),
            )
            // T3's footer: Settings, always in reach.
            .child(
                h_flex().flex_shrink_0().px_2().py_2().child(
                    Button::new("sidebar-settings")
                        .ghost()
                        .small()
                        .icon(IconName::Settings)
                        .label("Settings")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(super::super::OpenSettings), cx)
                        }),
                ),
            )
    }
}
