//! The command palette: one key to find and run any action, switch project, open a thread or focus a tab.
//! Entries come from the workspace; this file ranks them and hosts gpui-component's `Command` in a dialog.

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, KeyBinding, NoAction,
    ParentElement as _, Render, SharedString, Styled as _, Task, WeakEntity, Window, actions, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
    command::{Command, CommandItem, CommandState},
    h_flex,
    kbd::Kbd,
};
use std::rc::Rc;

actions!(skyelab, [OpenCommandPalette]);

#[cfg(test)]
pub const OPEN_PALETTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// What an entry does when chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A registered action, by its full name (`skyelab::OpenSettings`).
    Action(&'static str),
    Project,
    Thread,
    Tab,
}

impl Kind {
    fn label(&self) -> &'static str {
        match self {
            Kind::Action(_) => "Action",
            Kind::Project => "Project",
            Kind::Thread => "Thread",
            Kind::Tab => "Tab",
        }
    }
}

/// Runs with focus already back on the element that had it before the palette opened.
pub type Run = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct Entry {
    pub label: SharedString,
    pub kind: Kind,
    pub binding: Option<Kbd>,
    /// A toggle's current state, shown as a checkbox and On/Off; `None` for anything else.
    pub checked: Option<bool>,
    pub run: Run,
}

/// The palette keys for one OS. `KeyBinding::new` can't resolve `secondary-`, so the caller picks.
pub fn bindings(mac: bool) -> Vec<KeyBinding> {
    let (k, p) = if mac {
        ("cmd-k", "cmd-shift-p")
    } else {
        ("ctrl-k", "ctrl-shift-p")
    };
    let mut keys = vec![
        KeyBinding::new(k, OpenCommandPalette, Some("Workspace")),
        KeyBinding::new(p, OpenCommandPalette, Some("Workspace")),
    ];
    // ctrl-k is readline's kill-line: a focused shell keeps it, ctrl-shift-p still opens the palette there.
    if !mac {
        keys.push(KeyBinding::new("ctrl-k", NoAction, Some("Terminal")));
    }
    keys
}

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings(cfg!(target_os = "macos")));
}

/// One char per char, so text indexes stay aligned (`İ` lowercases to two).
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn is_word_start(text: &[char], i: usize) -> bool {
    i == 0
        || !text[i - 1].is_alphanumeric()
        || (text[i - 1].is_lowercase() && text[i].is_uppercase())
}

/// How well `query` fuzzy-matches `text`, higher is better; `None` when its letters aren't all there in order.
/// Case and spaces in the query are ignored. Word starts and consecutive runs score; gaps cost.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(lower)
        .collect();
    let Some((&first, rest)) = query.split_first() else {
        return Some(0);
    };
    let text: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.iter().copied().map(lower).collect();
    (0..text.len())
        .filter(|&i| lower[i] == first)
        .filter_map(|start| {
            let mut score = if start == 0 { 3 } else { 0 };
            if is_word_start(&text, start) {
                score += 10;
            }
            let mut prev = start;
            for &c in rest {
                let i = (prev + 1..text.len()).find(|&i| lower[i] == c)?;
                let gap = (i - prev - 1) as i32;
                score += if gap == 0 { 8 } else { -gap.min(5) };
                if gap > 0 && is_word_start(&text, i) {
                    score += 6;
                }
                prev = i;
            }
            Some(score)
        })
        .max()
}

/// Indexes of the labels that match `query`, best first.
pub fn rank<'a>(query: &str, labels: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = labels
        .into_iter()
        .enumerate()
        .filter_map(|(i, label)| score(query, label).map(|s| (s, i)))
        .collect();
    scored.sort_by_key(|&(s, _)| std::cmp::Reverse(s));
    scored.into_iter().map(|(_, i)| i).collect()
}

const HUMAN_NAME_OVERRIDES: &[(&str, &str)] = &[
    ("skyelab::ToggleEnvironments", "Switch Machine"),
    ("skyelab::Stop", "Stop Agent"),
    ("skyelab::SchedulePrompt", "Schedule this prompt…"),
    ("skyelab::Interrupt", "Interrupt Agent"),
    ("skyelab::RemoteStop", "Stop Remote Agent"),
    ("skyelab::SendRemotePrompt", "Send Prompt to Remote"),
    ("skyelab::ToggleSharing", "Toggle Sharing on Tailnet"),
    ("skyelab::CheckForUpdates", "Check for Updates"),
    ("skyelab::OpenRemote", "Open Remote Access"),
    ("skyelab::BindFolder", "Bind Folder to Account"),
    ("skyelab::ToggleWorktree", "Toggle Thread Worktree"),
    ("skyelab::OpenInEditor", "Open in Editor"),
    ("skyelab::OpenFilePicker", "Go to File"),
    ("skyelab::SearchFiles", "Search in Files"),
    ("skyelab::JumpToThread1", "Jump to Thread 1"),
    ("skyelab::JumpToThread2", "Jump to Thread 2"),
    ("skyelab::JumpToThread3", "Jump to Thread 3"),
    ("skyelab::JumpToThread4", "Jump to Thread 4"),
    ("skyelab::JumpToThread5", "Jump to Thread 5"),
    ("skyelab::JumpToThread6", "Jump to Thread 6"),
    ("skyelab::JumpToThread7", "Jump to Thread 7"),
    ("skyelab::JumpToThread8", "Jump to Thread 8"),
    ("skyelab::JumpToThread9", "Jump to Thread 9"),
];

const HIDDEN: &[&str] = &[
    "skyelab::OpenCommandPalette",
    "skyelab::ConnectTyped",
    "skyelab::ConfirmDiscardWorktree",
    "skyelab::CancelDiscardWorktree",
    "skyelab::CancelEdit",
    "skyelab::OpenPickedInEditor",
];

/// `skyelab::OpenSettings` → "Open Settings": the table, else the CamelCase name split into words.
pub fn humanize(name: &str) -> String {
    if let Some((_, human)) = HUMAN_NAME_OVERRIDES.iter().find(|(n, _)| *n == name) {
        return (*human).to_owned();
    }
    let short = name.rsplit("::").next().unwrap_or(name);
    let mut out = String::new();
    let mut prev_lower = false;
    for c in short.chars() {
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase();
        out.push(c);
    }
    out
}

pub fn listed(name: &str) -> bool {
    name.starts_with("skyelab::") && !HIDDEN.contains(&name)
}

/// More entries for a query, found off the UI thread (the store's thread search over message text). They list after
/// the ranked entries, minus any label already there.
pub type Search = Rc<dyn Fn(String, &mut App) -> Task<Vec<Entry>>>;

struct Palette {
    state: Entity<CommandState>,
    entries: Vec<Entry>,
    row_to_entry: Vec<usize>,
    prev: FocusHandle,
    /// How many of `entries` are the fixed ones; the rest are the last search's.
    fixed: usize,
    search: Option<Search>,
    _search: Option<Task<()>>,
}

pub fn open(
    entries: Vec<Entry>,
    search: Option<Search>,
    prev: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let state = cx.new(|cx| CommandState::new(window, cx));
    let palette = cx.new(|_| Palette {
        state: state.clone(),
        row_to_entry: (0..entries.len()).collect(),
        fixed: entries.len(),
        entries,
        prev,
        search,
        _search: None,
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let palette = palette.clone();
        dialog
            .w(px(560.))
            .close_button(false)
            .content(move |content, _, _| content.child(palette.clone()))
    });
    state.update(cx, |state, cx| state.focus(window, cx));
}

fn choose(palette: &WeakEntity<Palette>, row: usize, window: &mut Window, cx: &mut App) {
    let Some(palette) = palette.upgrade() else {
        return;
    };
    let picked = {
        let p = palette.read(cx);
        p.row_to_entry
            .get(row)
            .map(|&i| (p.entries[i].run.clone(), p.prev.clone()))
    };
    let Some((run, prev)) = picked else {
        return;
    };
    window.close_dialog(cx);
    // The dialog sits outside the workspace, so the action must start from the element focused before it.
    window.focus(&prev, cx);
    run(window, cx);
}

fn row(entry: &Entry, cx: &App) -> gpui::Div {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .w_full()
        .gap_2()
        .justify_between()
        .child(entry.label.clone())
        .child(
            h_flex()
                .gap_2()
                .text_color(muted)
                .children(entry.checked.map(|on| {
                    let theme = cx.theme();
                    // Drawn, not a control: the row is what toggles (T3's box is pointer-events-none too).
                    let tick = div()
                        .size_3p5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .border_1()
                        .border_color(if on { theme.primary } else { muted })
                        .when(on, |b| {
                            b.bg(theme.primary).child(
                                Icon::new(IconName::Check)
                                    .xsmall()
                                    .text_color(theme.primary_foreground),
                            )
                        });
                    h_flex()
                        .gap_1()
                        .child(if on { "On" } else { "Off" })
                        .child(tick)
                }))
                .child(entry.kind.label())
                .children(entry.binding.clone()),
        )
}

impl Palette {
    /// Runs the search for `query` and appends what it finds; a newer query drops (cancels) an older search.
    fn find_more(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(search) = self.search.clone().filter(|_| !query.trim().is_empty()) else {
            self._search = None;
            return;
        };
        let task = search(query.to_owned(), cx);
        self._search = Some(cx.spawn(async move |p, cx| {
            let found = task.await;
            _ = p.update(cx, |p, cx| {
                p.entries.truncate(p.fixed);
                let shown: std::collections::HashSet<SharedString> = p
                    .row_to_entry
                    .iter()
                    .map(|&i| p.entries[i].label.clone())
                    .collect();
                for entry in found.into_iter().filter(|e| !shown.contains(&e.label)) {
                    p.row_to_entry.push(p.entries.len());
                    p.entries.push(entry);
                }
                cx.notify();
            });
        }));
    }
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.row_to_entry.iter().map(|&i| {
            let entry = self.entries[i].clone();
            CommandItem::new()
                .label(entry.label.clone())
                .child(move |_, cx| row(&entry, cx))
        });
        let this = cx.entity().downgrade();
        let on_query = this.clone();
        Command::new(&self.state)
            .filterable(false)
            .bordered(false)
            .placeholder("Search actions, projects, threads and tabs")
            .items(items)
            .on_query(move |query, _, cx| {
                // A confirm deferred after this query must index the rows it ranks, before any redraw.
                _ = on_query.update(cx, |p, cx| {
                    p.entries.truncate(p.fixed);
                    p.row_to_entry = rank(query, p.entries.iter().map(|e| e.label.as_ref()));
                    p.find_more(query, cx);
                    cx.notify();
                });
            })
            .on_confirm(move |ix, window, cx| choose(&this, ix.row, window, cx))
    }
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
