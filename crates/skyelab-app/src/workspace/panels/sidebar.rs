//! The sidebar: the environment picker, then projects with the selected one's threads under it (T3's folder layout).

use super::*;
use gpui::{AnyElement, Subscription};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;

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
    /// Project rows: the account the folder runs on.
    pub account: Option<RowAccount>,
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
    /// The right-click menu: label and the action it dispatches.
    pub menu: Vec<(SharedString, Box<dyn Action>)>,
}

/// The selected project's archived threads, listed apart under a toggle row.
pub struct ArchivedRows {
    pub items: Vec<SidebarItem>,
    pub open: bool,
    /// Dispatched by the toggle row.
    pub toggle: Box<dyn Action>,
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
    pub(super) focus: FocusHandle,
    _search: Subscription,
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
            focus: cx.focus_handle(),
            _search,
        }
    }
}

// The sidebar draws its own header.
panel!(Sidebar, "Sidebar", closable: false, title_bar: false, toggles: false, |_this| "Sidebar");

fn section_button(s: &SidebarSection) -> Option<Button> {
    s.button.as_ref().map(|(label, action)| {
        let action = action.boxed_clone();
        Button::new(SharedString::from(format!("section-{}", s.title)))
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .label(label.clone())
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    })
}

/// Where a thread's title starts, so it lines up under its project's name (T3's `ps-9`): row padding + monogram + gap.
const THREAD_INDENT: f32 = 36.;

fn sidebar_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    focus: &FocusHandle,
    indent: bool,
    now: i64,
    theme: &gpui_component::Theme,
) -> AnyElement {
    let row = plain_row(item, marks, indent, now, theme);
    let Some(menu) = marks.filter(|m| !m.menu.is_empty()) else {
        return row.into_any_element();
    };
    let entries: Vec<(SharedString, Box<dyn Action>)> = menu
        .menu
        .iter()
        .map(|(l, a)| (l.clone(), a.boxed_clone()))
        .collect();
    let focus = focus.clone();
    row.context_menu(move |mut menu, _, _| {
        menu = menu.action_context(focus.clone());
        for (label, action) in &entries {
            menu = menu.menu(label.clone(), action.boxed_clone());
        }
        menu
    })
    .into_any_element()
}

fn plain_row(
    item: &SidebarItem,
    marks: Option<&RowMarks>,
    indent: bool,
    now: i64,
    theme: &gpui_component::Theme,
) -> gpui::Stateful<gpui::Div> {
    let action = item.action.boxed_clone();
    let is_project = item.account.is_some();
    let (pinned, unread) = marks.map_or((false, false), |m| (m.pinned, m.unread));
    // A project row is never lifted (T3 keeps project headers flat): a selected one is only set in medium weight.
    ui::row(item.id.clone(), item.selected && !is_project, theme)
        .when(item.selected && is_project, |this| {
            this.font_weight(gpui::FontWeight::MEDIUM)
        })
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .when(indent, |this| this.pl(gpui::px(THREAD_INDENT)))
        .when(is_project, |this| {
            this.child(ui::Monogram::new(&item.label))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(unread, |d| d.font_weight(gpui::FontWeight::MEDIUM))
                .child(item.label.clone()),
        )
        .when(pinned, |this| {
            this.child(
                Icon::new(IconName::Star)
                    .size_3()
                    .text_color(theme.muted_foreground),
            )
        })
        // T3's unread dot: a finished turn nobody has looked at yet.
        .when(unread, |this| this.child(ui::StatusDot::new(theme.info)))
        .children(item.account.as_ref().and_then(|a| {
            let badge = match a {
                RowAccount::Bound(acc) => ui::AccountBadge::new(acc.id.clone(), acc.label.clone()),
                RowAccount::Unbound => ui::AccountBadge::unbound(),
                RowAccount::Hidden => return None,
            };
            Some(div().flex_shrink_0().max_w(gpui::px(104.)).child(badge))
        }))
        .children(match item.status.and_then(ui::Status::badge) {
            Some((icon, word)) => {
                let color = item
                    .status
                    .map_or(theme.muted_foreground, |s| s.color(theme));
                Some(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .text_size(ui::META)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(ui::ink(theme, color))
                        .child(Icon::new(icon).size_3().text_color(color))
                        .child(word)
                        .into_any_element(),
                )
            }
            // shortcut: the time is computed when the sidebar draws, so an idle sidebar shows it as of its last redraw;
            // add a minute timer if stale times get noticed.
            None => item.updated_ms.map(|t| {
                div()
                    .flex_shrink_0()
                    .text_size(ui::META)
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(theme.muted_foreground)
                    .child(ui::ago(now, t))
                    .into_any_element()
            }),
        })
}

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
        v_flex()
            .id("sidebar")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .px_2()
            .pb_3()
            .gap_4()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            // The window's top row: on macOS the traffic lights sit at its start, so the picker starts after them.
            .child(
                div()
                    .min_h(ui::TOP_ROW_H)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .when(cfg!(target_os = "macos"), |d| {
                        d.pl(ui::TRAFFIC_LIGHTS_W - gpui::px(8.))
                    })
                    .child(match &self.picker {
                        Some(picker) => picker.clone().into_any_element(),
                        None => div()
                            .px_2p5()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("SkyeLab")
                            .into_any_element(),
                    }),
            )
            .when(self.searchable, |this| {
                this.child(
                    Input::new(&self.search).small().cleanable(true).prefix(
                        Icon::new(IconName::Search)
                            .size_3p5()
                            .text_color(theme.muted_foreground),
                    ),
                )
            })
            .children(blocks)
    }
}
