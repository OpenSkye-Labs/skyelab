//! The sidebar: the SkyeLab mark, the search / project scope / new thread row, then every project's threads in
//! one flat list (T3's default layout), and Settings at the bottom. Another machine's sidebar shows the same list, then
//! its projects.

use super::sidebar_rows::{THREAD_INDENT, scope_button, section_button, sidebar_row};
use super::sidebar_views::views_button;
use super::*;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use gpui::{AnyElement, ListAlignment, ListOffset, ListState, Subscription, list};
use gpui_component::input::{InputEvent, InputState};

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
    /// Flat thread rows: the project's name, drawn above the title. `None` for project rows.
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
    /// The header shows the row count and folds the section on click (T3's Working shelf).
    pub foldable: bool,
}

/// What a waiting agent asks the user for, as T3's row status names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// A permission request.
    Approval,
    /// An AskUserQuestion.
    Input,
    /// An ExitPlanMode plan to approve.
    Plan,
}

impl Ask {
    /// The ask a waiting request's tool makes.
    pub fn of(tool: &str) -> Self {
        match tool {
            skyelab_agent::QUESTION_TOOL => Self::Input,
            skyelab_agent::PLAN_TOOL => Self::Plan,
            _ => Self::Approval,
        }
    }

    /// The row's status word (T3: Approval, Input; a waiting plan is its "Plan Ready").
    pub fn label(self) -> &'static str {
        match self {
            Self::Approval => "Approval",
            Self::Input => "Input",
            Self::Plan => "Plan ready",
        }
    }
}

/// What a thread row carries besides its section item: kept apart so remote sections build rows as before.
#[derive(Default)]
pub struct RowMarks {
    pub pinned: bool,
    /// Settle and snooze from the row's buttons and menu.
    pub shelf: Option<RowShelf>,
    /// A scheduled task started it (from the stored row): the "Scheduled" badge.
    pub scheduled: bool,
    pub unread: bool,
    /// What the agent waits on the user for.
    pub ask: Option<Ask>,
    /// The right-click menu: label and the action it dispatches.
    pub menu: Vec<(SharedString, Box<dyn Action>)>,
    /// The agent's last reply as plain text, drawn muted under the title.
    pub preview: Option<SharedString>,
    /// The thread's worktree branch, else its project folder's.
    pub branch: Option<SharedString>,
    pub pr: Option<crate::workspace::pr_watch::PrBadge>,
    /// When the working turn started (ms since the epoch): the row reads "Working 1m 12s", ticking.
    pub started_ms: Option<i64>,
    /// What the row's hover card shows.
    pub card: Option<std::rc::Rc<RowCard>>,
    /// A comparison group's row: (its thread count, expanded, group), drawn as a "×3" toggle.
    pub group: Option<(usize, bool, i64)>,
    /// A comparison group's thread, listed under the group's row.
    pub nested: bool,
    /// A linked machine's row: that machine's short name, drawn in the meta line (T3's environment label).
    pub machine: Option<SharedString>,
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
    /// Text drawn wrapped under the lines (a linked machine's full state).
    pub note: Option<SharedString>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// A thread row's shelf (`host`: another machine's).
#[derive(Clone)]
pub struct RowShelf {
    pub id: i64,
    pub settled: bool,
    /// Asleep while ahead, woke once passed (until opened).
    pub snoozed_until: Option<i64>,
    pub host: Option<SharedString>,
}

/// T3's folded shelves at the bottom of the list ("Snoozed 2", "Settled 227"), before Archived.
pub struct Shelf {
    pub title: SharedString,
    pub items: Vec<SidebarItem>,
    /// Lists [`SETTLED_PAGE`] rows at a time, with a "Show N more" row.
    pub paged: bool,
}

use super::super::thread_list::SETTLED_PAGE;

#[path = "sidebar_shelves.rs"]
mod shelves;
#[cfg(test)]
pub(crate) use shelves::FOOTER_BUTTON_IDS;
use shelves::footer;

/// The selected project's archived threads, listed apart under a toggle row.
pub struct ArchivedRows {
    pub items: Vec<SidebarItem>,
    pub open: bool,
    /// Dispatched by the toggle row.
    pub toggle: Box<dyn Action>,
}

pub(in crate::workspace) use super::sidebar_snooze::SnoozeMenu;
pub use super::sidebar_views::{ScopeMenu, ScopeProject};

/// The thread filter's text changed.
pub struct SidebarSearch(pub SharedString);

pub struct Sidebar {
    pub sections: Vec<SidebarSection>,
    /// Thread rows' pin, unread and menu, by row id.
    pub marks: std::collections::HashMap<SharedString, RowMarks>,
    pub archived: Option<ArchivedRows>,
    pub shelves: Vec<Shelf>,
    /// Shelves the user opened and foldable sections the user folded, by title: kept across rebuilds.
    opened: std::collections::HashSet<SharedString>,
    folded: std::collections::HashSet<SharedString>,
    /// How many rows a paged shelf lists.
    paged_rows: usize,
    /// The thread filter (title and message text); hidden while another machine is shown.
    pub search: Entity<InputState>,
    pub searchable: bool,
    /// The project filter; `None` while another machine is shown.
    pub scope: Option<ScopeMenu>,
    pub(super) focus: FocusHandle,
    /// A click happened and the pointer hasn't moved since: rows show no hover card, so a row the click left under
    /// a resting pointer (or one the list moved there) opens its card only after the pointer moves and rests again.
    cards_held: bool,
    /// A new thread's row and when it appeared: it fades in once, keyed by its id (used once).
    pub appeared: Option<(SharedString, std::time::Instant)>,
    /// The list's lines, rebuilt with the rows (`rows_changed`); only the visible ones are drawn.
    entries: Vec<Entry>,
    list: ListState,
    /// How often the rows were rebuilt: tests check a keystroke or a streamed delta rebuilds nothing.
    #[cfg(test)]
    pub rebuilds: usize,
    _search: Subscription,
    /// Redraws when the update check finds (or drops) a newer release.
    _updates: Subscription,
    /// Redraws each second while a row shows a working timer.
    _tick: Option<gpui::Task<()>>,
    /// A row's snooze menu, drawn by the sidebar above its list: a popover inside a list row never paints.
    pub(in crate::workspace) snooze: SnoozeMenu,
}

/// One line of the sidebar's list: section `s`'s header, its row `i` or its empty state, the archived toggle and
/// rows, then the bottom padding.
#[derive(Clone, Copy)]
enum Entry {
    Header(usize),
    Row(usize, usize),
    Empty(usize),
    /// Shelf `k`'s toggle row, its row `i`, its "Show N more" row.
    ShelfHead(usize),
    ShelfRow(usize, usize),
    ShelfMore(usize),
    ArchivedHead,
    Archived(usize),
    End,
}

/// The archived toggle and rows, then the bottom padding.
fn entries(archived: Option<&ArchivedRows>) -> Vec<Entry> {
    let mut out = Vec::new();
    // "Archived" at the end of the list, as the user guide says.
    if let Some(a) = archived.filter(|a| !a.items.is_empty()) {
        out.push(Entry::ArchivedHead);
        if a.open {
            out.extend((0..a.items.len()).map(Entry::Archived));
        }
    }
    out.push(Entry::End);
    out
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
        let focus = cx.focus_handle();
        let mut this = Self {
            sections,
            marks: Default::default(),
            archived: None,
            shelves: Vec::new(),
            opened: Default::default(),
            folded: Default::default(),
            paged_rows: SETTLED_PAGE,
            search,
            searchable: false,
            scope: None,
            focus: focus.clone(),
            cards_held: false,
            appeared: None,
            entries: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, gpui::px(200.)),
            #[cfg(test)]
            rebuilds: 0,
            _search,
            _updates: cx.observe_global::<crate::updates::Updates>(|_, cx| cx.notify()),
            _tick: None,
            snooze: SnoozeMenu::new(&focus, cx),
        };
        this.rows_changed(cx);
        this
    }

    /// The sections, marks or archived rows were replaced: re-list them, keeping the scroll position.
    pub fn rows_changed(&mut self, cx: &mut Context<Self>) {
        #[cfg(test)]
        {
            self.rebuilds += 1;
        }
        self.entries = self.entries();
        let top = self.list.logical_scroll_top();
        self.list.reset(self.entries.len());
        self.list.scroll_to(ListOffset {
            item_ix: top.item_ix.min(self.entries.len() - 1),
            offset_in_item: top.offset_in_item,
        });
        self.sync_tick(cx);
        cx.notify();
    }

    /// Working rows read "Working 1m 12s": redraw each second while one does, and stop when none does.
    fn sync_tick(&mut self, cx: &mut Context<Self>) {
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
    /// List line `ix`, spaced as the sections were: 16 px between blocks, 4 under a header, 2 between rows.
    fn render_entry(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let now = ui::now_ms();
        let (top, el) = match self.entries[ix] {
            Entry::Header(s) if self.sections[s].foldable => {
                let s = &self.sections[s];
                let open = !self.folded.contains(&s.title);
                let head = self.fold_head(&s.title, s.items.len(), open, cx);
                (if ix == 0 { 0. } else { 16. }, head)
            }
            Entry::ShelfHead(k) => {
                let shelf = &self.shelves[k];
                let open = self.opened.contains(&shelf.title);
                (
                    16.,
                    self.fold_head(&shelf.title, shelf.items.len(), open, cx),
                )
            }
            Entry::ShelfRow(k, i) => (2., self.row(&self.shelves[k].items[i], false, now, theme)),
            Entry::ShelfMore(k) => {
                let more = (self.shelves[k].items.len() - self.shelf_shown(&self.shelves[k]))
                    .min(SETTLED_PAGE);
                let row = ui::row("shelf-more", false, theme)
                    .pl(gpui::px(THREAD_INDENT))
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .on_click(cx.listener(|this, _, _, cx| this.show_more(cx)))
                    .child(format!("Show {more} more"));
                (2., row.into_any_element())
            }
            Entry::Header(s) => {
                let s = &self.sections[s];
                let header = ui::SectionHeader::new(s.title.clone())
                    .when_some(section_button(s), |h, b| h.trailing(b));
                (if ix == 0 { 0. } else { 16. }, header.into_any_element())
            }
            Entry::Empty(s) => {
                let s = &self.sections[s];
                let empty = ui::EmptyState::new(s.empty_title.clone()).hint(s.empty_hint.clone());
                (4., empty.into_any_element())
            }
            Entry::Row(s, i) => {
                let row = self.row(&self.sections[s].items[i], false, now, theme);
                (if i == 0 { 4. } else { 2. }, row)
            }
            Entry::ArchivedHead => {
                let head = self.archived_head(theme).into_any_element();
                (if ix == 0 { 0. } else { 16. }, head)
            }
            Entry::Archived(i) => match &self.archived {
                Some(a) => (2., self.row(&a.items[i], true, now, theme)),
                None => (0., div().into_any_element()),
            },
            Entry::End => (12., div().into_any_element()),
        };
        div().px_2().pt(gpui::px(top)).child(el).into_any_element()
    }

    fn row(
        &self,
        item: &SidebarItem,
        indent: bool,
        now: i64,
        theme: &gpui_component::Theme,
    ) -> AnyElement {
        let row = sidebar_row(
            item,
            self.marks.get(&item.id),
            indent,
            !self.cards_held,
            &self.snooze,
            now,
            theme,
        );
        let since = self
            .appeared
            .as_ref()
            .filter(|a| a.0 == item.id)
            .map(|a| a.1);
        ui::motion::appear_since(row, since, item.id.clone())
    }

    /// The archived threads' toggle row ("Archived 3").
    fn archived_head(&self, theme: &gpui_component::Theme) -> gpui::Stateful<gpui::Div> {
        let (open, count, toggle) = self.archived.as_ref().map_or((false, 0, None), |a| {
            (a.open, a.items.len(), Some(a.toggle.boxed_clone()))
        });
        ui::row("archived-toggle", false, theme)
            .pl(gpui::px(THREAD_INDENT))
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .on_click(move |_, window, cx| {
                if let Some(t) = &toggle {
                    window.dispatch_action(t.boxed_clone(), cx)
                }
            })
            .child(
                Icon::new(if open {
                    SkyeIcon::ChevronDown
                } else {
                    SkyeIcon::ChevronRight
                })
                .size_3(),
            )
            .child(Icon::new(SkyeIcon::Archive).size_3())
            .child(div().flex_1().child("Archived"))
            .child(count.to_string())
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Sidebar");
        let pill = super::super::update_notice::pill(cx.entity_id(), window, cx);
        let theme = cx.theme();
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
            // Top row: brand mark and name; macOS traffic lights sit at the left.
            .child(
                div()
                    .map(ui::drag_area)
                    .px_2()
                    .min_h(ui::TOP_ROW_H)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .when(cfg!(target_os = "macos"), |d| d.pl(ui::MAC_TOP_PAD))
                    .child(
                        h_flex()
                            .gap_1p5()
                            .px_2p5()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(ui::brand_mark(gpui::px(16.), theme.sidebar_foreground))
                            .child("SkyeLab"),
                    ),
            )
            .when(self.searchable, |this| {
                this.child(
                    h_flex()
                        .px_2()
                        .pb_2()
                        .gap_1()
                        .child(
                            div().flex_1().min_w_0().child(
                                TextField::new(&self.search).small().cleanable(true).prefix(
                                    Icon::new(SkyeIcon::Search)
                                        .size_3p5()
                                        .text_color(theme.muted_foreground),
                                ),
                            ),
                        )
                        .children(self.scope.clone().map(|scope| views_button(scope, &focus)))
                        .children(self.scope.clone().map(|scope| scope_button(scope, &focus)))
                        .child(
                            Button::new("sidebar-new-thread")
                                .ghost()
                                .small()
                                .icon(SkyeIcon::NewThread)
                                .tooltip("New thread")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(super::super::NewThread), cx)
                                }),
                        ),
                )
            })
            .child(
                list(
                    self.list.clone(),
                    cx.processor(|this, ix, _, cx| this.render_entry(ix, cx)),
                )
                .flex_1()
                .min_h_0()
                .w_full(),
            )
            // T3's footer, with the update pill.
            .child(footer(pill))
            .children(self.snooze.render())
    }
}
