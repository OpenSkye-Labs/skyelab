//! The palette's view: T3's card in a dialog, with gpui-component's `Command` for the list (its virtual list, arrow
//! keys that wrap, hover select and keep-in-view), our rows, pages for submenus, and the footer of key hints.

use super::{Entry, Group, Kind, PLACEHOLDER, Search, arrange, flatten, rank};
use crate::ui::RADIUS_XL;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, WeakEntity, Window, div, prelude::*, px,
};
#[path = "palette_rows.rs"]
mod rows;
pub(crate) use rows::footer;
use rows::{back_row, row, search_field};

use crate::ui::ActiveTheme as _;
use gpui::Subscription;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{
    IndexPath,
    command::{Command, CommandGroup, CommandItem, CommandState},
};

/// A page to open: its title, fixed rows, the rows found for each query (the empty one too), the field's hint.
#[derive(Clone)]
pub struct PageSpec {
    pub title: SharedString,
    pub entries: Vec<Entry>,
    pub search: Option<Search>,
    pub placeholder: Option<SharedString>,
}

impl PageSpec {
    pub fn new(title: impl Into<SharedString>, entries: Vec<Entry>) -> Self {
        Self {
            title: title.into(),
            entries,
            search: None,
            placeholder: None,
        }
    }

    pub fn search(mut self, search: Search) -> Self {
        self.search = Some(search);
        self
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = Some(text.into());
        self
    }
}

/// A submenu's items (or a [`PageSpec`]), shown instead of the root with a Back row.
struct Page {
    title: SharedString,
    /// The fixed rows, then (from `fixed`) the last search's.
    entries: Vec<Entry>,
    fixed: usize,
    search: Option<Search>,
    placeholder: Option<SharedString>,
}

impl Page {
    fn of(spec: PageSpec) -> Self {
        Self {
            fixed: spec.entries.len(),
            title: spec.title,
            entries: spec.entries,
            search: spec.search,
            placeholder: spec.placeholder,
        }
    }
}

struct Palette {
    state: Entity<CommandState>,
    /// The search field: ours, at T3's large size (the Command's own field is a fixed medium one).
    field: Entity<InputState>,
    /// The root's entries: top-level, then submenu items from `nested`, then the last search's from `fixed`.
    entries: Vec<Entry>,
    nested: usize,
    fixed: usize,
    page: Option<Page>,
    /// The pages under the shown one, for Back.
    stack: Vec<Page>,
    query: SharedString,
    /// What the list shows now, by `arrange`; indexes into [`Self::list`].
    groups: Vec<Group>,
    prev: FocusHandle,
    search: Option<Search>,
    _search: Option<Task<()>>,
    _field: Option<Subscription>,
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
    open_spec(PageSpec::new(title, entries), prev, window, cx);
}

/// Opens the palette straight on `spec` (the Add project page); Back from it closes.
pub fn open_spec(spec: PageSpec, prev: FocusHandle, window: &mut Window, cx: &mut App) {
    open_with(Vec::new(), Some(Page::of(spec)), None, prev, window, cx);
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
    let field = cx.new(|cx| InputState::new(window, cx));
    let palette = cx.new(|cx| {
        // Typing in our field drives the palette and keeps the Command's query (its Escape and empty state) in step.
        let sub = cx.subscribe_in(
            &field,
            window,
            |p: &mut Palette, field, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::Change) {
                    let query = field.read(cx).value();
                    p.set_query(&query, cx);
                    p.state.update(cx, |s, cx| s.set_query(query, window, cx));
                }
            },
        );
        let mut p = Palette {
            state: state.clone(),
            field: field.clone(),
            nested,
            fixed: entries.len(),
            entries,
            page,
            stack: Vec::new(),
            query: SharedString::default(),
            groups: Vec::new(),
            prev,
            search,
            _search: None,
            _field: Some(sub),
        };
        p.arrange();
        p.find_more(cx);
        p
    });
    #[cfg(all(test, unix))]
    cx.set_global(OpenPalette(palette.downgrade()));
    let shown = palette.clone();
    crate::ui::open_dialog(window, cx, move |dialog, _, _| {
        let palette = shown.clone();
        // T3's card: max-w-xl, rounded-2xl, the list and footer reaching its edges.
        // Rex's palette opens and closes in one frame.
        dialog
            .entrance(crate::ui::DialogEntrance::None)
            .w(px(600.))
            .p_0()
            .rounded(RADIUS_XL)
            .overflow_hidden()
            .close_button(false)
            .content(move |content, _, _| content.p_0().child(palette.clone()))
    });
    sync_field(&palette, "".into(), window, cx);
    field.update(cx, |f, cx| f.focus(window, cx));
}

/// Our field shows `query` (set without a change event) and the shown page's hint.
fn sync_field(palette: &Entity<Palette>, query: SharedString, window: &mut Window, cx: &mut App) {
    let (field, hint) = {
        let p = palette.read(cx);
        let hint = match &p.page {
            Some(page) => page.placeholder.clone().unwrap_or_else(|| "Search…".into()),
            None => PLACEHOLDER.into(),
        };
        (p.field.clone(), hint)
    };
    field.update(cx, |f, cx| {
        f.set_placeholder(hint, window, cx);
        if f.value() != query {
            f.set_value(query, window, cx);
        }
    });
}

impl Palette {
    fn list(&self) -> &[Entry] {
        self.page.as_ref().map_or(&self.entries, |p| &p.entries)
    }

    fn arrange(&mut self) {
        self.groups = match &self.page {
            Some(page) => {
                // By the shown title: a page's labels share a prefix ("Theme: …") that would match every row.
                let fixed = &page.entries[..page.fixed];
                let rows = rank(&self.query, fixed.iter().map(|e| e.shown().as_ref()));
                let label = page.title.clone();
                let mut groups: Vec<Group> = (!rows.is_empty())
                    .then_some(Group { label, rows })
                    .into_iter()
                    .collect();
                // Found rows keep the search's order, under their own sections.
                for (i, e) in page.entries.iter().enumerate().skip(page.fixed) {
                    let label = e.group.clone().unwrap_or_else(|| "Results".into());
                    match groups.iter_mut().find(|g| g.label == label) {
                        Some(g) => g.rows.push(i),
                        None => groups.push(Group {
                            label,
                            rows: vec![i],
                        }),
                    }
                }
                groups
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
        // A page searches every query, the empty one too (its recent rows); the root only a typed one.
        let search = match &self.page {
            Some(page) => page.search.clone(),
            None => self.search.clone().filter(|_| !query.trim().is_empty()),
        };
        let Some(search) = search else {
            self._search = None;
            return;
        };
        let task = search(query, cx);
        let paged = self.page.is_some();
        self._search = Some(cx.spawn(async move |p, cx| {
            let found = task.await;
            _ = p.update(cx, |p, cx| {
                if paged {
                    if let Some(page) = p.page.as_mut() {
                        page.entries.truncate(page.fixed);
                        page.entries.extend(found);
                    }
                    p.arrange();
                    return cx.notify();
                }
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
        if let Some(page) = self.page.as_mut() {
            page.entries.truncate(page.fixed);
        }
        self.arrange();
        self.find_more(cx);
        cx.notify();
    }
}

gpui::actions!(skyelab, [CompleteFolder]);

/// Tests: the palette opened last.
#[cfg(all(test, unix))]
struct OpenPalette(WeakEntity<Palette>);

#[cfg(all(test, unix))]
impl gpui::Global for OpenPalette {}

/// Tests: what the open palette shows.
#[cfg(all(test, unix))]
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// The page's title, `None` at the root.
    pub page: Option<String>,
    pub query: String,
    /// (section, title, subtitle) of each row, in order.
    pub rows: Vec<(String, String, String)>,
    /// The highlighted row.
    pub selected: Option<usize>,
}

#[cfg(all(test, unix))]
pub fn shown(cx: &App) -> Option<Shown> {
    let p = cx.try_global::<OpenPalette>()?.0.upgrade()?;
    let p = p.read(cx);
    let list = p.list();
    let mut rows = Vec::new();
    let mut selected = None;
    let at = p.state.read(cx).selected_index();
    for (gi, g) in p.groups.iter().enumerate() {
        for (ri, &i) in g.rows.iter().enumerate() {
            if at == Some(IndexPath::default().section(gi).row(ri)) {
                selected = Some(rows.len());
            }
            let e = &list[i];
            rows.push((
                g.label.to_string(),
                e.shown().to_string(),
                e.subtitle
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
            ));
        }
    }
    Some(Shown {
        page: p.page.as_ref().map(|pg| pg.title.to_string()),
        query: p.state.read(cx).query(cx).to_string(),
        rows,
        selected,
    })
}

/// The palette's own key context: Tab completes a folder there (Root's Tab would move focus).
const KEY_CONTEXT: &str = "SkyePalette";

/// Tab in the palette: complete the highlighted folder into the field.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([gpui::KeyBinding::new(
        "tab",
        CompleteFolder,
        Some(KEY_CONTEXT),
    )]);
}

/// Closes the palette and gives focus back to what had it.
fn close(palette: &Entity<Palette>, window: &mut Window, cx: &mut App) {
    let prev = palette.read(cx).prev.clone();
    crate::ui::close_dialog(window, cx);
    // The dialog sits outside the workspace, so the action must start from the element focused before it.
    window.focus(&prev, cx);
}

/// Opens `page` over the shown one, which Back returns to.
fn push_page(palette: &Entity<Palette>, page: Page, window: &mut Window, cx: &mut App) {
    palette.update(cx, |p, _| {
        if let Some(under) = p.page.take() {
            p.stack.push(under);
        }
    });
    show_page(palette, Some(page), window, cx);
}

/// Puts `query` in the field as if typed (a folder browsed into), the caret at its end.
fn set_field(palette: &Entity<Palette>, query: SharedString, window: &mut Window, cx: &mut App) {
    let state = palette.update(cx, |p, cx| {
        p.set_query(&query, cx);
        p.state.clone()
    });
    state.update(cx, |s, cx| {
        s.set_query(query.clone(), window, cx);
        s.set_selected_index(Some(IndexPath::default()), window, cx);
    });
    sync_field(palette, query, window, cx);
}

/// Tab: the highlighted row's folder into the field.
fn complete(palette: &Entity<Palette>, window: &mut Window, cx: &mut App) -> bool {
    let p = palette.read(cx);
    let ix = p.state.read(cx).selected_index().unwrap_or_default();
    let Some(query) = p.entry_at(ix).and_then(|e| e.browse.clone()) else {
        return false;
    };
    set_field(palette, query, window, cx);
    true
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
    sync_field(palette, "".into(), window, cx);
}

/// Escape, Backspace on an empty query and the Back row: a page goes back to the root; the root (or a page opened
/// on its own) closes.
fn back(palette: &Entity<Palette>, window: &mut Window, cx: &mut App) {
    let under = palette.update(cx, |p, _| p.stack.pop());
    if let Some(under) = under {
        return show_page(palette, Some(under), window, cx);
    }
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
    if let Some(query) = entry.browse {
        return set_field(&palette, query, window, cx);
    }
    if let Some(spec) = entry.page {
        return push_page(&palette, Page::of((*spec).clone()), window, cx);
    }
    if let Some(items) = entry.submenu {
        let spec = PageSpec::new(entry.title.unwrap_or(entry.label), items.to_vec());
        return push_page(&palette, Page::of(spec), window, cx);
    }
    close(&palette, window, cx);
    (entry.run)(window, cx);
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("Palette");
        let list = self.list();
        let page_title = self.page.as_ref().map(|p| p.title.clone());
        let groups: Vec<CommandGroup> = self
            .groups
            .iter()
            .map(|g| {
                let group = CommandGroup::new();
                // A page's own rows go unheaded: the back row above already names the page.
                let group = if page_title.as_ref() == Some(&g.label) {
                    group
                } else {
                    group.label(g.label.clone())
                };
                group.items(g.rows.iter().map(|&i| {
                    let (entry, query) = (list[i].clone(), self.query.clone());
                    CommandItem::new()
                        .label(entry.label.clone())
                        .child(move |_, _| row(&entry, &query))
                }))
            })
            .collect();
        let this = cx.entity().downgrade();
        let paged = self.page.as_ref().map(|p| p.title.clone());
        let completes = self.page.as_ref().is_some_and(|p| p.search.is_some());
        let stacked = !self.stack.is_empty();
        let mut command = Command::new(&self.state)
            .filterable(false)
            // Our own field in the header instead (T3's large search field); it drives the query.
            .searchable(false)
            .bordered(false)
            .bg(gpui::transparent_black())
            .max_h(px(420.))
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
                let back = paged.is_some() && (stacked || !self.entries.is_empty());
                move |_, _, cx| footer(back, completes, cx)
            })
            .on_confirm({
                let this = this.clone();
                move |ix, window, cx| choose(&this, ix, window, cx)
            });
        let field = self.field.clone();
        command = command.header({
            let this = this.clone();
            move |_, _, cx| {
                gpui_component::v_flex()
                    .children(paged.clone().map(|t| back_row(t, this.clone(), cx)))
                    .child(search_field(&field, cx))
                    .into_any_element()
            }
        });
        for group in groups {
            command = command.group(group);
        }
        let (on_escape, on_backspace, on_tab) = (this.clone(), this.clone(), this);
        div()
            .key_context(KEY_CONTEXT)
            .capture_action(move |_: &CompleteFolder, window, cx| {
                if let Some(p) = on_tab.upgrade()
                    && complete(&p, window, cx)
                {
                    cx.stop_propagation();
                }
            })
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
