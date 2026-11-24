//! The palette's view: T3's card in a dialog, with gpui-component's `Command` for the list (its virtual list, arrow
//! keys that wrap, hover select and keep-in-view), our rows, pages for submenus, and the footer of key hints.

use super::{Entry, Group, Kind, PLACEHOLDER, Search, arrange, flatten, match_ranges, rank};
use crate::ui::{CommandRow, Kbd, RADIUS_XL, SkyeIcon};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, Keystroke, ParentElement as _,
    Render, SharedString, Styled as _, Task, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IndexPath, WindowExt as _,
    command::{Command, CommandGroup, CommandItem, CommandState},
    h_flex,
};

/// A submenu's items, shown instead of the root with a Back row.
struct Page {
    title: SharedString,
    entries: Vec<Entry>,
}

struct Palette {
    state: Entity<CommandState>,
    /// The root's entries: top-level, then submenu items from `nested`, then the last search's from `fixed`.
    entries: Vec<Entry>,
    nested: usize,
    fixed: usize,
    page: Option<Page>,
    query: SharedString,
    /// What the list shows now, by `arrange`; indexes into [`Self::list`].
    groups: Vec<Group>,
    prev: FocusHandle,
    search: Option<Search>,
    _search: Option<Task<()>>,
}

/// Opens the palette over `entries`; `search` adds more for each query.
pub fn open(
    entries: Vec<Entry>,
    search: Option<Search>,
    prev: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    open_with(entries, None, search, prev, window, cx);
}

/// Opens the palette straight on a page (T3's "New thread in…" intent); Back closes it.
pub fn open_page(
    title: impl Into<SharedString>,
    entries: Vec<Entry>,
    prev: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let page = Page {
        title: title.into(),
        entries,
    };
    open_with(Vec::new(), Some(page), None, prev, window, cx);
}

fn open_with(
    entries: Vec<Entry>,
    page: Option<Page>,
    search: Option<Search>,
    prev: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let nested = entries.len();
    let entries = flatten(entries);
    let state = cx.new(|cx| CommandState::new(window, cx));
    let palette = cx.new(|_| {
        let mut p = Palette {
            state: state.clone(),
            nested,
            fixed: entries.len(),
            entries,
            page,
            query: SharedString::default(),
            groups: Vec::new(),
            prev,
            search,
            _search: None,
        };
        p.arrange();
        p
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let palette = palette.clone();
        // T3's card: max-w-xl, rounded-2xl, the list and footer reaching its edges.
        dialog
            .w(px(600.))
            .p_0()
            .rounded(RADIUS_XL)
            .overflow_hidden()
            .close_button(false)
            .content(move |content, _, _| content.p_0().child(palette.clone()))
    });
    state.update(cx, |state, cx| state.focus(window, cx));
}

impl Palette {
    fn list(&self) -> &[Entry] {
        self.page.as_ref().map_or(&self.entries, |p| &p.entries)
    }

    fn arrange(&mut self) {
        self.groups = match &self.page {
            Some(page) => {
                // By the shown title: a page's labels share a prefix ("Theme: …") that would match every row.
                let rows = rank(&self.query, page.entries.iter().map(|e| e.shown().as_ref()));
                let label = page.title.clone();
                (!rows.is_empty())
                    .then_some(Group { label, rows })
                    .into_iter()
                    .collect()
            }
            None => arrange(&self.query, &self.entries, self.nested, self.fixed),
        };
    }

    fn entry_at(&self, ix: IndexPath) -> Option<&Entry> {
        let i = *self.groups.get(ix.section)?.rows.get(ix.row)?;
        self.list().get(i)
    }

    /// Runs the search for `query` and appends what it finds; a newer query drops (cancels) an older search.
    fn find_more(&mut self, cx: &mut Context<Self>) {
        let query = self.query.to_string();
        let search = self.search.clone();
        let Some(search) = search.filter(|_| self.page.is_none() && !query.trim().is_empty())
        else {
            self._search = None;
            return;
        };
        let task = search(query, cx);
        self._search = Some(cx.spawn(async move |p, cx| {
            let found = task.await;
            _ = p.update(cx, |p, cx| {
                p.entries.truncate(p.fixed);
                let shown: std::collections::HashSet<SharedString> = p
                    .groups
                    .iter()
                    .flat_map(|g| g.rows.iter().map(|&i| p.entries[i].label.clone()))
                    .collect();
                p.entries
                    .extend(found.into_iter().filter(|e| !shown.contains(&e.label)));
                p.arrange();
                cx.notify();
            });
        }));
    }

    fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_owned().into();
        self.entries.truncate(self.fixed);
        self.arrange();
        self.find_more(cx);
        cx.notify();
    }
}

/// Closes the palette and gives focus back to what had it.
fn close(palette: &Entity<Palette>, window: &mut Window, cx: &mut App) {
    let prev = palette.read(cx).prev.clone();
    window.close_dialog(cx);
    // The dialog sits outside the workspace, so the action must start from the element focused before it.
    window.focus(&prev, cx);
}

/// Shows `page` (or the root for `None`) with an empty query.
fn show_page(palette: &Entity<Palette>, page: Option<Page>, window: &mut Window, cx: &mut App) {
    let state = palette.update(cx, |p, cx| {
        p.page = page;
        p.set_query("", cx);
        p.state.clone()
    });
    // Outside the palette's update: the query callback updates the palette. An unchanged (empty) query keeps the
    // old highlight, so start the new list at its first row.
    state.update(cx, |s, cx| {
        s.set_query("", window, cx);
        s.set_selected_index(Some(IndexPath::default()), window, cx);
    });
}

/// Escape, Backspace on an empty query and the Back row: a page goes back to the root; the root (or a page opened
/// on its own) closes.
fn back(palette: &Entity<Palette>, window: &mut Window, cx: &mut App) {
    let p = palette.read(cx);
    let to_root = p.page.is_some() && !p.entries.is_empty();
    if to_root {
        show_page(palette, None, window, cx);
    } else {
        close(palette, window, cx);
    }
}

fn choose(palette: &WeakEntity<Palette>, ix: IndexPath, window: &mut Window, cx: &mut App) {
    let Some(palette) = palette.upgrade() else {
        return;
    };
    let Some(entry) = palette.read(cx).entry_at(ix).cloned() else {
        return;
    };
    if let Some(items) = entry.submenu {
        let page = Page {
            title: entry.title.unwrap_or(entry.label),
            entries: items.to_vec(),
        };
        return show_page(&palette, Some(page), window, cx);
    }
    close(&palette, window, cx);
    (entry.run)(window, cx);
}

/// The icon a row shows when its entry names none: by kind, and for actions by a word in their name.
fn icon_for(entry: &Entry) -> SkyeIcon {
    if let Some(icon) = entry.icon {
        return icon;
    }
    let name = match entry.kind {
        Kind::Project => return SkyeIcon::Folder,
        Kind::Thread => return SkyeIcon::MessageCircle,
        Kind::Tab => return SkyeIcon::PanelBottom,
        Kind::Action(name) => name.rsplit("::").next().unwrap_or(name),
    };
    const BY_WORD: &[(&str, SkyeIcon)] = &[
        ("NewThread", SkyeIcon::NewThread),
        ("NewScratch", SkyeIcon::NewThread),
        ("NewSideChat", SkyeIcon::MessagesSquare),
        ("Settings", SkyeIcon::Settings),
        ("Theme", SkyeIcon::Palette),
        ("Script", SkyeIcon::Play),
        ("Forward", SkyeIcon::Plug),
        ("Terminal", SkyeIcon::SquareTerminal),
        ("Sidebar", SkyeIcon::PanelLeft),
        ("BottomPanel", SkyeIcon::PanelBottom),
        ("RightPanel", SkyeIcon::PanelRight),
        ("FilePicker", SkyeIcon::File),
        ("Find", SkyeIcon::Search),
        ("Search", SkyeIcon::Search),
        ("Browser", SkyeIcon::Browser),
        ("Device", SkyeIcon::Smartphone),
        ("Environment", SkyeIcon::Remote),
        ("Remote", SkyeIcon::Remote),
        ("Sharing", SkyeIcon::Remote),
        ("PullRequest", SkyeIcon::GitPullRequest),
        ("ThreadPr", SkyeIcon::GitPullRequest),
        ("Issues", SkyeIcon::CircleDot),
        ("Changes", SkyeIcon::FileDiff),
        ("Worktree", SkyeIcon::GitBranch),
        ("Branch", SkyeIcon::GitBranch),
        ("Commit", SkyeIcon::GitCommitVertical),
        ("Copy", SkyeIcon::Copy),
        ("Archive", SkyeIcon::Archive),
        ("Delete", SkyeIcon::Trash),
        ("Pin", SkyeIcon::Pin),
        ("Rename", SkyeIcon::PenLine),
        ("Project", SkyeIcon::FolderPlus),
        ("Folder", SkyeIcon::FolderOpen),
        ("Account", SkyeIcon::User),
        ("Update", SkyeIcon::RefreshCw),
        ("Stop", SkyeIcon::Square),
        ("Interrupt", SkyeIcon::Square),
        ("Schedule", SkyeIcon::Clock),
        ("Dictation", SkyeIcon::Mic),
        ("Editor", SkyeIcon::SquareCode),
        ("Gallery", SkyeIcon::Rows3),
        ("Debug", SkyeIcon::Code),
        ("Keymap", SkyeIcon::Keyboard),
        ("Window", SkyeIcon::Monitor),
        ("Quit", SkyeIcon::X),
        ("Codex", SkyeIcon::Codex),
        ("Claude", SkyeIcon::Claude),
        ("Thread", SkyeIcon::MessageCircle),
    ];
    BY_WORD
        .iter()
        .find(|(word, _)| name.contains(word))
        .map_or(SkyeIcon::SquareMenu, |&(_, icon)| icon)
}

fn row(entry: &Entry, query: &str) -> CommandRow {
    let title = entry.shown().clone();
    let strong = match_ranges(query, &title, entry.bold.clone());
    CommandRow::new(icon_for(entry), title)
        .strong(strong)
        .subtitle(entry.subtitle.clone())
        .checked(entry.checked)
        .shortcut(entry.binding.clone())
        .submenu(entry.submenu.is_some())
}

/// A key cap with T3's words ("Enter", not ↩).
fn cap(key: &str, label: &'static str) -> Kbd {
    Kbd::new(Keystroke {
        modifiers: Default::default(),
        key: key.into(),
        key_char: None,
    })
    .with_label(label)
}

/// T3's footer: ↑ ↓ Navigate, Enter Select, (Backspace Back), Esc Close.
fn footer(paged: bool, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let hint = |caps: Vec<Kbd>, text: &'static str| {
        h_flex()
            .gap_1()
            .children(caps)
            .child(div().ml_0p5().child(text))
    };
    h_flex()
        .w_full()
        .gap_3()
        .px_4()
        .py_2p5()
        .border_t_1()
        .border_color(theme.foreground.opacity(0.07))
        .bg(theme.foreground.opacity(0.025))
        .text_size(crate::ui::META)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(hint(vec![cap("up", "↑"), cap("down", "↓")], "Navigate"))
        .child(hint(vec![cap("enter", "Enter")], "Select"))
        .when(paged, |f| {
            f.child(hint(vec![cap("backspace", "Backspace")], "Back"))
        })
        .child(hint(
            vec![cap("escape", "Esc")],
            if paged { "Back" } else { "Close" },
        ))
}

/// Above a page's search field: ‹ and the page's title, back to the root on click (T3's Back addon).
fn back_row(
    title: SharedString,
    palette: WeakEntity<Palette>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    h_flex()
        .id("palette-back")
        .debug_selector(|| "palette-back".into())
        .mx_2()
        .mt_2()
        .px_2()
        .py_1()
        .gap_1p5()
        .rounded(crate::ui::RADIUS_MD)
        .text_size(crate::ui::META)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .hover(|s| s.bg(theme.foreground.opacity(0.06)))
        .child(Icon::new(SkyeIcon::ChevronLeft).size_3p5())
        .child(title)
        .on_click(move |_, window, cx| {
            if let Some(p) = palette.upgrade() {
                back(&p, window, cx);
            }
        })
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Palette");
        let list = self.list();
        let groups: Vec<CommandGroup> = self
            .groups
            .iter()
            .map(|g| {
                CommandGroup::new()
                    .label(g.label.clone())
                    .items(g.rows.iter().map(|&i| {
                        let (entry, query) = (list[i].clone(), self.query.clone());
                        CommandItem::new()
                            .label(entry.label.clone())
                            .child(move |_, _| row(&entry, &query))
                    }))
            })
            .collect();
        let this = cx.entity().downgrade();
        let paged = self.page.as_ref().map(|p| p.title.clone());
        let mut command = Command::new(&self.state)
            .filterable(false)
            .bordered(false)
            .bg(gpui::transparent_black())
            .max_h(px(420.))
            .placeholder(if paged.is_some() {
                "Search…"
            } else {
                PLACEHOLDER
            })
            .empty(|_, _, cx| {
                div()
                    .py_10()
                    .w_full()
                    .text_center()
                    .text_size(crate::ui::BODY)
                    .text_color(cx.theme().muted_foreground)
                    .child("No matching commands, projects, or threads.")
            })
            .footer({
                // A page opened on its own has no root to go back to: its keys close.
                let back = paged.is_some() && !self.entries.is_empty();
                move |_, _, cx| footer(back, cx)
            })
            .on_query({
                let this = this.clone();
                move |query, _, cx| {
                    // A confirm deferred after this query must index the rows it ranks, before any redraw.
                    _ = this.update(cx, |p, cx| p.set_query(query, cx));
                }
            })
            .on_confirm({
                let this = this.clone();
                move |ix, window, cx| choose(&this, ix, window, cx)
            });
        if let Some(title) = paged {
            let this = this.clone();
            command = command.header(move |_, _, cx| back_row(title.clone(), this.clone(), cx));
        }
        for group in groups {
            command = command.group(group);
        }
        let (on_escape, on_backspace) = (this.clone(), this);
        div()
            // Before the Command's own Escape, which first clears the query: T3 closes (or goes back) at once.
            .capture_action(move |_: &gpui_base::actions::Cancel, window, cx| {
                if let Some(p) = on_escape.upgrade() {
                    cx.stop_propagation();
                    back(&p, window, cx);
                }
            })
            .capture_action(move |_: &gpui_component::input::Backspace, window, cx| {
                let Some(p) = on_backspace.upgrade() else {
                    return;
                };
                let read = p.read(cx);
                if read.page.is_some() && read.state.read(cx).query(cx).is_empty() {
                    cx.stop_propagation();
                    back(&p, window, cx);
                }
            })
            .child(command)
    }
}

#[cfg(test)]
mod icon_test {
    use super::*;

    #[test]
    fn action_icons_match_whole_words_not_prefixes() {
        let icon = |name: &'static str| {
            icon_for(&Entry::new(
                "x",
                Kind::Action(name),
                std::rc::Rc::new(|_, _| {}),
            ))
        };
        // "Pr" once matched inside Project and Prompt.
        assert_eq!(icon("skyelab::AddProject"), SkyeIcon::FolderPlus);
        assert_eq!(icon("skyelab::SchedulePrompt"), SkyeIcon::Clock);
        assert_eq!(icon("skyelab::OpenThreadPr"), SkyeIcon::GitPullRequest);
        assert_eq!(icon("skyelab::OpenPullRequests"), SkyeIcon::GitPullRequest);
        assert_eq!(icon("skyelab::OpenSettings"), SkyeIcon::Settings);
    }
}
